use std::cmp::Ordering;

use super::{
    solver::EPS,
    sparse::{Error, Perm, ScatteredVec, SparseMat, TriangleMat},
};

/// LU decomposition of a square matrix: `P_row * A * P_col = L * U`.
///
/// The basis matrix (the columns of the constraint matrix corresponding to
/// the current basic variables) is factored into lower-triangular `L` and
/// upper-triangular `U`, with optional row and column permutations for
/// numerical stability and sparsity. This factorisation is the core of
/// the simplex method — every pivot requires solving linear systems
/// `B * x = b`, which becomes `L * U * x = P * b`.
#[derive(Clone)]
pub struct LUFactors {
    lower: TriangleMat,
    upper: TriangleMat,
    /// Row permutation applied before the LU solve (for pivot stability).
    row_perm: Option<Perm>,
    /// Column permutation applied after the LU solve (for sparsity).
    col_perm: Option<Perm>,
}

/// Pre-allocated working buffers reused across LU solves to avoid repeated allocation.
#[derive(Clone, Debug)]
pub struct ScratchSpace {
    rhs: ScatteredVec,
    dense_rhs: Vec<f64>,
    mark_nonzero: MarkNonzero,
}

impl ScratchSpace {
    pub fn with_capacity(n: usize) -> ScratchSpace {
        ScratchSpace {
            rhs: ScatteredVec::empty(n),
            dense_rhs: vec![0.0; n],
            mark_nonzero: MarkNonzero::with_capacity(n),
        }
    }

    pub(crate) fn clear_sparse(&mut self, size: usize) {
        self.rhs.clear_and_resize(size);
        self.mark_nonzero.clear_and_resize(size);
    }
}

impl std::fmt::Debug for LUFactors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "L:\n{:?}", self.lower)?;
        writeln!(f, "U:\n{:?}", self.upper)?;
        writeln!(
            f,
            "row_perm.new2orig: {:?}",
            self.row_perm.as_ref().map(|p| &p.new2orig)
        )?;
        writeln!(
            f,
            "col_perm.new2orig: {:?}",
            self.col_perm.as_ref().map(|p| &p.new2orig)
        )?;
        Ok(())
    }
}

impl LUFactors {
    pub fn nnz(&self) -> usize {
        self.lower.nondiag.nnz() + self.upper.nondiag.nnz() + self.lower.cols()
    }

    /// Solve `A * x = rhs` for `x`, overwriting `rhs` with the solution.
    /// Uses the dense path (forward-substitute through L, back-substitute through U).
    pub fn solve_dense(&self, rhs: &mut [f64], scratch: &mut ScratchSpace) {
        scratch.dense_rhs.resize(rhs.len(), 0.0);

        if let Some(row_perm) = &self.row_perm {
            for (i, rhs_el) in rhs.iter().enumerate() {
                scratch.dense_rhs[row_perm.orig2new[i]] = *rhs_el;
            }
        } else {
            scratch.dense_rhs.copy_from_slice(rhs);
        }

        tri_solve_dense(&self.lower, Triangle::Lower, &mut scratch.dense_rhs);
        tri_solve_dense(&self.upper, Triangle::Upper, &mut scratch.dense_rhs);

        if let Some(col_perm) = &self.col_perm {
            for i in 0..rhs.len() {
                rhs[col_perm.new2orig[i]] = scratch.dense_rhs[i];
            }
        } else {
            rhs.copy_from_slice(&scratch.dense_rhs);
        }
    }

    /// Solve `A * x = rhs` for `x`, overwriting `rhs` with the solution.
    /// Uses the sparse path — only touches non-zero entries for efficiency.
    pub fn solve(&self, rhs: &mut ScatteredVec, scratch: &mut ScratchSpace) {
        if let Some(row_perm) = &self.row_perm {
            scratch.rhs.clear();
            for &i in &rhs.nonzero {
                let new_i = row_perm.orig2new[i];
                scratch.rhs.nonzero.push(new_i);
                scratch.rhs.is_nonzero[new_i] = true;
                scratch.rhs.values[new_i] = rhs.values[i];
            }
        } else {
            std::mem::swap(&mut scratch.rhs, rhs);
        }

        tri_solve_sparse(&self.lower, scratch);
        tri_solve_sparse(&self.upper, scratch);

        if let Some(col_perm) = &self.col_perm {
            rhs.clear();
            for &i in &scratch.rhs.nonzero {
                let new_i = col_perm.new2orig[i];
                rhs.nonzero.push(new_i);
                rhs.is_nonzero[new_i] = true;
                rhs.values[new_i] = scratch.rhs.values[i];
            }
        } else {
            std::mem::swap(rhs, &mut scratch.rhs);
        }
    }

    pub fn transpose(&self) -> LUFactors {
        LUFactors {
            lower: self.upper.transpose(),
            upper: self.lower.transpose(),
            row_perm: self.col_perm.clone(),
            col_perm: self.row_perm.clone(),
        }
    }
}

/// Compute the LU factorisation of a square matrix given column-wise access.
///
/// `get_col(c)` returns `(row_indices, values)` for column `c`.
/// `stability_coeff` controls the threshold for partial pivoting — a pivot
/// candidate must be at least `stability_coeff * max_abs` to be eligible,
/// which trades fill-in for numerical accuracy (0.1 is a typical value).
///
/// Returns `Err(SingularMatrix)` if the matrix is (numerically) singular.
pub fn lu_factorize<'a>(
    size: usize,
    get_col: impl Fn(usize) -> (&'a [usize], &'a [f64]),
    stability_coeff: f64,
    scratch: &mut ScratchSpace,
) -> Result<LUFactors, Error> {
    let col_perm = super::ordering::order_simple(size, |c| get_col(c).0);

    let mut orig_row2elt_count = vec![0; size];
    for col_rows in (0..size).map(|c| get_col(c).0) {
        for &orig_r in col_rows {
            orig_row2elt_count[orig_r] += 1;
        }
    }

    scratch.clear_sparse(size);

    let mut lower = SparseMat::new(size);
    let mut upper = SparseMat::new(size);
    let mut upper_diag = Vec::with_capacity(size);

    let mut new2orig_row = (0..size).collect::<Vec<_>>();
    let mut orig2new_row = new2orig_row.clone();

    for i_col in 0..size {
        let mat_col = get_col(col_perm.new2orig[i_col]);

        scratch.rhs.set(mat_col.0.iter().copied().zip(mat_col.1));

        scratch.mark_nonzero.run(
            &mut scratch.rhs,
            |new_i| lower.col_rows(new_i),
            |new_i| new_i < i_col,
            |orig_r| orig2new_row[orig_r],
        );

        for &orig_i in scratch.mark_nonzero.visited.iter().rev() {
            let new_i = orig2new_row[orig_i];
            if new_i < i_col {
                let x_val = scratch.rhs.values[orig_i];
                for (orig_r, coeff) in lower.col_iter(new_i) {
                    scratch.rhs.values[orig_r] -= x_val * coeff;
                }
            }
        }

        let pivot_orig_r = {
            let mut max_abs = 0.0;
            for &orig_r in &scratch.rhs.nonzero {
                if orig2new_row[orig_r] < i_col {
                    continue;
                }

                let abs = f64::abs(scratch.rhs.values[orig_r]);
                if abs > max_abs {
                    max_abs = abs;
                }
            }

            if max_abs < EPS {
                return Err(Error::SingularMatrix);
            }

            assert!(max_abs.is_normal());

            let mut best_orig_r = None;
            let mut best_elt_count = None;
            for &orig_r in &scratch.rhs.nonzero {
                if orig2new_row[orig_r] < i_col {
                    continue;
                }

                if f64::abs(scratch.rhs.values[orig_r]) >= stability_coeff * max_abs {
                    let elt_count = orig_row2elt_count[orig_r];
                    if best_elt_count.is_none() || best_elt_count.unwrap() > elt_count {
                        best_orig_r = Some(orig_r);
                        best_elt_count = Some(elt_count);
                    }
                }
            }
            best_orig_r.unwrap()
        };

        let pivot_val = scratch.rhs.values[pivot_orig_r];

        {
            let row = i_col;
            let orig_row = new2orig_row[row];
            let pivot_row = orig2new_row[pivot_orig_r];
            new2orig_row.swap(row, pivot_row);
            orig2new_row.swap(orig_row, pivot_orig_r);
        }

        for &orig_r in &scratch.rhs.nonzero {
            let val = scratch.rhs.values[orig_r];

            if val == 0.0 {
                continue;
            }

            let new_r = orig2new_row[orig_r];
            match new_r.cmp(&i_col) {
                Ordering::Less => upper.push(new_r, val),
                Ordering::Equal => upper_diag.push(pivot_val),
                Ordering::Greater => lower.push(orig_r, val / pivot_val),
            }
        }

        upper.seal_column();
        lower.seal_column();
    }

    // permute rows of lower to "new" indices.
    for i_col in 0..lower.cols() {
        for r in lower.col_rows_mut(i_col) {
            *r = orig2new_row[*r];
        }
    }

    let res = LUFactors {
        lower: TriangleMat {
            nondiag: lower,
            diag: None,
        },
        upper: TriangleMat {
            nondiag: upper,
            diag: Some(upper_diag),
        },
        row_perm: Some(Perm {
            orig2new: orig2new_row,
            new2orig: new2orig_row,
        }),
        col_perm: Some(col_perm),
    };

    Ok(res)
}

/// Determines which entries in the solution will be non-zero *before*
/// doing the actual arithmetic, by running a depth-first search on
/// the dependency graph of the triangular matrix.
///
/// This is a standard optimisation for sparse triangular solves — by
/// knowing the non-zero pattern up front, we only compute values we
/// actually need.
#[derive(Clone, Debug)]
struct MarkNonzero {
    dfs_stack: Vec<DfsStep>,
    is_visited: Vec<bool>,
    /// Indices in reverse topological order (the order we need to process them).
    visited: Vec<usize>,
}

#[derive(Clone, Debug)]
struct DfsStep {
    orig_i: usize,
    cur_child: usize,
}

impl MarkNonzero {
    fn with_capacity(n: usize) -> MarkNonzero {
        MarkNonzero {
            dfs_stack: Vec::with_capacity(n),
            is_visited: vec![false; n],
            visited: vec![],
        }
    }

    fn clear(&mut self) {
        assert!(self.dfs_stack.is_empty());
        for &i in &self.visited {
            self.is_visited[i] = false;
        }
        self.visited.clear();
    }

    fn clear_and_resize(&mut self, n: usize) {
        self.clear();
        self.dfs_stack.reserve(n);
        self.is_visited.resize(n, false);
    }

    // compute the non-zero elements of the result by dfs traversal
    fn run<'a>(
        &mut self,
        rhs: &mut ScatteredVec,
        get_children: impl Fn(usize) -> &'a [usize] + 'a,
        filter: impl Fn(usize) -> bool,
        orig2new_row: impl Fn(usize) -> usize,
    ) {
        self.clear();

        for &orig_r in &rhs.nonzero {
            let new_r = orig2new_row(orig_r);
            if !filter(new_r) {
                continue;
            }
            if self.is_visited[orig_r] {
                continue;
            }

            self.dfs_stack.push(DfsStep {
                orig_i: orig_r,
                cur_child: 0,
            });
            while !self.dfs_stack.is_empty() {
                let cur_step = self.dfs_stack.last_mut().unwrap();
                let new_i = orig2new_row(cur_step.orig_i);
                let children = if filter(new_i) {
                    get_children(new_i)
                } else {
                    &[]
                };
                if !self.is_visited[cur_step.orig_i] {
                    self.is_visited[cur_step.orig_i] = true;
                } else {
                    cur_step.cur_child += 1;
                }

                while cur_step.cur_child < children.len() {
                    let child_orig_r = children[cur_step.cur_child];
                    if !self.is_visited[child_orig_r] {
                        break;
                    }
                    cur_step.cur_child += 1;
                }

                if cur_step.cur_child < children.len() {
                    let i_child = cur_step.cur_child;
                    self.dfs_stack.push(DfsStep {
                        orig_i: children[i_child],
                        cur_child: 0,
                    });
                } else {
                    self.visited.push(cur_step.orig_i);
                    self.dfs_stack.pop();
                }
            }
        }

        for &i in &self.visited {
            if !rhs.is_nonzero[i] {
                rhs.is_nonzero[i] = true;
                rhs.nonzero.push(i)
            }
        }
    }
}

/// Which triangle of the matrix we're solving against.
enum Triangle {
    Lower,
    Upper,
}

/// Triangular solve against a dense right-hand side.
/// Lower: forward substitution (columns 0..n). Upper: back substitution (columns n..0).
fn tri_solve_dense(tri_mat: &TriangleMat, triangle: Triangle, rhs: &mut [f64]) {
    assert_eq!(tri_mat.rows(), rhs.len());
    match triangle {
        Triangle::Lower => {
            for col in 0..tri_mat.cols() {
                tri_solve_process_col(tri_mat, col, rhs);
            }
        }

        Triangle::Upper => {
            for col in (0..tri_mat.cols()).rev() {
                tri_solve_process_col(tri_mat, col, rhs);
            }
        }
    };
}

/// rhs is passed via scratch.visited, scratch.values.
fn tri_solve_sparse(tri_mat: &TriangleMat, scratch: &mut ScratchSpace) {
    assert_eq!(tri_mat.rows(), scratch.rhs.len());

    // compute the non-zero elements of the result by dfs traversal
    scratch.mark_nonzero.run(
        &mut scratch.rhs,
        |col| tri_mat.nondiag.col_rows(col),
        |_| true,
        |orig_i| orig_i,
    );

    // solve for the non-zero values into dense workspace.
    // rev() because DFS returns vertices in reverse topological order.
    for &col in scratch.mark_nonzero.visited.iter().rev() {
        tri_solve_process_col(tri_mat, col, &mut scratch.rhs.values);
    }
}

/// Process one column during triangular substitution: divide by the diagonal
/// (if present), then subtract contributions from all off-diagonal entries.
fn tri_solve_process_col(tri_mat: &TriangleMat, col: usize, rhs: &mut [f64]) {
    let x_val = if let Some(diag) = tri_mat.diag.as_ref() {
        rhs[col] / diag[col]
    } else {
        rhs[col]
    };

    rhs[col] = x_val;
    for (r, &coeff) in tri_mat.nondiag.col_iter(col) {
        rhs[r] -= x_val * coeff;
    }
}
