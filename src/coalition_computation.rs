use crate::types::{LPPrimitives, Link, Result, ShapleyValue, f64_to_decimal, round_decimal};
use clarabel::algebra::*;
use clarabel::solver::{DefaultSettingsBuilder, DefaultSolver, IPSolver, SolverStatus};
use faer::{
    Col, Mat, Par, Unbind,
    sparse::{SparseColMat, Triplet},
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::HashSet;

/// Enumerate all unique operators from private links
pub fn enumerate_operators(private_links: &[Link]) -> Vec<String> {
    let mut operators = HashSet::new();

    for link in private_links {
        if link.operator1 != "0" {
            operators.insert(link.operator1.clone());
        }
        if link.operator2 != "0" {
            operators.insert(link.operator2.clone());
        }
    }

    let mut sorted_operators: Vec<String> = operators.into_iter().collect();
    sorted_operators.sort();
    sorted_operators
}

/// Generate bitmap for all possible coalitions
pub fn generate_coalition_bitmap(n_operators: usize) -> Mat<u8> {
    let n_coalitions = 1 << n_operators; // 2^n_operators
    let mut bitmap = Mat::from_fn(n_operators, n_coalitions, |_, _| 0u8);

    for col in 0..n_coalitions {
        for row in 0..n_operators {
            if (col >> row) & 1 == 1 {
                bitmap[(row, col)] = 1;
            }
        }
    }

    bitmap
}

/// Solve linear program for each coalition to get optimal values
pub fn solve_coalition_values(
    operators: &[String],
    bitmap: &Mat<u8>,
    primitives: &LPPrimitives,
) -> Result<(Col<f64>, Col<usize>)> {
    let n_coalitions = bitmap.ncols();
    let mut svalue = Col::full(n_coalitions, f64::NEG_INFINITY);
    let mut size = Col::from_fn(n_coalitions, |_| 0usize);

    // For very large problems (10+ operators), use sampling
    if operators.len() >= 10 {
        solve_coalition_values_sampled(operators, bitmap, primitives, &mut svalue, &mut size)?;
    } else {
        // Determine if we should use parallel processing
        let use_parallel = operators.len() > 2;

        if use_parallel {
            // Parallel version
            let results: Vec<(f64, usize)> = (0..n_coalitions)
                .into_par_iter()
                .map(|idx| {
                    let subset = get_coalition_subset(operators, bitmap, idx);
                    let coalition_size = subset.len();

                    let (row_mask, col_mask) = get_coalition_masks(&subset, primitives);
                    let value = solve_single_coalition(primitives, &row_mask, &col_mask)
                        .unwrap_or(f64::NEG_INFINITY);

                    (value, coalition_size)
                })
                .collect();

            for (idx, (value, coalition_size)) in results.into_iter().enumerate() {
                svalue[idx] = value;
                size[idx] = coalition_size;
            }
        } else {
            // Sequential version
            for idx in 0..n_coalitions {
                let subset = get_coalition_subset(operators, bitmap, idx);
                size[idx] = subset.len();

                let (row_mask, col_mask) = get_coalition_masks(&subset, primitives);

                if let Some(value) = solve_single_coalition(primitives, &row_mask, &col_mask) {
                    svalue[idx] = value;
                }
            }
        }
    }

    Ok((svalue, size))
}

/// Sampling-based approach for very large coalition counts (10+ operators)
fn solve_coalition_values_sampled(
    operators: &[String],
    bitmap: &Mat<u8>,
    primitives: &LPPrimitives,
    svalue: &mut Col<f64>,
    size: &mut Col<usize>,
) -> Result<()> {
    let n_ops = operators.len();
    let n_coalitions = bitmap.ncols();

    // Always compute empty and full coalitions
    svalue[0] = 0.0;
    size[0] = 0;

    let full_idx = n_coalitions - 1;
    let ops_refs: Vec<&str> = operators.iter().map(|s| s.as_str()).collect();
    let (row_mask, col_mask) = get_coalition_masks(&ops_refs, primitives);
    svalue[full_idx] =
        solve_single_coalition(primitives, &row_mask, &col_mask).unwrap_or(f64::NEG_INFINITY);
    size[full_idx] = n_ops;

    // Stratified sampling: ensure we sample from each coalition size
    let samples_per_size = match n_ops {
        10..=12 => 50,
        13..=15 => 30,
        _ => 20,
    };

    // Group coalitions by size
    let mut coalitions_by_size: Vec<Vec<usize>> = vec![vec![]; n_ops + 1];
    for idx in 0..n_coalitions {
        let coalition_size = (0..n_ops).filter(|&i| bitmap[(i, idx)] == 1).count();
        coalitions_by_size[coalition_size].push(idx);
    }

    // Sample from each size (except empty and full which we already computed)
    let sampled_results: Vec<(usize, f64, usize)> = (1..n_ops)
        .into_par_iter()
        .flat_map(|coalition_size| {
            let mut local_rng = StdRng::seed_from_u64(42 + coalition_size as u64);
            let coalitions = &coalitions_by_size[coalition_size];
            let n_samples = samples_per_size.min(coalitions.len());

            // Sample without replacement
            let mut indices: Vec<usize> = coalitions.to_vec();
            indices.sort_unstable();

            (0..n_samples)
                .map(move |_| {
                    let idx = local_rng.gen_range(0..indices.len());
                    let coalition_idx = indices.swap_remove(idx);

                    let subset = get_coalition_subset(operators, bitmap, coalition_idx);
                    let (row_mask, col_mask) = get_coalition_masks(&subset, primitives);
                    let value = solve_single_coalition(primitives, &row_mask, &col_mask)
                        .unwrap_or(f64::NEG_INFINITY);

                    (coalition_idx, value, coalition_size)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Fill in sampled values
    for (idx, value, coalition_size) in sampled_results {
        svalue[idx] = value;
        size[idx] = coalition_size;
    }

    // Interpolate remaining values using nearest neighbors of same size
    for (coalition_size, coalitions) in coalitions_by_size
        .iter()
        .enumerate()
        .skip(1)
        .take(n_ops - 1)
    {
        let known_values: Vec<(usize, f64)> = coalitions
            .iter()
            .filter_map(|&idx| {
                if svalue[idx] > f64::NEG_INFINITY {
                    Some((idx, svalue[idx]))
                } else {
                    None
                }
            })
            .collect();

        if known_values.is_empty() {
            continue;
        }

        // For unsampled coalitions, use average of sampled ones of same size
        let avg_value =
            known_values.iter().map(|(_, v)| v).sum::<f64>() / known_values.len() as f64;

        for &idx in coalitions {
            if svalue[idx] == f64::NEG_INFINITY {
                svalue[idx] = avg_value;
                size[idx] = coalition_size;
            }
        }
    }

    Ok(())
}

/// Compute expected values accounting for operator downtime
pub fn compute_expected_values(
    svalue: &Col<f64>,
    size: &Col<usize>,
    operator_uptime: f64,
    n_ops: usize,
) -> Result<Col<f64>> {
    // Returns owned Col<f64>
    faer::set_global_parallelism(Par::rayon(0));

    let n_coal = svalue.nrows();
    let bitmap = generate_coalition_bitmap(n_ops);
    let submask: Mat<f64> = build_submask(&bitmap, n_coal);

    let base_p: Col<f64> = Col::from_fn(size.nrows(), |i| operator_uptime.powi(size[i] as i32));

    // bp_masked[i,j] = base_p[j] * submask[i,j]
    // Using loop for clarity, faer's broadcasting for this specific pattern can be less direct
    let mut bp_masked = Mat::zeros(n_coal, n_coal);
    for r in 0..n_coal {
        for c in 0..n_coal {
            bp_masked[(r, c)] = base_p[c] * submask[(r, c)];
        }
    }

    // Build sparse matrix
    let coef_sparse = build_coefficient_matrix(n_ops)?;
    // Then dense
    let coef_mat: Mat<f64> = coef_sparse.to_dense();

    // coef_times_submask = coef_mat .* submask (element-wise)
    let coef_times_submask =
        Mat::from_fn(n_coal, n_coal, |r, c| coef_mat[(r, c)] * submask[(r, c)]);

    // term = bp_masked * coef_times_submask (matrix multiplication)
    let term: Mat<f64> = bp_masked.as_ref() * coef_times_submask.as_ref();

    // part = (bp_masked + term) .* submask (element-wise)
    let part = Mat::from_fn(n_coal, n_coal, |r, c| {
        (bp_masked[(r, c)] + term[(r, c)]) * submask[(r, c)]
    });

    let mut evalue: Col<f64> = part.as_ref() * svalue.as_ref();

    if n_coal > 0 {
        evalue[0] = svalue[0];
    }

    Ok(evalue)
}

/// Calculate Shapley values for each operator
pub fn calculate_shapley_values(
    operators: &[String],
    evalue: &Col<f64>,
    size: &Col<usize>,
    n_ops: usize,
) -> Result<Vec<ShapleyValue>> {
    let bitmap = generate_coalition_bitmap(n_ops);
    let mut shapley = Col::zeros(n_ops);

    // Pre-compute factorials up to n_ops
    let factorials: Vec<f64> = (0..=n_ops).map(|i| factorial(i) as f64).collect();
    let fact_n = factorials[n_ops];

    for (k, _op) in operators.iter().enumerate() {
        // Find coalitions with/without operator
        let with_op: Vec<usize> = (0..bitmap.ncols())
            .filter(|&i| bitmap[(k, i)] == 1)
            .collect();

        let without_op: Vec<usize> = with_op.iter().map(|&i| i - (1 << k)).collect();

        // Calculate weights using pre-computed factorials
        let weights: Vec<f64> = with_op
            .iter()
            .map(|&i| {
                let s = size[i];
                factorials[s - 1] * factorials[n_ops - s] / fact_n
            })
            .collect();

        // Compute Shapley value
        let mut contribution = 0.0;
        for (idx, (&with_idx, &without_idx)) in with_op.iter().zip(&without_op).enumerate() {
            contribution += weights[idx] * (evalue[with_idx] - evalue[without_idx]);
        }
        shapley[k] = contribution;
    }

    // Convert to percentages
    let mut percent = Col::zeros(n_ops);
    for i in 0..n_ops {
        percent[i] = shapley[i].max(0.0);
    }
    let total: f64 = (0..n_ops).map(|i| percent[i]).sum();
    if total > 0.0 {
        for i in 0..n_ops {
            percent[i] /= total;
        }
    }

    // Build result
    let mut results = Vec::with_capacity(operators.len());
    for (i, op) in operators.iter().enumerate() {
        results.push(ShapleyValue {
            operator: op.clone(),
            value: round_decimal(f64_to_decimal(shapley[i])),
            percent: round_decimal(f64_to_decimal(percent[i])),
        });
    }

    Ok(results)
}

// Helper functions

#[inline]
fn get_coalition_subset<'a>(operators: &'a [String], bitmap: &Mat<u8>, idx: usize) -> Vec<&'a str> {
    operators
        .iter()
        .enumerate()
        .filter_map(|(i, op)| match bitmap[(i, idx)] == 1 {
            false => None,
            true => Some(op.as_str()),
        })
        .collect()
}

fn get_coalition_masks(subset: &[&str], primitives: &LPPrimitives) -> (Vec<bool>, Vec<bool>) {
    // Include "0" for public operator
    let mut valid_ops: HashSet<&str> = HashSet::with_capacity(subset.len() + 1);
    valid_ops.insert("0");
    for &op in subset {
        valid_ops.insert(op);
    }

    let row_mask: Vec<bool> = primitives
        .row_index1
        .iter()
        .zip(&primitives.row_index2)
        .map(|(op1, op2)| valid_ops.contains(op1.as_str()) && valid_ops.contains(op2.as_str()))
        .collect();

    let col_mask: Vec<bool> = primitives
        .col_index1
        .iter()
        .zip(&primitives.col_index2)
        .map(|(op1, op2)| valid_ops.contains(op1.as_str()) && valid_ops.contains(op2.as_str()))
        .collect();

    (row_mask, col_mask)
}

fn solve_single_coalition(
    primitives: &LPPrimitives,
    row_mask: &[bool],
    col_mask: &[bool],
) -> Option<f64> {
    // Filter matrices and vectors based on masks
    let selected_cols: Vec<usize> = col_mask
        .iter()
        .enumerate()
        .filter_map(|(i, m)| match m {
            false => None,
            true => Some(i),
        })
        .collect();

    let selected_rows: Vec<usize> = row_mask
        .iter()
        .enumerate()
        .filter_map(|(i, m)| match m {
            false => None,
            true => Some(i),
        })
        .collect();

    if selected_cols.is_empty() {
        return None;
    }

    let n_vars = selected_cols.len();

    // Build objective vector c (costs for selected columns)
    let c: Vec<f64> = selected_cols
        .iter()
        .map(|&old_col| primitives.cost[old_col])
        .collect();

    // Build constraint matrices for clarabel
    // First collect all constraints: equalities then inequalities
    // Improved memory allocation estimates based on actual sparsity
    let avg_nnz_per_col = if primitives.a_eq.ncols() > 0 {
        // Count non-zeros manually from triplets
        let nnz = primitives.a_eq.triplet_iter().count();
        nnz as f64 / primitives.a_eq.ncols() as f64
    } else {
        10.0
    };
    let estimated_nnz = (avg_nnz_per_col * selected_cols.len() as f64 * 1.5) as usize;
    let estimated_constraints = primitives.a_eq.nrows() + selected_rows.len() + n_vars;
    let mut all_constraints_triplets = Vec::with_capacity(estimated_nnz);
    let mut all_b = Vec::with_capacity(estimated_constraints);
    let mut cone_dims = Vec::with_capacity(3); // At most 3 cones
    let mut constraint_row = 0;

    // Add flow conservation constraints (equality)
    let mut a_eq_map = std::collections::HashMap::new();
    for triplet in primitives.a_eq.triplet_iter() {
        let row_idx = triplet.row.unbound();
        let col_idx = triplet.col.unbound();
        a_eq_map.insert((row_idx, col_idx), *triplet.val);
    }

    let n_eq_constraints = primitives.a_eq.nrows();
    for row in 0..n_eq_constraints {
        for (new_col, &old_col) in selected_cols.iter().enumerate() {
            if let Some(&coeff) = a_eq_map.get(&(row, old_col)) {
                if coeff != 0.0 {
                    all_constraints_triplets.push(Triplet::new(constraint_row, new_col, coeff));
                }
            }
        }
        all_b.push(primitives.b_eq[row]);
        constraint_row += 1;
    }

    if n_eq_constraints > 0 {
        cone_dims.push(clarabel::solver::ZeroConeT(n_eq_constraints));
    }

    // Add bandwidth constraints (inequality) if any
    let mut n_ineq_constraints = 0;
    if !selected_rows.is_empty() && primitives.b_ub.nrows() > 0 {
        let mut a_ub_map = std::collections::HashMap::new();
        for triplet in primitives.a_ub.triplet_iter() {
            let row_idx = triplet.row.unbound();
            let col_idx = triplet.col.unbound();
            a_ub_map.insert((row_idx, col_idx), *triplet.val);
        }

        for &old_row in selected_rows.iter() {
            for (new_col, &old_col) in selected_cols.iter().enumerate() {
                if let Some(&coeff) = a_ub_map.get(&(old_row, old_col)) {
                    if coeff != 0.0 {
                        all_constraints_triplets.push(Triplet::new(constraint_row, new_col, coeff));
                    }
                }
            }
            all_b.push(primitives.b_ub[old_row]);
            constraint_row += 1;
            n_ineq_constraints += 1;
        }

        if n_ineq_constraints > 0 {
            cone_dims.push(clarabel::solver::NonnegativeConeT(n_ineq_constraints));
        }
    }

    // Add non-positive constraints for variables (x <= 0)
    // This is equivalent to -x >= 0, so we use -I matrix
    for i in 0..n_vars {
        all_constraints_triplets.push(Triplet::new(constraint_row + i, i, -1.0));
        all_b.push(0.0);
    }
    cone_dims.push(clarabel::solver::NonnegativeConeT(n_vars));

    let total_constraints = constraint_row + n_vars;

    // Build the constraint matrix A
    let a_matrix =
        SparseColMat::try_new_from_triplets(total_constraints, n_vars, &all_constraints_triplets)
            .ok()?;

    // Convert to clarabel format using faer's direct CSC accessors
    let (symbolic, values) = a_matrix.as_ref().parts();
    let col_ptrs = symbolic.col_ptr();
    let row_indices = symbolic.row_idx();

    // Convert to Clarabel's expected types
    let colptr: Vec<usize> = col_ptrs.to_vec();
    let rowval: Vec<usize> = row_indices.to_vec();
    let nzval: Vec<f64> = values.to_vec();

    let a = CscMatrix::new(total_constraints, n_vars, colptr, rowval, nzval);

    // Create P matrix (zero for LP)
    let p = CscMatrix::<f64>::zeros((n_vars, n_vars));

    // Create solver with settings adapted for problem size
    let is_large_problem = n_vars >= 100 || total_constraints >= 200;

    let settings = if is_large_problem {
        // Settings optimized for larger problems (8+ operators)
        DefaultSettingsBuilder::default()
            .verbose(false)
            .equilibrate_enable(false) // Can slow down large problems
            .presolve_enable(true) // Simplify problem
            .static_regularization_enable(false) // Not needed for LP
            .dynamic_regularization_enable(false) // Not needed for LP
            .iterative_refinement_enable(false) // Skip for performance
            .max_iter(50) // Limit iterations for large problems
            .tol_feas(1e-6) // Relax tolerance slightly for speed
            .tol_gap_abs(1e-6)
            .tol_gap_rel(1e-6)
            .build()
            .ok()?
    } else {
        // Settings for smaller problems
        DefaultSettingsBuilder::default()
            .verbose(false)
            .equilibrate_enable(true) // Keep for numerical stability
            .presolve_enable(true) // Simplify problem
            .static_regularization_enable(false) // Not needed for LP
            .dynamic_regularization_enable(false) // Not needed for LP
            .iterative_refinement_enable(false) // Test impact
            .tol_feas(1e-8) // Match decimal precision
            .tol_gap_abs(1e-8)
            .tol_gap_rel(1e-8)
            .build()
            .ok()?
    };

    let mut solver = DefaultSolver::new(&p, &c, &a, &all_b, &cone_dims, settings).ok()?;

    // Solve
    #[cfg(debug_assertions)]
    let solve_start = std::time::Instant::now();

    solver.solve();

    #[cfg(debug_assertions)]
    {
        let solve_time = solve_start.elapsed();
        eprintln!(
            "Solver stats - vars: {}, constraints: {}, iterations: {}, time: {:?}, status: {:?}",
            n_vars, total_constraints, solver.info.iterations, solve_time, solver.solution.status
        );
    }

    match solver.solution.status {
        SolverStatus::Solved => {
            // Calculate objective value
            let obj_value: f64 = solver
                .solution
                .x
                .iter()
                .zip(&c)
                .map(|(x_i, c_i)| x_i * c_i)
                .sum();
            Some(-obj_value) // Negate because we're maximizing
        }
        _ => None,
    }
}

#[inline]
fn build_submask(_bitmap: &Mat<u8>, n_coal: usize) -> Mat<f64> {
    let mut submask = Mat::zeros(n_coal, n_coal);

    for i in 0..n_coal {
        for j in 0..=i {
            // Only check lower triangle
            // Check if coalition j is subset of i using bitmask
            // j is subset of i if (j & i) == j
            if (j & i) == j {
                submask[(i, j)] = 1.0;
            }
        }
    }

    submask
}

/// Build recursive coefficient matrix for probability estimates
pub fn build_coefficient_matrix(n_ops: usize) -> Result<SparseColMat<usize, f64>> {
    if n_ops == 0 {
        // Return a 1x1 matrix containing 0
        return SparseColMat::try_new_from_triplets(1, 1, &[]).map_err(|_| {
            crate::error::ShapleyError::ComputationError(
                "Failed to create sparse matrix".to_string(),
            )
        });
    }

    // Start with a 1x1 matrix containing 0
    let mut triplets: Vec<Triplet<usize, usize, f64>> = Vec::new();
    let mut coef_nrows = 1;
    let mut coef_ncols = 1;

    for i in 0..n_ops {
        let sz = 1 << i; // 2^i
        let new_nrows = 2 * coef_nrows;
        let new_ncols = 2 * coef_ncols;
        let mut new_triplets = Vec::new();

        // Copy existing triplets to top-left block [coef, zeros]
        for t in &triplets {
            new_triplets.push(Triplet::new(t.row, t.col, t.val));
        }

        // Bottom-left block: -coef - I
        // First add -coef entries
        for t in &triplets {
            new_triplets.push(Triplet::new(t.row + sz, t.col, -t.val));
        }
        // Then subtract identity
        for j in 0..sz {
            new_triplets.push(Triplet::new(j + sz, j, -1.0));
        }

        // Bottom-right block: coef
        for t in &triplets {
            new_triplets.push(Triplet::new(t.row + sz, t.col + sz, t.val));
        }

        triplets = new_triplets;
        coef_nrows = new_nrows;
        coef_ncols = new_ncols;
    }

    SparseColMat::try_new_from_triplets(coef_nrows, coef_ncols, &triplets).map_err(|_| {
        crate::error::ShapleyError::ComputationError(
            "Failed to create coefficient matrix".to_string(),
        )
    })
}

// TODO: This should be fixed, usize will overflow very quickly when n >= 21
#[inline]
fn factorial(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        _ => (2..=n).product(),
    }
}

#[cfg(test)]
mod tests {
    use crate::LinkBuilder;

    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_enumerate_operators() {
        let links = vec![
            {
                LinkBuilder::new("A".to_string(), "B".to_string())
                    .operator1("Alpha".to_string())
                    .build()
            },
            {
                LinkBuilder::new("B".to_string(), "C".to_string())
                    .operator1("Beta".to_string())
                    .operator2("Gamma".to_string())
                    .build()
            },
            {
                LinkBuilder::new("C".to_string(), "D".to_string())
                    .operator1("Alpha".to_string())
                    .operator2("Beta".to_string())
                    .build()
            },
        ];

        let operators = enumerate_operators(&links);
        assert_eq!(operators, vec!["Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn test_generate_coalition_bitmap() {
        let bitmap = generate_coalition_bitmap(3);

        assert_eq!((bitmap.nrows(), bitmap.ncols()), (3, 8)); // 3 operators, 2^3 coalitions

        // Empty coalition
        assert_eq!(bitmap[(0, 0)], 0);
        assert_eq!(bitmap[(1, 0)], 0);
        assert_eq!(bitmap[(2, 0)], 0);

        // Full coalition
        assert_eq!(bitmap[(0, 7)], 1);
        assert_eq!(bitmap[(1, 7)], 1);
        assert_eq!(bitmap[(2, 7)], 1);

        // Coalition {0, 2}
        assert_eq!(bitmap[(0, 5)], 1);
        assert_eq!(bitmap[(1, 5)], 0);
        assert_eq!(bitmap[(2, 5)], 1);
    }

    #[test]
    fn test_factorial() {
        assert_eq!(factorial(0), 1);
        assert_eq!(factorial(1), 1);
        assert_eq!(factorial(2), 2);
        assert_eq!(factorial(3), 6);
        assert_eq!(factorial(4), 24);
        assert_eq!(factorial(5), 120);
    }

    #[test]
    fn test_build_submask() {
        let bitmap = generate_coalition_bitmap(2);
        let submask = build_submask(&bitmap, 4);

        // submask[i,j] = 1 if coalition j is subset of coalition i
        // Coalition 0 is empty set, so only itself is a subset
        assert_eq!(submask[(0, 0)], 1.0);
        assert_eq!(submask[(0, 1)], 0.0);
        assert_eq!(submask[(0, 2)], 0.0);
        assert_eq!(submask[(0, 3)], 0.0);

        // Coalition 1 has empty set as subset, and itself
        assert_eq!(submask[(1, 0)], 1.0);
        assert_eq!(submask[(1, 1)], 1.0);
        assert_eq!(submask[(1, 2)], 0.0);
        assert_eq!(submask[(1, 3)], 0.0);

        // Coalition 3 is full coalition, all are subsets
        assert_eq!(submask[(3, 0)], 1.0);
        assert_eq!(submask[(3, 1)], 1.0);
        assert_eq!(submask[(3, 2)], 1.0);
        assert_eq!(submask[(3, 3)], 1.0);
    }

    #[test]
    fn test_shapley_value_calculation() {
        let operators = vec!["Op1".to_string(), "Op2".to_string()];
        let evalue = Col::from_iter(vec![0.0, 50.0, 50.0, 100.0]);
        let size = Col::from_iter(vec![0, 1, 1, 2]);

        let result = calculate_shapley_values(&operators, &evalue, &size, 2).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].operator, "Op1");
        assert_eq!(result[1].operator, "Op2");

        // Both operators contribute equally, so should have 0.5 each
        assert_eq!(result[0].percent, dec!(0.5));
        assert_eq!(result[1].percent, dec!(0.5));

        // Percentages should sum to 1
        let total: rust_decimal::Decimal = result.iter().map(|sv| sv.percent).sum();
        assert_eq!(total, dec!(1.0));
    }

    #[test]
    fn test_solve_coalition_values_parallel_path() {
        // Create 5 operators to trigger parallel execution (> 4)
        let operators = vec![
            "Op1".to_string(),
            "Op2".to_string(),
            "Op3".to_string(),
            "Op4".to_string(),
            "Op5".to_string(),
        ];
        let bitmap = generate_coalition_bitmap(5);

        // Create a simple LP problem that can be solved
        let n_links = 10;
        let n_constraints = 5;

        // Create simple identity matrices for testing
        let mut eq_triplets = Vec::new();
        let mut ub_triplets = Vec::new();
        for i in 0..n_constraints {
            eq_triplets.push(Triplet::new(i, i, 1.0));
            ub_triplets.push(Triplet::new(i, i, 1.0));
        }
        let a_eq = SparseColMat::try_new_from_triplets(n_constraints, n_constraints, &eq_triplets)
            .expect("Failed to create test matrix");
        let a_ub = SparseColMat::try_new_from_triplets(n_constraints, n_constraints, &ub_triplets)
            .expect("Failed to create test matrix");
        let b_eq = Col::ones(n_constraints);
        let b_ub = Col::full(n_constraints, 100.0);
        let cost = Col::full(n_links, 1.0);

        // Create operator indices
        let row_index1 = vec!["Op1".to_string(); n_constraints];
        let row_index2 = vec!["Op1".to_string(); n_constraints];
        let col_index1: Vec<_> = (0..n_links)
            .map(|i| {
                if i < 5 {
                    format!("Op{}", (i % 5) + 1)
                } else {
                    "0".to_string()
                }
            })
            .collect();
        let col_index2 = col_index1.clone();

        let primitives = LPPrimitives {
            a_eq,
            a_ub,
            b_eq,
            b_ub,
            cost,
            row_index1,
            row_index2,
            col_index1,
            col_index2,
        };

        let (values, sizes) = solve_coalition_values(&operators, &bitmap, &primitives).unwrap();

        // With 5 operators, we have 2^5 = 32 coalitions
        assert_eq!(values.nrows(), 32);
        assert_eq!(sizes.nrows(), 32);

        // Verify coalition sizes are correct
        assert_eq!(sizes[0], 0); // Empty coalition
        assert_eq!(sizes[31], 5); // Full coalition

        // The parallel code should have been executed
        // We can't directly test that parallel code ran, but we can verify
        // the results are consistent
        for i in 0..32 {
            let expected_size = (0..5).filter(|&bit| (i >> bit) & 1 == 1).count();
            assert_eq!(sizes[i], expected_size);
        }
    }
}
