// Vendored from microlp v0.4.0 — stripped of integer solving (solve_integer, branch-and-bound).

use microlp::{ComparisonOp, Error, StopReason, VarDomain};
use sprs::CompressedStorage;
use web_time::Instant;

use super::{
    helpers::{resized_view, to_dense},
    lu::{LUFactors, ScratchSpace, lu_factorize},
    sparse::{ScatteredVec, SparseMat, SparseVec},
};

pub(crate) type CsVec = sprs::CsVecI<f64, usize>;
/// Optional wall-clock deadline — if `Some`, the solver periodically checks
/// whether the deadline has passed and returns `StopReason::Limit` if so.
pub(crate) type Deadline = Option<Instant>;

type CsMat = sprs::CsMatI<f64, usize>;

/// Tolerances for floating-point comparisons. Values closer than `EPS` are
/// considered equal. This avoids infinite cycling in the simplex algorithm
/// caused by tiny rounding differences.
pub(crate) const MACHINE_EPS: f64 = f64::EPSILON * 10.0;
pub const EPS: f64 = if MACHINE_EPS > 1e-8 {
    MACHINE_EPS
} else {
    1e-10
};

pub(crate) fn float_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}
pub(crate) fn float_ne(a: f64, b: f64) -> bool {
    !float_eq(a, b)
}

#[inline]
fn check_deadline(deadline: &Deadline) -> StopReason {
    if let Some(dl) = deadline {
        if Instant::now() >= *dl {
            return StopReason::Limit;
        }
    }
    StopReason::Finished
}

/// Revised simplex method solver for linear programs of the form:
///
/// ```text
///   minimise   c^T x
///   subject to A x {≤, ≥, =} b
///              lb ≤ x ≤ ub
/// ```
///
/// The simplex method works by maintaining a "basis" — a subset of variables
/// whose values are determined by the constraints (the rest sit at their
/// bounds). Each iteration ("pivot") swaps one variable into the basis and
/// one out, improving the objective until optimality is reached.
///
/// This solver supports both the **primal** simplex (used when the current
/// solution is feasible but not optimal) and the **dual** simplex (used to
/// restore feasibility after adding a constraint).
#[derive(Clone)]
pub(crate) struct Solver {
    /// Number of original (non-slack) decision variables.
    pub(crate) num_vars: usize,
    pub(crate) deadline: Deadline,

    // ── Problem data (immutable after construction) ──────────────────────
    /// Objective function coefficients for all variables (original + slack).
    orig_obj_coeffs: Vec<f64>,
    /// Lower bounds for all variables.
    orig_var_mins: Vec<f64>,
    /// Upper bounds for all variables.
    orig_var_maxs: Vec<f64>,
    pub(crate) orig_var_domains: Vec<VarDomain>,
    /// Constraint matrix in CSR format (rows = constraints, columns = all vars including slack).
    orig_constraints: CsMat,
    /// Same matrix in CSC format (for efficient column access during pivots).
    orig_constraints_csc: CsMat,
    /// Right-hand side of each constraint.
    orig_rhs: Vec<f64>,

    // ── Algorithm control ────────────────────────────────────────────────
    /// Steepest-edge pricing: improves pivot selection at the cost of
    /// maintaining squared norms. Generally makes the solver converge in
    /// fewer iterations.
    enable_primal_steepest_edge: bool,
    enable_dual_steepest_edge: bool,

    /// Feasibility flags — these determine which phase of the simplex
    /// method is active (restore feasibility vs. optimise).
    is_primal_feasible: bool,
    is_dual_feasible: bool,

    // ── Basis state (updated on each pivot) ──────────────────────────────
    /// For each variable: whether it is basic or non-basic, and its index
    /// within the corresponding array.
    var_states: Vec<VarState>,
    /// LU factorisation of the current basis matrix, used to solve
    /// `B * x = b` and `B^T * y = c` at each pivot.
    basis_solver: BasisSolver,

    /// For each constraint (row), the variable currently in the basis.
    basic_vars: Vec<usize>,
    /// Current values of the basic variables.
    basic_var_vals: Vec<f64>,
    basic_var_mins: Vec<f64>,
    basic_var_maxs: Vec<f64>,
    /// Squared norms for dual steepest-edge pricing.
    dual_edge_sq_norms: Vec<f64>,

    /// Non-basic variables (the ones sitting at a bound). 'nb' = non-basic.
    nb_vars: Vec<usize>,
    /// Reduced costs of the non-basic variables (how much the objective
    /// improves per unit increase of each non-basic variable).
    nb_var_obj_coeffs: Vec<f64>,
    nb_var_vals: Vec<f64>,
    nb_var_states: Vec<NonBasicVarState>,
    nb_var_is_fixed: Vec<bool>,
    /// Squared norms for primal steepest-edge pricing.
    primal_edge_sq_norms: Vec<f64>,

    /// Current objective function value.
    pub(crate) cur_obj_val: f64,

    // ── Scratch space (recomputed on each pivot) ─────────────────────────
    /// Column of the basis-inverse times the entering variable's constraint column.
    col_coeffs: SparseVec,
    sq_norms_update_helper: Vec<f64>,
    inv_basis_row_coeffs: SparseVec,
    /// Row of reduced costs for the leaving variable's constraint row.
    row_coeffs: ScatteredVec,
}

/// Tracks whether a variable is currently in the basis or not, and its
/// index within the `basic_vars` or `nb_vars` array respectively.
#[derive(Clone, Debug)]
enum VarState {
    /// In the basis — the `usize` is the index into `basic_vars` / `basic_var_vals`.
    Basic(usize),
    /// Not in the basis — the `usize` is the index into `nb_vars` / `nb_var_vals`.
    NonBasic(usize),
}

/// For a non-basic variable, records whether it is currently sitting at
/// its lower bound, upper bound, or neither (free variable).
#[derive(Clone, Debug)]
struct NonBasicVarState {
    at_min: bool,
    at_max: bool,
}

impl std::fmt::Debug for Solver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Solver")?;
        writeln!(
            f,
            "num_vars: {}, num_constraints: {}, is_primal_feasible: {}, is_dual_feasible: {}",
            self.num_vars,
            self.num_constraints(),
            self.is_primal_feasible,
            self.is_dual_feasible,
        )?;
        writeln!(f, "orig_obj_coeffs:\n{:?}", self.orig_obj_coeffs)?;
        writeln!(f, "orig_var_mins:\n{:?}", self.orig_var_mins)?;
        writeln!(f, "orig_var_maxs:\n{:?}", self.orig_var_maxs)?;
        writeln!(f, "orig_constraints:")?;
        for row in self.orig_constraints.outer_iterator() {
            writeln!(f, "{:?}", to_dense(&row))?;
        }
        writeln!(f, "orig_rhs:\n{:?}", self.orig_rhs)?;
        writeln!(f, "basic_vars:\n{:?}", self.basic_vars)?;
        writeln!(f, "basic_var_vals:\n{:?}", self.basic_var_vals)?;
        writeln!(f, "dual_edge_sq_norms:\n{:?}", self.dual_edge_sq_norms)?;
        writeln!(f, "nb_vars:\n{:?}", self.nb_vars)?;
        writeln!(f, "nb_var_vals:\n{:?}", self.nb_var_vals)?;
        writeln!(f, "nb_var_obj_coeffs:\n{:?}", self.nb_var_obj_coeffs)?;
        writeln!(f, "primal_edge_sq_norms:\n{:?}", self.primal_edge_sq_norms)?;
        writeln!(f, "cur_obj_val: {:?}", self.cur_obj_val)?;
        Ok(())
    }
}

impl Solver {
    pub(crate) fn try_new(
        obj_coeffs: &[f64],
        var_mins: &[f64],
        var_maxs: &[f64],
        constraints: &[(CsVec, ComparisonOp, f64)],
        var_domains: &[VarDomain],
        deadline: Deadline,
    ) -> Result<Self, Error> {
        let enable_steepest_edge = true;

        let num_vars = obj_coeffs.len();

        assert_eq!(num_vars, var_mins.len());
        assert_eq!(num_vars, var_maxs.len());
        let mut orig_var_mins = var_mins.to_vec();
        let mut orig_var_maxs = var_maxs.to_vec();

        let mut var_states = vec![];

        let mut nb_vars = vec![];
        let mut nb_var_vals = vec![];
        let mut nb_var_states = vec![];

        let mut obj_val = 0.0;

        let mut is_dual_feasible = true;

        for v in 0..num_vars {
            let min = orig_var_mins[v];
            let max = orig_var_maxs[v];
            if min > max {
                return Err(Error::Infeasible);
            }

            var_states.push(VarState::NonBasic(nb_vars.len()));
            nb_vars.push(v);

            let init_val = if float_eq(min, max) {
                min
            } else if min.is_infinite() && max.is_infinite() {
                if float_ne(obj_coeffs[v], 0.0) {
                    is_dual_feasible = false;
                }
                0.0
            } else if obj_coeffs[v] > 0.0 {
                if min.is_finite() {
                    min
                } else {
                    is_dual_feasible = false;
                    max
                }
            } else if obj_coeffs[v] < 0.0 {
                if max.is_finite() {
                    max
                } else {
                    is_dual_feasible = false;
                    min
                }
            } else if min.is_finite() {
                min
            } else {
                max
            };

            nb_var_vals.push(init_val);
            obj_val += init_val * obj_coeffs[v];

            nb_var_states.push(NonBasicVarState {
                at_min: float_eq(init_val, min),
                at_max: float_eq(init_val, max),
            });
        }

        let mut constraint_coeffs = vec![];
        let mut orig_rhs = vec![];

        let mut basic_vars = vec![];
        let mut basic_var_vals = vec![];
        let mut basic_var_mins = vec![];
        let mut basic_var_maxs = vec![];

        for (coeffs, cmp_op, rhs) in constraints {
            let rhs = *rhs;

            if coeffs.indices().is_empty() {
                let is_tautological = match cmp_op {
                    ComparisonOp::Eq => float_eq(rhs, 0.0),
                    ComparisonOp::Le => 0.0 <= rhs,
                    ComparisonOp::Ge => 0.0 >= rhs,
                };

                if is_tautological {
                    continue;
                } else {
                    return Err(Error::Infeasible);
                }
            }

            constraint_coeffs.push(coeffs.clone());
            orig_rhs.push(rhs);

            let (slack_var_min, slack_var_max) = match cmp_op {
                ComparisonOp::Le => (0.0, f64::INFINITY),
                ComparisonOp::Ge => (f64::NEG_INFINITY, 0.0),
                ComparisonOp::Eq => (0.0, 0.0),
            };

            orig_var_mins.push(slack_var_min);
            orig_var_maxs.push(slack_var_max);

            basic_var_mins.push(slack_var_min);
            basic_var_maxs.push(slack_var_max);

            let cur_slack_var = var_states.len();
            var_states.push(VarState::Basic(basic_vars.len()));
            basic_vars.push(cur_slack_var);

            let mut lhs_val = 0.0;
            for (var, &coeff) in coeffs.iter() {
                lhs_val += coeff * nb_var_vals[var];
            }
            basic_var_vals.push(rhs - lhs_val);
        }

        let num_constraints = constraint_coeffs.len();
        let num_total_vars = num_vars + num_constraints;

        let mut orig_obj_coeffs = obj_coeffs.to_vec();
        orig_obj_coeffs.resize(num_total_vars, 0.0);

        let mut orig_constraints = CsMat::empty(CompressedStorage::CSR, num_total_vars);
        for (cur_slack_var, coeffs) in constraint_coeffs.into_iter().enumerate() {
            let mut coeffs = into_resized(coeffs, num_total_vars);
            coeffs.append(num_vars + cur_slack_var, 1.0);
            orig_constraints = orig_constraints.append_outer_csvec(coeffs.view());
        }
        let orig_constraints_csc = orig_constraints.to_csc();

        let is_primal_feasible = basic_var_vals
            .iter()
            .zip(&basic_var_mins)
            .zip(&basic_var_maxs)
            .all(|((&val, &min), &max)| val >= min && val <= max);

        let need_artificial_obj = !is_primal_feasible && !is_dual_feasible;

        let enable_dual_steepest_edge = enable_steepest_edge;
        let dual_edge_sq_norms = if enable_dual_steepest_edge {
            vec![1.0; basic_vars.len()]
        } else {
            vec![]
        };

        let enable_primal_steepest_edge = enable_steepest_edge && !is_dual_feasible;
        let sq_norms_update_helper = if enable_primal_steepest_edge {
            vec![0.0; num_total_vars - num_constraints]
        } else {
            vec![]
        };

        let mut nb_var_obj_coeffs = vec![];
        let mut primal_edge_sq_norms = vec![];
        for (&var, state) in nb_vars.iter().zip(&nb_var_states) {
            let col = orig_constraints_csc.outer_view(var).unwrap();

            if need_artificial_obj {
                let coeff = if state.at_min && !state.at_max {
                    1.0
                } else if state.at_max && !state.at_min {
                    -1.0
                } else {
                    0.0
                };
                nb_var_obj_coeffs.push(coeff);
            } else {
                nb_var_obj_coeffs.push(orig_obj_coeffs[var]);
            }

            if enable_primal_steepest_edge {
                primal_edge_sq_norms.push(col.squared_l2_norm() + 1.0);
            }
        }

        let cur_obj_val = if need_artificial_obj { 0.0 } else { obj_val };

        let mut scratch = ScratchSpace::with_capacity(num_constraints);
        let lu_factors = lu_factorize(
            basic_vars.len(),
            |c| {
                orig_constraints_csc
                    .outer_view(basic_vars[c])
                    .unwrap()
                    .into_raw_storage()
            },
            0.1,
            &mut scratch,
        )
        .map_err(|e| Error::InternalError(e.to_string()))?;
        let lu_factors_transp = lu_factors.transpose();

        let nb_var_is_fixed = vec![false; nb_vars.len()];

        let res = Self {
            num_vars,
            orig_obj_coeffs,
            orig_var_mins,
            orig_var_maxs,
            orig_constraints,
            orig_constraints_csc,
            orig_rhs,
            deadline,
            orig_var_domains: var_domains.to_vec(),
            enable_primal_steepest_edge,
            enable_dual_steepest_edge,
            is_primal_feasible,
            is_dual_feasible,
            var_states,
            basis_solver: BasisSolver {
                lu_factors,
                lu_factors_transp,
                scratch,
                eta_matrices: EtaMatrices::new(num_constraints),
                rhs: ScatteredVec::empty(num_constraints),
            },
            basic_vars,
            basic_var_vals,
            basic_var_mins,
            basic_var_maxs,
            dual_edge_sq_norms,
            nb_vars,
            nb_var_obj_coeffs,
            nb_var_vals,
            nb_var_states,
            nb_var_is_fixed,
            primal_edge_sq_norms,
            cur_obj_val,
            col_coeffs: SparseVec::new(),
            sq_norms_update_helper,
            inv_basis_row_coeffs: SparseVec::new(),
            row_coeffs: ScatteredVec::empty(num_total_vars - num_constraints),
        };

        Ok(res)
    }

    /// Like `try_new`, but accepts a pre-built CSR constraint matrix instead of
    /// individual `CsVec` rows.  This avoids ~N per-row `CsVec` allocations
    /// when the caller can build the matrix in bulk (e.g. via `TriMatI`).
    ///
    /// `constraint_matrix_csr` must be `(num_constraints × num_vars)` in CSR
    /// storage.  `constraint_ops` and `constraint_rhs` must have length
    /// `num_constraints` and correspond row-by-row to the matrix.
    pub(crate) fn try_new_from_matrix(
        obj_coeffs: &[f64],
        var_mins: &[f64],
        var_maxs: &[f64],
        constraint_matrix_csr: CsMat, // (num_constraints × num_vars), CSR
        constraint_ops: &[ComparisonOp],
        constraint_rhs: &[f64],
        var_domains: &[VarDomain],
        deadline: Deadline,
    ) -> Result<Self, Error> {
        let enable_steepest_edge = true;

        let num_vars = obj_coeffs.len();
        assert_eq!(num_vars, var_mins.len());
        assert_eq!(num_vars, var_maxs.len());
        assert_eq!(constraint_ops.len(), constraint_rhs.len());

        let mut orig_var_mins = var_mins.to_vec();
        let mut orig_var_maxs = var_maxs.to_vec();

        let mut var_states = vec![];
        let mut nb_vars = vec![];
        let mut nb_var_vals = vec![];
        let mut nb_var_states = vec![];
        let mut obj_val = 0.0;
        let mut is_dual_feasible = true;

        // --- variable initialisation (identical to try_new) ---
        for v in 0..num_vars {
            let min = orig_var_mins[v];
            let max = orig_var_maxs[v];
            if min > max {
                return Err(Error::Infeasible);
            }

            var_states.push(VarState::NonBasic(nb_vars.len()));
            nb_vars.push(v);

            let init_val = if float_eq(min, max) {
                min
            } else if min.is_infinite() && max.is_infinite() {
                if float_ne(obj_coeffs[v], 0.0) {
                    is_dual_feasible = false;
                }
                0.0
            } else if obj_coeffs[v] > 0.0 {
                if min.is_finite() {
                    min
                } else {
                    is_dual_feasible = false;
                    max
                }
            } else if obj_coeffs[v] < 0.0 {
                if max.is_finite() {
                    max
                } else {
                    is_dual_feasible = false;
                    min
                }
            } else if min.is_finite() {
                min
            } else {
                max
            };

            nb_var_vals.push(init_val);
            obj_val += init_val * obj_coeffs[v];

            nb_var_states.push(NonBasicVarState {
                at_min: float_eq(init_val, min),
                at_max: float_eq(init_val, max),
            });
        }

        // --- process constraints from the CSR matrix ---
        // We need to handle empty rows exactly as try_new does: skip tautological
        // ones and error on infeasible ones.  Non-empty rows become actual
        // constraints with slack variables.

        let input_rows = constraint_matrix_csr.rows();

        // First pass: determine which rows survive (non-empty)
        let mut kept_row_indices: Vec<usize> = Vec::with_capacity(input_rows);
        for i in 0..input_rows {
            let row_view = constraint_matrix_csr.outer_view(i).unwrap();
            if row_view.nnz() == 0 {
                let rhs = constraint_rhs[i];
                let is_tautological = match constraint_ops[i] {
                    ComparisonOp::Eq => float_eq(rhs, 0.0),
                    ComparisonOp::Le => 0.0 <= rhs,
                    ComparisonOp::Ge => 0.0 >= rhs,
                };
                if !is_tautological {
                    return Err(Error::Infeasible);
                }
                // tautological — skip
            } else {
                kept_row_indices.push(i);
            }
        }

        let num_constraints = kept_row_indices.len();
        let num_total_vars = num_vars + num_constraints;

        let mut orig_rhs = Vec::with_capacity(num_constraints);
        let mut basic_vars = Vec::with_capacity(num_constraints);
        let mut basic_var_vals = Vec::with_capacity(num_constraints);
        let mut basic_var_mins = Vec::with_capacity(num_constraints);
        let mut basic_var_maxs = Vec::with_capacity(num_constraints);

        // Build the augmented CSR matrix (original cols + slack identity cols)
        // from the input matrix's raw CSR storage.
        let src_indptr = constraint_matrix_csr.indptr();
        let src_indices = constraint_matrix_csr.indices();
        let src_data = constraint_matrix_csr.data();

        let mut aug_indptr: Vec<usize> = Vec::with_capacity(num_constraints + 1);
        // Upper bound on nnz: original nnz (for kept rows) + num_constraints (slack 1.0 each)
        let mut aug_indices: Vec<usize> = Vec::new();
        let mut aug_data: Vec<f64> = Vec::new();

        aug_indptr.push(0);

        for (slack_idx, &src_row) in kept_row_indices.iter().enumerate() {
            let rhs = constraint_rhs[src_row];
            let cmp_op = constraint_ops[src_row];

            orig_rhs.push(rhs);

            let (slack_var_min, slack_var_max) = match cmp_op {
                ComparisonOp::Le => (0.0, f64::INFINITY),
                ComparisonOp::Ge => (f64::NEG_INFINITY, 0.0),
                ComparisonOp::Eq => (0.0, 0.0),
            };

            orig_var_mins.push(slack_var_min);
            orig_var_maxs.push(slack_var_max);
            basic_var_mins.push(slack_var_min);
            basic_var_maxs.push(slack_var_max);

            let cur_slack_var = var_states.len();
            var_states.push(VarState::Basic(basic_vars.len()));
            basic_vars.push(cur_slack_var);

            // Compute lhs_val = sum(coeff * nb_var_vals[var]) for this row
            let row_start = src_indptr.as_slice().unwrap()[src_row];
            let row_end = src_indptr.as_slice().unwrap()[src_row + 1];
            let mut lhs_val = 0.0;
            for pos in row_start..row_end {
                let col = src_indices[pos];
                let coeff = src_data[pos];
                // All original variables are non-basic at this point, with
                // nb index == var index (they were added in order 0..num_vars).
                lhs_val += coeff * nb_var_vals[col];
            }
            basic_var_vals.push(rhs - lhs_val);

            // Append this row's entries to the augmented CSR arrays.
            // Original columns first (already sorted in CSR), then the slack col.
            for pos in row_start..row_end {
                let col = src_indices[pos];
                if col < num_total_vars {
                    aug_indices.push(col);
                    aug_data.push(src_data[pos]);
                }
            }
            // Append slack column entry: column = num_vars + slack_idx, value = 1.0
            let slack_col = num_vars + slack_idx;
            aug_indices.push(slack_col);
            aug_data.push(1.0);

            aug_indptr.push(aug_indices.len());
        }

        let orig_constraints = CsMat::new(
            (num_constraints, num_total_vars),
            aug_indptr,
            aug_indices,
            aug_data,
        );
        let orig_constraints_csc = orig_constraints.to_csc();

        // --- remainder is identical to try_new ---

        let mut orig_obj_coeffs = obj_coeffs.to_vec();
        orig_obj_coeffs.resize(num_total_vars, 0.0);

        let is_primal_feasible = basic_var_vals
            .iter()
            .zip(&basic_var_mins)
            .zip(&basic_var_maxs)
            .all(|((&val, &min), &max)| val >= min && val <= max);

        let need_artificial_obj = !is_primal_feasible && !is_dual_feasible;

        let enable_dual_steepest_edge = enable_steepest_edge;
        let dual_edge_sq_norms = if enable_dual_steepest_edge {
            vec![1.0; basic_vars.len()]
        } else {
            vec![]
        };

        let enable_primal_steepest_edge = enable_steepest_edge && !is_dual_feasible;
        let sq_norms_update_helper = if enable_primal_steepest_edge {
            vec![0.0; num_total_vars - num_constraints]
        } else {
            vec![]
        };

        let mut nb_var_obj_coeffs = vec![];
        let mut primal_edge_sq_norms = vec![];
        for (&var, state) in nb_vars.iter().zip(&nb_var_states) {
            let col = orig_constraints_csc.outer_view(var).unwrap();

            if need_artificial_obj {
                let coeff = if state.at_min && !state.at_max {
                    1.0
                } else if state.at_max && !state.at_min {
                    -1.0
                } else {
                    0.0
                };
                nb_var_obj_coeffs.push(coeff);
            } else {
                nb_var_obj_coeffs.push(orig_obj_coeffs[var]);
            }

            if enable_primal_steepest_edge {
                primal_edge_sq_norms.push(col.squared_l2_norm() + 1.0);
            }
        }

        let cur_obj_val = if need_artificial_obj { 0.0 } else { obj_val };

        let mut scratch = ScratchSpace::with_capacity(num_constraints);
        let lu_factors = lu_factorize(
            basic_vars.len(),
            |c| {
                orig_constraints_csc
                    .outer_view(basic_vars[c])
                    .unwrap()
                    .into_raw_storage()
            },
            0.1,
            &mut scratch,
        )
        .map_err(|e| Error::InternalError(e.to_string()))?;
        let lu_factors_transp = lu_factors.transpose();

        let nb_var_is_fixed = vec![false; nb_vars.len()];

        let res = Self {
            num_vars,
            orig_obj_coeffs,
            orig_var_mins,
            orig_var_maxs,
            orig_constraints,
            orig_constraints_csc,
            orig_rhs,
            deadline,
            orig_var_domains: var_domains.to_vec(),
            enable_primal_steepest_edge,
            enable_dual_steepest_edge,
            is_primal_feasible,
            is_dual_feasible,
            var_states,
            basis_solver: BasisSolver {
                lu_factors,
                lu_factors_transp,
                scratch,
                eta_matrices: EtaMatrices::new(num_constraints),
                rhs: ScatteredVec::empty(num_constraints),
            },
            basic_vars,
            basic_var_vals,
            basic_var_mins,
            basic_var_maxs,
            dual_edge_sq_norms,
            nb_vars,
            nb_var_obj_coeffs,
            nb_var_vals,
            nb_var_states,
            nb_var_is_fixed,
            primal_edge_sq_norms,
            cur_obj_val,
            col_coeffs: SparseVec::new(),
            sq_norms_update_helper,
            inv_basis_row_coeffs: SparseVec::new(),
            row_coeffs: ScatteredVec::empty(num_total_vars - num_constraints),
        };

        Ok(res)
    }

    pub(crate) fn get_value(&self, var: usize) -> &f64 {
        match self.var_states[var] {
            VarState::Basic(idx) => &self.basic_var_vals[idx],
            VarState::NonBasic(idx) => &self.nb_var_vals[idx],
        }
    }

    pub(crate) fn num_constraints(&self) -> usize {
        self.orig_constraints.rows()
    }

    fn num_total_vars(&self) -> usize {
        self.num_vars + self.num_constraints()
    }

    /// Run the full solve: first restore primal feasibility (dual simplex),
    /// then optimise the objective (primal simplex). This is the main entry
    /// point after constructing a Solver.
    pub(crate) fn initial_solve(&mut self) -> Result<StopReason, Error> {
        if check_deadline(&self.deadline) == StopReason::Limit {
            return Ok(StopReason::Limit);
        }

        if !self.is_primal_feasible && self.restore_feasibility()? == StopReason::Limit {
            return Ok(StopReason::Limit);
        }

        if !self.is_dual_feasible {
            self.recalc_obj_coeffs()?;
            if self.optimize()? == StopReason::Limit {
                return Ok(StopReason::Limit);
            }
        }

        self.enable_primal_steepest_edge = false;

        Ok(StopReason::Finished)
    }

    /// Primal simplex loop: repeatedly pick the best entering variable
    /// (choose_pivot) and perform the basis exchange (pivot) until no
    /// improving variable remains, meaning we've reached the optimum.
    fn optimize(&mut self) -> Result<StopReason, Error> {
        for iter in 0.. {
            if iter % 1000 == 0 {
                if check_deadline(&self.deadline) == StopReason::Limit {
                    return Ok(StopReason::Limit);
                }
            }

            if let Some(pivot_info) = self.choose_pivot()? {
                self.pivot(&pivot_info)?;
            } else {
                break;
            }
        }

        self.is_dual_feasible = true;
        Ok(StopReason::Finished)
    }

    /// Dual simplex loop: used when the current solution is infeasible
    /// (some basic variable violates its bounds). Each iteration picks the
    /// most infeasible row (choose_pivot_row_dual), finds a compatible
    /// entering column (choose_entering_col_dual), and pivots to reduce
    /// infeasibility until all bounds are satisfied.
    fn restore_feasibility(&mut self) -> Result<StopReason, Error> {
        for iter in 0.. {
            if iter % 1000 == 0 {
                if check_deadline(&self.deadline) == StopReason::Limit {
                    return Ok(StopReason::Limit);
                }
            }

            if let Some((row, leaving_new_val)) = self.choose_pivot_row_dual() {
                self.calc_row_coeffs(row);
                let pivot_info = self.choose_entering_col_dual(row, leaving_new_val)?;
                self.calc_col_coeffs(pivot_info.col);
                self.pivot(&pivot_info)?;
            } else {
                break;
            }
        }

        self.is_primal_feasible = true;
        Ok(StopReason::Finished)
    }

    /// Add a new constraint to an already-solved LP and re-solve.
    /// The new constraint gets a slack variable and enters the basis.
    /// If adding it makes the solution infeasible, dual simplex restores feasibility.
    pub(crate) fn add_constraint(
        &mut self,
        mut coeffs: CsVec,
        cmp_op: ComparisonOp,
        rhs: f64,
    ) -> Result<StopReason, Error> {
        assert!(self.is_primal_feasible);
        assert!(self.is_dual_feasible);

        if coeffs.indices().is_empty() {
            let is_tautological = match cmp_op {
                ComparisonOp::Eq => float_eq(rhs, 0.0),
                ComparisonOp::Le => 0.0 <= rhs,
                ComparisonOp::Ge => 0.0 >= rhs,
            };

            return if is_tautological {
                Ok(StopReason::Finished)
            } else {
                Err(Error::Infeasible)
            };
        }

        let slack_var = self.num_total_vars();
        let (slack_var_min, slack_var_max) = match cmp_op {
            ComparisonOp::Le => (0.0, f64::INFINITY),
            ComparisonOp::Ge => (f64::NEG_INFINITY, 0.0),
            ComparisonOp::Eq => (0.0, 0.0),
        };

        self.orig_obj_coeffs.push(0.0);
        self.orig_var_mins.push(slack_var_min);
        self.orig_var_maxs.push(slack_var_max);
        self.var_states.push(VarState::Basic(self.basic_vars.len()));
        self.basic_vars.push(slack_var);
        self.basic_var_mins.push(slack_var_min);
        self.basic_var_maxs.push(slack_var_max);

        let mut lhs_val = 0.0;
        for (var, &coeff) in coeffs.iter() {
            let val = match self.var_states[var] {
                VarState::Basic(idx) => self.basic_var_vals[idx],
                VarState::NonBasic(idx) => self.nb_var_vals[idx],
            };
            lhs_val += val * coeff;
        }
        self.basic_var_vals.push(rhs - lhs_val);

        let new_num_total_vars = self.num_total_vars() + 1;
        let mut new_orig_constraints = CsMat::empty(CompressedStorage::CSR, new_num_total_vars);
        for row in self.orig_constraints.outer_iterator() {
            new_orig_constraints =
                new_orig_constraints.append_outer_csvec(resized_view(&row, new_num_total_vars));
        }
        coeffs = into_resized(coeffs, new_num_total_vars);
        coeffs.append(slack_var, 1.0);
        new_orig_constraints = new_orig_constraints.append_outer_csvec(coeffs.view());

        self.orig_rhs.push(rhs);

        self.orig_constraints = new_orig_constraints;
        self.orig_constraints_csc = self.orig_constraints.to_csc();

        self.basis_solver
            .reset(&self.orig_constraints_csc, &self.basic_vars)?;

        if self.enable_primal_steepest_edge || self.enable_dual_steepest_edge {
            self.calc_row_coeffs(self.num_constraints() - 1);

            if self.enable_primal_steepest_edge {
                for (c, &coeff) in self.row_coeffs.iter() {
                    self.primal_edge_sq_norms[c] += coeff * coeff;
                }
            }

            if self.enable_dual_steepest_edge {
                self.dual_edge_sq_norms
                    .push(self.inv_basis_row_coeffs.sq_norm());
            }
        }

        self.is_primal_feasible = false;
        self.restore_feasibility()
    }

    /// Number of infeasible basic vars and sum of their infeasibilities.
    #[allow(dead_code)]
    fn calc_primal_infeasibility(&self) -> (usize, f64) {
        let mut num_vars = 0;
        let mut infeasibility = 0.0;
        for ((&val, &min), &max) in self
            .basic_var_vals
            .iter()
            .zip(&self.basic_var_mins)
            .zip(&self.basic_var_maxs)
        {
            if val < min - EPS {
                num_vars += 1;
                infeasibility += min - val;
            } else if val > max + EPS {
                num_vars += 1;
                infeasibility += val - max;
            }
        }
        (num_vars, infeasibility)
    }

    /// Number of infeasible obj. coeffs and sum of their infeasibilities.
    #[allow(dead_code)]
    fn calc_dual_infeasibility(&self) -> (usize, f64) {
        let mut num_vars = 0;
        let mut infeasibility = 0.0;
        for (&obj_coeff, var_state) in self.nb_var_obj_coeffs.iter().zip(&self.nb_var_states) {
            if !(var_state.at_min && obj_coeff > -EPS || var_state.at_max && obj_coeff < EPS) {
                num_vars += 1;
                infeasibility += obj_coeff.abs();
            }
        }
        (num_vars, infeasibility)
    }

    /// Calculate current coeffs column for a single non-basic variable.
    fn calc_col_coeffs(&mut self, c_var: usize) {
        let var = self.nb_vars[c_var];
        let orig_col = self.orig_constraints_csc.outer_view(var).unwrap();
        self.basis_solver
            .solve(orig_col.iter())
            .to_sparse_vec(&mut self.col_coeffs);
    }

    /// Calculate current coeffs row for a single constraint (permuted according to nb_vars).
    fn calc_row_coeffs(&mut self, r_constr: usize) {
        self.basis_solver
            .solve_transp(std::iter::once((r_constr, &1.0)))
            .to_sparse_vec(&mut self.inv_basis_row_coeffs);

        self.row_coeffs.clear_and_resize(self.nb_vars.len());
        for (r, &coeff) in self.inv_basis_row_coeffs.iter() {
            for (v, &val) in self.orig_constraints.outer_view(r).unwrap().iter() {
                if let VarState::NonBasic(idx) = self.var_states[v] {
                    *self.row_coeffs.get_mut(idx) += val * coeff;
                }
            }
        }
    }

    /// Primal simplex pivot selection.
    ///
    /// 1. **Pick the entering variable**: the non-basic variable with the
    ///    best reduced cost (using steepest-edge pricing if enabled).
    /// 2. **Compute the pivot column**: solve `B * col = a_j` to get the
    ///    representation of the entering column in the current basis.
    /// 3. **Pick the leaving variable** (ratio test with Harris rule):
    ///    find the basic variable that hits its bound first as the entering
    ///    variable increases. The Harris rule adds a small tolerance to
    ///    avoid degenerate pivots.
    ///
    /// Returns `None` when all reduced costs are non-improving → optimal.
    fn choose_pivot(&mut self) -> Result<Option<PivotInfo>, Error> {
        let entering_c = {
            let filtered_obj_coeffs = self
                .nb_var_obj_coeffs
                .iter()
                .zip(&self.nb_var_states)
                .enumerate()
                .filter_map(|(col, (&obj_coeff, var_state))| {
                    if (var_state.at_min && obj_coeff > -EPS)
                        || (var_state.at_max && obj_coeff < EPS)
                    {
                        None
                    } else {
                        Some((col, obj_coeff))
                    }
                });

            let mut best_col = None;
            let mut best_score = f64::NEG_INFINITY;
            if self.enable_primal_steepest_edge {
                for (col, obj_coeff) in filtered_obj_coeffs {
                    let score = obj_coeff * obj_coeff / self.primal_edge_sq_norms[col];
                    if score > best_score {
                        best_col = Some(col);
                        best_score = score;
                    }
                }
            } else {
                for (col, obj_coeff) in filtered_obj_coeffs {
                    let score = obj_coeff.abs();
                    if score > best_score {
                        best_col = Some(col);
                        best_score = score;
                    }
                }
            }

            if let Some(col) = best_col {
                col
            } else {
                return Ok(None);
            }
        };

        let entering_cur_val = self.nb_var_vals[entering_c];
        let entering_diff_sign = self.nb_var_obj_coeffs[entering_c] < 0.0;
        let entering_other_val = if entering_diff_sign {
            self.orig_var_maxs[self.nb_vars[entering_c]]
        } else {
            self.orig_var_mins[self.nb_vars[entering_c]]
        };

        self.calc_col_coeffs(entering_c);

        let get_leaving_var_step = |r: usize, coeff: f64| -> f64 {
            let val = self.basic_var_vals[r];
            if (entering_diff_sign && coeff < 0.0) || (!entering_diff_sign && coeff > 0.0) {
                let max = self.basic_var_maxs[r];
                if val < max { max - val } else { 0.0 }
            } else {
                let min = self.basic_var_mins[r];
                if val > min { val - min } else { 0.0 }
            }
        };

        // Harris rule
        let mut max_step = (entering_other_val - entering_cur_val).abs();
        for (r, &coeff) in self.col_coeffs.iter() {
            let coeff_abs = coeff.abs();
            if coeff_abs < EPS {
                continue;
            }

            let cur_step = (get_leaving_var_step(r, coeff) + EPS) / coeff_abs;
            if cur_step < max_step {
                max_step = cur_step;
            }
        }

        let mut leaving_r = None;
        let mut leaving_new_val = 0.0;
        let mut pivot_coeff_abs = f64::NEG_INFINITY;
        let mut pivot_coeff = 0.0;
        for (r, &coeff) in self.col_coeffs.iter() {
            let coeff_abs = coeff.abs();
            if coeff_abs < EPS {
                continue;
            }

            let cur_step = get_leaving_var_step(r, coeff) / coeff_abs;
            if cur_step <= max_step && coeff_abs > pivot_coeff_abs {
                leaving_r = Some(r);
                leaving_new_val = if (entering_diff_sign && coeff < 0.0)
                    || (!entering_diff_sign && coeff > 0.0)
                {
                    self.basic_var_maxs[r]
                } else {
                    self.basic_var_mins[r]
                };
                pivot_coeff = coeff;
                pivot_coeff_abs = coeff_abs;
            }
        }

        if let Some(row) = leaving_r {
            self.calc_row_coeffs(row);

            let entering_diff = (self.basic_var_vals[row] - leaving_new_val) / pivot_coeff;
            let entering_new_val = entering_cur_val + entering_diff;

            Ok(Some(PivotInfo {
                col: entering_c,
                entering_new_val,
                entering_diff,
                elem: Some(PivotElem {
                    row,
                    coeff: pivot_coeff,
                    leaving_new_val,
                }),
            }))
        } else {
            if entering_other_val.is_infinite() {
                return Err(Error::Unbounded);
            }

            Ok(Some(PivotInfo {
                col: entering_c,
                entering_new_val: entering_other_val,
                entering_diff: entering_other_val - entering_cur_val,
                elem: None,
            }))
        }
    }

    /// Dual simplex: pick the leaving row — the basic variable with the
    /// worst bound violation (weighted by steepest-edge norm if enabled).
    fn choose_pivot_row_dual(&self) -> Option<(usize, f64)> {
        let infeasibilities = self
            .basic_var_vals
            .iter()
            .zip(&self.basic_var_mins)
            .zip(&self.basic_var_maxs)
            .enumerate()
            .filter_map(|(r, ((&val, &min), &max))| {
                if val < min - EPS {
                    Some((r, min - val))
                } else if val > max + EPS {
                    Some((r, val - max))
                } else {
                    None
                }
            });

        let mut leaving_r = None;
        let mut max_score = f64::NEG_INFINITY;
        if self.enable_dual_steepest_edge {
            for (r, infeasibility) in infeasibilities {
                let sq_norm = self.dual_edge_sq_norms[r];
                let score = infeasibility * infeasibility / sq_norm;
                if score > max_score {
                    leaving_r = Some(r);
                    max_score = score;
                }
            }
        } else {
            for (r, infeasibility) in infeasibilities {
                if infeasibility > max_score {
                    leaving_r = Some(r);
                    max_score = infeasibility;
                }
            }
        }

        leaving_r.map(|r| {
            let val = self.basic_var_vals[r];
            let min = self.basic_var_mins[r];
            let max = self.basic_var_maxs[r];

            let new_val = if val < min {
                min
            } else if val > max {
                max
            } else {
                unreachable!();
            };
            (r, new_val)
        })
    }

    /// Dual simplex: given the leaving row, pick the entering column.
    /// Uses the dual ratio test (with Harris rule) to find the non-basic
    /// variable that maintains dual feasibility while allowing the leaving
    /// variable to reach its bound.
    fn choose_entering_col_dual(
        &self,
        row: usize,
        leaving_new_val: f64,
    ) -> Result<PivotInfo, Error> {
        let leaving_diff_sign = leaving_new_val > self.basic_var_vals[row];

        fn clamp_obj_coeff(mut obj_coeff: f64, var_state: &NonBasicVarState) -> f64 {
            if var_state.at_min && obj_coeff < 0.0 {
                obj_coeff = 0.0;
            }
            if var_state.at_max && obj_coeff > 0.0 {
                obj_coeff = 0.0;
            }
            obj_coeff
        }

        let is_eligible_var = |coeff: f64, var_state: &NonBasicVarState| -> bool {
            let entering_diff_sign = if coeff >= EPS {
                !leaving_diff_sign
            } else if coeff <= -EPS {
                leaving_diff_sign
            } else {
                return false;
            };

            if entering_diff_sign {
                !var_state.at_max
            } else {
                !var_state.at_min
            }
        };

        // Harris rule
        let mut max_step = f64::INFINITY;
        for (c, &coeff) in self.row_coeffs.iter() {
            let var_state = &self.nb_var_states[c];
            if !is_eligible_var(coeff, var_state) {
                continue;
            }

            let obj_coeff = clamp_obj_coeff(self.nb_var_obj_coeffs[c], var_state);
            let cur_step = (obj_coeff.abs() + EPS) / coeff.abs();
            if cur_step < max_step {
                max_step = cur_step;
            }
        }

        let mut entering_c = None;
        let mut pivot_coeff_abs = f64::NEG_INFINITY;
        let mut pivot_coeff = 0.0;
        for (c, &coeff) in self.row_coeffs.iter() {
            let var_state = &self.nb_var_states[c];
            if !is_eligible_var(coeff, var_state) {
                continue;
            }

            let obj_coeff = clamp_obj_coeff(self.nb_var_obj_coeffs[c], var_state);

            let cur_step = obj_coeff.abs() / coeff.abs();
            if cur_step <= max_step {
                let coeff_abs = coeff.abs();
                if coeff_abs > pivot_coeff_abs {
                    entering_c = Some(c);
                    pivot_coeff_abs = coeff_abs;
                    pivot_coeff = coeff;
                }
            }
        }

        if let Some(col) = entering_c {
            let entering_diff = (self.basic_var_vals[row] - leaving_new_val) / pivot_coeff;
            let entering_new_val = self.nb_var_vals[col] + entering_diff;

            Ok(PivotInfo {
                col,
                entering_new_val,
                entering_diff,
                elem: Some(PivotElem {
                    row,
                    leaving_new_val,
                    coeff: pivot_coeff,
                }),
            })
        } else {
            Err(Error::Infeasible)
        }
    }

    /// Execute a basis exchange: swap the entering variable into the basis
    /// and the leaving variable out. This updates all solver state:
    /// - basic/non-basic variable values and bounds
    /// - reduced costs (objective coefficients)
    /// - steepest-edge norms (if enabled)
    /// - the LU factorisation (either by appending an eta matrix or
    ///   refactoring from scratch when eta fill-in gets too large)
    fn pivot(&mut self, pivot_info: &PivotInfo) -> Result<(), Error> {
        self.cur_obj_val += self.nb_var_obj_coeffs[pivot_info.col] * pivot_info.entering_diff;

        let entering_var = self.nb_vars[pivot_info.col];

        if pivot_info.elem.is_none() {
            self.nb_var_vals[pivot_info.col] = pivot_info.entering_new_val;
            for (r, coeff) in self.col_coeffs.iter() {
                self.basic_var_vals[r] -= pivot_info.entering_diff * coeff;
            }
            let var_state = &mut self.nb_var_states[pivot_info.col];
            var_state.at_min = float_eq(
                pivot_info.entering_new_val,
                self.orig_var_mins[entering_var],
            );
            var_state.at_max = float_eq(
                pivot_info.entering_new_val,
                self.orig_var_maxs[entering_var],
            );
            return Ok(());
        }
        let pivot_elem = pivot_info.elem.as_ref().unwrap();
        let pivot_coeff = pivot_elem.coeff;

        // Update basic vars stuff
        for (r, coeff) in self.col_coeffs.iter() {
            if r == pivot_elem.row {
                self.basic_var_vals[r] = pivot_info.entering_new_val;
            } else {
                self.basic_var_vals[r] -= pivot_info.entering_diff * coeff;
            }
        }

        self.basic_var_mins[pivot_elem.row] = self.orig_var_mins[entering_var];
        self.basic_var_maxs[pivot_elem.row] = self.orig_var_maxs[entering_var];

        if self.enable_dual_steepest_edge {
            self.update_dual_sq_norms(pivot_elem.row, pivot_coeff);
        }

        // Update non-basic vars stuff
        let leaving_var = self.basic_vars[pivot_elem.row];

        self.nb_var_vals[pivot_info.col] = pivot_elem.leaving_new_val;
        let leaving_var_state = &mut self.nb_var_states[pivot_info.col];
        leaving_var_state.at_min =
            float_eq(pivot_elem.leaving_new_val, self.orig_var_mins[leaving_var]);
        leaving_var_state.at_max =
            float_eq(pivot_elem.leaving_new_val, self.orig_var_maxs[leaving_var]);

        let pivot_obj = self.nb_var_obj_coeffs[pivot_info.col] / pivot_coeff;
        for (c, &coeff) in self.row_coeffs.iter() {
            if c == pivot_info.col {
                self.nb_var_obj_coeffs[c] = -pivot_obj;
            } else {
                self.nb_var_obj_coeffs[c] -= pivot_obj * coeff;
            }
        }

        if self.enable_primal_steepest_edge {
            self.update_primal_sq_norms(pivot_info.col, pivot_coeff);
        }

        // Update basis itself
        self.basic_vars[pivot_elem.row] = entering_var;
        self.var_states[entering_var] = VarState::Basic(pivot_elem.row);
        self.nb_vars[pivot_info.col] = leaving_var;
        self.var_states[leaving_var] = VarState::NonBasic(pivot_info.col);

        let eta_matrices_nnz = self.basis_solver.eta_matrices.coeff_cols.nnz();
        if eta_matrices_nnz < self.basis_solver.lu_factors.nnz() {
            self.basis_solver
                .push_eta_matrix(&self.col_coeffs, pivot_elem.row, pivot_coeff);
        } else {
            self.basis_solver
                .reset(&self.orig_constraints_csc, &self.basic_vars)?;
        }
        Ok(())
    }

    /// Incrementally update the squared steepest-edge norms for primal pricing
    /// after a pivot. This avoids recomputing them from scratch each iteration.
    fn update_primal_sq_norms(&mut self, entering_col: usize, pivot_coeff: f64) {
        let tmp = self.basis_solver.solve_transp(self.col_coeffs.iter());

        for &r in tmp.indices() {
            for &v in self.orig_constraints.outer_view(r).unwrap().indices() {
                if let VarState::NonBasic(idx) = self.var_states[v] {
                    self.sq_norms_update_helper[idx] = 0.0;
                }
            }
        }

        for (r, &coeff) in tmp.iter() {
            for (v, &val) in self.orig_constraints.outer_view(r).unwrap().iter() {
                if let VarState::NonBasic(idx) = self.var_states[v] {
                    self.sq_norms_update_helper[idx] += val * coeff;
                }
            }
        }

        let pivot_sq_norm = self.col_coeffs.sq_norm() + 1.0;

        let pivot_coeff_sq = pivot_coeff * pivot_coeff;
        for (c, &r_coeff) in self.row_coeffs.iter() {
            if c == entering_col {
                self.primal_edge_sq_norms[c] = pivot_sq_norm / pivot_coeff_sq;
            } else {
                self.primal_edge_sq_norms[c] += -2.0 * r_coeff * self.sq_norms_update_helper[c]
                    / pivot_coeff
                    + pivot_sq_norm * r_coeff * r_coeff / pivot_coeff_sq;
            }

            assert!(self.primal_edge_sq_norms[c].is_finite());
        }
    }

    /// Incrementally update the squared steepest-edge norms for dual pricing.
    fn update_dual_sq_norms(&mut self, leaving_row: usize, pivot_coeff: f64) {
        let tau = self.basis_solver.solve(self.inv_basis_row_coeffs.iter());

        let pivot_sq_norm = self.inv_basis_row_coeffs.sq_norm();

        let pivot_coeff_sq = pivot_coeff * pivot_coeff;
        for (r, &col_coeff) in self.col_coeffs.iter() {
            if r == leaving_row {
                self.dual_edge_sq_norms[r] = pivot_sq_norm / pivot_coeff_sq;
            } else {
                self.dual_edge_sq_norms[r] += -2.0 * col_coeff * tau.get(r) / pivot_coeff
                    + pivot_sq_norm * col_coeff * col_coeff / pivot_coeff_sq;
            }

            assert!(self.dual_edge_sq_norms[r].is_finite());
        }
    }

    #[allow(dead_code)]
    fn recalc_basic_var_vals(&mut self) -> Result<(), Error> {
        let mut cur_vals = self.orig_rhs.clone();
        for (i, var) in self.nb_vars.iter().enumerate() {
            let val = self.nb_var_vals[i];
            if val != 0.0 {
                for (r, &coeff) in self.orig_constraints_csc.outer_view(*var).unwrap().iter() {
                    cur_vals[r] -= val * coeff;
                }
            }
        }

        if self.basis_solver.eta_matrices.len() > 0 {
            self.basis_solver
                .reset(&self.orig_constraints_csc, &self.basic_vars)?;
        }

        self.basis_solver
            .lu_factors
            .solve_dense(&mut cur_vals, &mut self.basis_solver.scratch);
        self.basic_var_vals = cur_vals;
        Ok(())
    }

    /// Recompute the reduced costs of all non-basic variables from scratch.
    /// Called when switching from the artificial objective (phase I) to
    /// the real objective (phase II), or when accumulated rounding makes
    /// incremental updates unreliable.
    fn recalc_obj_coeffs(&mut self) -> Result<(), Error> {
        if self.basis_solver.eta_matrices.len() > 0 {
            self.basis_solver
                .reset(&self.orig_constraints_csc, &self.basic_vars)?;
        }

        let multipliers = {
            let mut rhs = vec![0.0; self.num_constraints()];
            for (c, &var) in self.basic_vars.iter().enumerate() {
                rhs[c] = self.orig_obj_coeffs[var];
            }
            self.basis_solver
                .lu_factors_transp
                .solve_dense(&mut rhs, &mut self.basis_solver.scratch);
            rhs
        };

        self.nb_var_obj_coeffs.clear();
        for &var in &self.nb_vars {
            let col = self.orig_constraints_csc.outer_view(var).unwrap();
            let dot_prod: f64 = col.iter().map(|(r, val)| val * multipliers[r]).sum();
            self.nb_var_obj_coeffs
                .push(self.orig_obj_coeffs[var] - dot_prod);
        }

        self.cur_obj_val = 0.0;
        for (r, &var) in self.basic_vars.iter().enumerate() {
            self.cur_obj_val += self.orig_obj_coeffs[var] * self.basic_var_vals[r];
        }
        for (c, &var) in self.nb_vars.iter().enumerate() {
            self.cur_obj_val += self.orig_obj_coeffs[var] * self.nb_var_vals[c];
        }
        Ok(())
    }
}

/// Everything needed to perform a single pivot operation.
#[derive(Debug)]
struct PivotInfo {
    /// Non-basic index of the entering variable.
    col: usize,
    /// Value the entering variable will take after the pivot.
    entering_new_val: f64,
    /// Change in the entering variable's value (new - old).
    entering_diff: f64,
    /// `None` when the entering variable simply moves to its other bound
    /// without swapping with a basic variable (a "bound flip").
    elem: Option<PivotElem>,
}

/// Details of the basis exchange when a variable actually leaves the basis.
#[derive(Debug)]
struct PivotElem {
    /// Constraint row of the leaving basic variable.
    row: usize,
    /// The pivot element: coefficient of the entering variable in this row.
    coeff: f64,
    /// Value the leaving variable will sit at after leaving the basis.
    leaving_new_val: f64,
}

/// Maintains the LU factorisation of the current basis matrix `B` and
/// provides `solve(rhs)` → `B^{-1} * rhs` and `solve_transp(rhs)` → `B^{-T} * rhs`.
///
/// After each pivot, instead of refactoring from scratch, we append an
/// "eta matrix" — a rank-1 correction that captures the basis change.
/// When the accumulated eta matrices get too large (more non-zeros than
/// the LU factors themselves), we refactor from scratch.
#[derive(Clone)]
struct BasisSolver {
    lu_factors: LUFactors,
    /// Transposed LU factors, for solving `B^T * y = c` (needed for
    /// reduced cost computation and steepest-edge updates).
    lu_factors_transp: LUFactors,
    scratch: ScratchSpace,
    /// Accumulated rank-1 corrections since the last full refactorisation.
    eta_matrices: EtaMatrices,
    /// Reusable working buffer for solve operations.
    rhs: ScatteredVec,
}

impl BasisSolver {
    /// Record a rank-1 basis update instead of refactoring the full LU.
    fn push_eta_matrix(&mut self, col_coeffs: &SparseVec, r_leaving: usize, pivot_coeff: f64) {
        let coeffs = col_coeffs.iter().map(|(r, &coeff)| {
            let val = if r == r_leaving {
                1.0 - 1.0 / pivot_coeff
            } else {
                coeff / pivot_coeff
            };
            (r, val)
        });
        self.eta_matrices.push(r_leaving, coeffs);
    }

    /// Full LU refactorisation of the current basis matrix from scratch.
    /// Called when eta fill-in exceeds the base LU size.
    fn reset(&mut self, orig_constraints_csc: &CsMat, basic_vars: &[usize]) -> Result<(), Error> {
        self.scratch.clear_sparse(basic_vars.len());
        self.eta_matrices.clear_and_resize(basic_vars.len());
        self.rhs.clear_and_resize(basic_vars.len());
        self.lu_factors = lu_factorize(
            basic_vars.len(),
            |c| {
                orig_constraints_csc
                    .outer_view(basic_vars[c])
                    .unwrap()
                    .into_raw_storage()
            },
            0.1,
            &mut self.scratch,
        )
        .map_err(|e| Error::InternalError(e.to_string()))?;
        self.lu_factors_transp = self.lu_factors.transpose();
        Ok(())
    }

    /// Solve `B * x = rhs` where B is the current basis matrix.
    /// First applies the base LU, then applies each accumulated eta correction.
    fn solve<'a>(&mut self, rhs: impl Iterator<Item = (usize, &'a f64)>) -> &ScatteredVec {
        self.rhs.set(rhs);
        self.lu_factors.solve(&mut self.rhs, &mut self.scratch);

        for idx in 0..self.eta_matrices.len() {
            let r_leaving = self.eta_matrices.leaving_rows[idx];
            let coeff = *self.rhs.get(r_leaving);
            for (r, &val) in self.eta_matrices.coeff_cols.col_iter(idx) {
                *self.rhs.get_mut(r) -= coeff * val;
            }
        }

        &mut self.rhs
    }

    /// Solve `B^T * y = rhs` (the transpose system).
    /// Eta corrections are applied in reverse order, then the transposed LU solve runs.
    fn solve_transp<'a>(&mut self, rhs: impl Iterator<Item = (usize, &'a f64)>) -> &ScatteredVec {
        self.rhs.set(rhs);
        for idx in (0..self.eta_matrices.len()).rev() {
            let mut coeff = 0.0;
            for (i, &val) in self.eta_matrices.coeff_cols.col_iter(idx) {
                coeff += val * self.rhs.get(i);
            }
            let r_leaving = self.eta_matrices.leaving_rows[idx];
            *self.rhs.get_mut(r_leaving) -= coeff;
        }

        self.lu_factors_transp
            .solve(&mut self.rhs, &mut self.scratch);
        &mut self.rhs
    }
}

/// A sequence of eta (rank-1 update) matrices that accumulate between full
/// LU refactorisations. Each eta matrix records one basis change: which row
/// left and what coefficients changed. Applying them in order after the base
/// LU solve gives the correct result for the current basis.
#[derive(Clone, Debug)]
struct EtaMatrices {
    /// Which row left the basis in each successive pivot.
    leaving_rows: Vec<usize>,
    /// The update coefficients for each pivot, stored column-wise.
    coeff_cols: SparseMat,
}

impl EtaMatrices {
    fn new(n_rows: usize) -> EtaMatrices {
        EtaMatrices {
            leaving_rows: vec![],
            coeff_cols: SparseMat::new(n_rows),
        }
    }

    fn len(&self) -> usize {
        self.leaving_rows.len()
    }

    fn clear_and_resize(&mut self, n_rows: usize) {
        self.leaving_rows.clear();
        self.coeff_cols.clear_and_resize(n_rows);
    }

    fn push(&mut self, leaving_row: usize, coeffs: impl Iterator<Item = (usize, f64)>) {
        self.leaving_rows.push(leaving_row);
        self.coeff_cols.append_col(coeffs);
    }
}

/// Consume a sparse vector and return one truncated/extended to `len` dimensions.
/// Entries with index >= len are dropped.
fn into_resized(vec: CsVec, len: usize) -> CsVec {
    let (mut indices, mut data) = vec.into_raw_storage();

    while let Some(&i) = indices.last() {
        if i < len {
            break;
        }

        indices.pop();
        data.pop();
    }

    CsVec::new(len, indices, data)
}
