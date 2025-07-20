use crate::{
    error::{Result, ShapleyError},
    lp_builder::LpPrimitives,
};
use clarabel::{
    algebra::CscMatrix,
    solver::{DefaultSettings, DefaultSolver, IPSolver, SolverStatus, SupportedConeT},
};

/// Type alias for stacked constraints
type StackedConstraints = (CscMatrix<f64>, Vec<f64>, Vec<SupportedConeT<f64>>);

/// LP solver wrapper for Clarabel
pub(crate) struct LpSolver {
    solver: DefaultSolver<f64>,
}

/// Result of solving an LP
#[derive(Debug)]
pub(crate) struct LpSolution {
    pub status: SolverStatus,
    pub objective_value: f64,
}

impl LpSolver {
    /// Create a new LP solver from primitives
    pub(crate) fn new(primitives: &LpPrimitives) -> Result<Self> {
        // Convert our LP to Clarabel's standard form:
        // minimize    (1/2) x'Px + q'x
        // subject to  Ax + s = b
        //            s in K
        //
        // For LP, we have:
        // - P = 0 (no quadratic term)
        // - q = cost vector
        // - Equality constraints: A_eq * x = b_eq
        // - Inequality constraints: A_ub * x <= b_ub
        //
        // We need to combine into single A matrix and handle slack variables

        let n_vars = primitives.cost.len();

        // Create zero P matrix (no quadratic objective)
        let p = CscMatrix::new(n_vars, n_vars, vec![0; n_vars + 1], vec![], vec![]);

        // Cost vector
        let q = primitives.cost.clone();

        // Stack equality and inequality constraints
        let (a, b, cones) = stack_constraints(primitives)?;

        // Configure solver settings
        let settings = DefaultSettings::<f64> {
            verbose: false,
            max_iter: 10000,
            tol_gap_abs: 1e-8,
            tol_gap_rel: 1e-8,
            tol_feas: 1e-8,
            ..Default::default()
        };

        // Create solver
        let solver = DefaultSolver::new(&p, &q, &a, &b, &cones, settings).map_err(|e| {
            ShapleyError::LpSolver(format!("Failed to create Clarabel solver: {e}"))
        })?;

        Ok(Self { solver })
    }

    /// Solve the LP problem
    pub(crate) fn solve(mut self) -> Result<LpSolution> {
        self.solver.solve();

        let info = &self.solver.info;

        // Check solver status
        match info.status {
            SolverStatus::Solved | SolverStatus::AlmostSolved => Ok(LpSolution {
                status: info.status,
                objective_value: info.cost_primal,
            }),
            SolverStatus::PrimalInfeasible | SolverStatus::AlmostPrimalInfeasible => Err(
                ShapleyError::LpSolver("Problem is primal infeasible".to_string()),
            ),
            SolverStatus::DualInfeasible | SolverStatus::AlmostDualInfeasible => Err(
                ShapleyError::LpSolver("Problem is dual infeasible (unbounded)".to_string()),
            ),
            SolverStatus::MaxIterations => Err(ShapleyError::LpSolver(
                "Maximum iterations reached".to_string(),
            )),
            SolverStatus::MaxTime => Err(ShapleyError::LpSolver("Time limit reached".to_string())),
            SolverStatus::NumericalError => Err(ShapleyError::LpSolver(
                "Numerical error in solver".to_string(),
            )),
            SolverStatus::InsufficientProgress => Err(ShapleyError::LpSolver(
                "Solver made insufficient progress".to_string(),
            )),
            _ => Err(ShapleyError::LpSolver(format!(
                "Unexpected solver status: {status:?}",
                status = info.status
            ))),
        }
    }
}

/// Stack equality and inequality constraints for Clarabel format
fn stack_constraints(primitives: &LpPrimitives) -> Result<StackedConstraints> {
    let n_vars = primitives.cost.len();
    let n_eq = primitives.a_eq.m;
    let n_ineq = primitives.a_ub.m;
    let n_nonneg = n_vars; // Add non-negativity constraints for all variables
    let n_constraints = n_eq + n_ineq + n_nonneg;

    // We need to stack A_eq, A_ub, and -I (for x >= 0) vertically
    let mut triplets = Vec::new();

    // Add equality constraint entries
    for col in 0..primitives.a_eq.n {
        let start = primitives.a_eq.colptr[col];
        let end = primitives.a_eq.colptr[col + 1];

        for idx in start..end {
            let row = primitives.a_eq.rowval[idx];
            let val = primitives.a_eq.nzval[idx];
            triplets.push((row, col, val));
        }
    }

    // Add inequality constraint entries (offset rows by n_eq)
    for col in 0..primitives.a_ub.n {
        let start = primitives.a_ub.colptr[col];
        let end = primitives.a_ub.colptr[col + 1];

        for idx in start..end {
            let row = primitives.a_ub.rowval[idx] + n_eq;
            let val = primitives.a_ub.nzval[idx];
            triplets.push((row, col, val));
        }
    }

    // Add non-negativity constraints: -I * x <= 0 (i.e., x >= 0)
    let offset = n_eq + n_ineq;
    for i in 0..n_vars {
        triplets.push((offset + i, i, -1.0));
    }

    // Build combined constraint matrix
    let a = build_csc_from_triplets(&triplets, n_constraints, n_vars)?;

    // Stack b vectors
    let mut b = Vec::with_capacity(n_constraints);
    b.extend_from_slice(&primitives.b_eq);
    b.extend_from_slice(&primitives.b_ub);
    // Add zeros for non-negativity constraints
    b.extend(vec![0.0; n_nonneg]);

    // Define cones: equality constraints are ZeroCone, inequalities and non-negativity are NonnegativeCone
    let mut cones = Vec::new();

    if n_eq > 0 {
        cones.push(SupportedConeT::ZeroConeT(n_eq));
    }

    if n_ineq + n_nonneg > 0 {
        cones.push(SupportedConeT::NonnegativeConeT(n_ineq + n_nonneg));
    }

    Ok((a, b, cones))
}

/// Build CSC matrix from triplets (helper function)
fn build_csc_from_triplets(
    triplets: &[(usize, usize, f64)],
    n_rows: usize,
    n_cols: usize,
) -> Result<CscMatrix<f64>> {
    if triplets.is_empty() {
        return Ok(CscMatrix::new(
            n_rows,
            n_cols,
            vec![0; n_cols + 1],
            vec![],
            vec![],
        ));
    }

    // Sort triplets by column, then row
    let mut sorted_triplets = triplets.to_vec();
    sorted_triplets.sort_by_key(|&(r, c, _)| (c, r));

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    let mut current_col = 0;

    for &(row, col, val) in &sorted_triplets {
        // Fill in empty columns
        while current_col < col {
            col_ptr.push(row_ind.len());
            current_col += 1;
        }

        row_ind.push(row);
        values.push(val);
    }

    // Fill remaining columns
    while current_col < n_cols {
        col_ptr.push(row_ind.len());
        current_col += 1;
    }

    Ok(CscMatrix::new(n_rows, n_cols, col_ptr, row_ind, values))
}

/// Create LP solver for a specific coalition
pub(crate) fn create_coalition_solver(
    primitives: &LpPrimitives,
    _coalition_bitmap: u32,
    col_op1: &[String],
    coalition_operators: &[String],
) -> Result<LpSolver> {
    // Always include "Public" and "Private" operators
    let always_included = ["Public", "Private"];

    // Filter columns based on coalition membership
    // A column is included if BOTH col_op1 AND col_op2 are in coalition (or always included)
    let keep_cols: Vec<usize> = (0..col_op1.len())
        .filter(|&i| {
            let op1 = &primitives.col_op1[i];
            let op2 = &primitives.col_op2[i];

            let op1_included =
                always_included.contains(&op1.as_str()) || coalition_operators.contains(op1);
            let op2_included =
                always_included.contains(&op2.as_str()) || coalition_operators.contains(op2);

            op1_included && op2_included
        })
        .collect();

    if keep_cols.is_empty() {
        return Err(ShapleyError::MatrixConstructionError(
            "No columns selected for coalition".to_string(),
        ));
    }

    // Determine if this is the grand coalition (contains all operators)
    // First, collect all unique operators from row_op1 and row_op2 (excluding empty, Public, Private)
    let mut all_operators = std::collections::HashSet::new();
    for op in primitives.row_op1.iter().chain(primitives.row_op2.iter()) {
        if !op.is_empty() && op != "Public" && op != "Private" {
            all_operators.insert(op.as_str());
        }
    }

    // Filter rows for A_ub based on coalition membership
    // A row is included if BOTH row_op1 AND row_op2 are in coalition (or always included)
    let keep_rows: Vec<usize> = (0..primitives.row_op1.len())
        .filter(|&i| {
            let op1 = &primitives.row_op1[i];
            let op2 = &primitives.row_op2[i];

            // Include constraints if both operators are in the coalition or if operators are empty (universal constraints)
            let op1_included = op1.is_empty()
                || always_included.contains(&op1.as_str())
                || coalition_operators.contains(op1);
            let op2_included = op2.is_empty()
                || always_included.contains(&op2.as_str())
                || coalition_operators.contains(op2);
            op1_included && op2_included
        })
        .collect();

    // Filter constraint matrices
    let a_eq_filtered = filter_columns(&primitives.a_eq, &keep_cols)?;
    let a_ub_filtered = filter_rows_and_columns(&primitives.a_ub, &keep_rows, &keep_cols)?;

    // Filter b_ub vector
    let b_ub_filtered: Vec<f64> = keep_rows
        .iter()
        .filter_map(|&i| primitives.b_ub.get(i))
        .copied()
        .collect();

    // Filter cost vector
    let cost_filtered: Vec<f64> = keep_cols
        .iter()
        .filter_map(|&i| primitives.cost.get(i))
        .copied()
        .collect();

    // Create new primitives with filtered data
    let filtered_primitives = LpPrimitives {
        a_eq: a_eq_filtered,
        a_ub: a_ub_filtered,
        b_eq: primitives.b_eq.clone(),
        b_ub: b_ub_filtered,
        cost: cost_filtered,
        row_op1: keep_rows
            .iter()
            .filter_map(|&i| primitives.row_op1.get(i).cloned())
            .collect(),
        row_op2: keep_rows
            .iter()
            .filter_map(|&i| primitives.row_op2.get(i).cloned())
            .collect(),
        col_op1: keep_cols
            .iter()
            .filter_map(|&i| primitives.col_op1.get(i).cloned())
            .collect(),
        col_op2: keep_cols
            .iter()
            .filter_map(|&i| primitives.col_op2.get(i).cloned())
            .collect(),
    };

    LpSolver::new(&filtered_primitives)
}

/// Filter columns of a CSC matrix
fn filter_columns(matrix: &CscMatrix<f64>, keep: &[usize]) -> Result<CscMatrix<f64>> {
    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    for &col in keep {
        if col >= matrix.n {
            return Err(ShapleyError::MatrixConstructionError(format!(
                "Column index {col} out of bounds"
            )));
        }

        let start = matrix.colptr[col];
        let end = matrix.colptr[col + 1];

        for idx in start..end {
            row_ind.push(matrix.rowval[idx]);
            values.push(matrix.nzval[idx]);
        }

        col_ptr.push(row_ind.len());
    }

    Ok(CscMatrix::new(
        matrix.m,
        keep.len(),
        col_ptr,
        row_ind,
        values,
    ))
}

/// Filter both rows and columns of a CSC matrix
fn filter_rows_and_columns(
    matrix: &CscMatrix<f64>,
    keep_rows: &[usize],
    keep_cols: &[usize],
) -> Result<CscMatrix<f64>> {
    // Create row mapping
    let mut row_map = vec![None; matrix.m];
    for (new_idx, &old_idx) in keep_rows.iter().enumerate() {
        if old_idx < matrix.m {
            row_map[old_idx] = Some(new_idx);
        }
    }

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    for &col in keep_cols {
        if col >= matrix.n {
            return Err(ShapleyError::MatrixConstructionError(format!(
                "Column index {col} out of bounds"
            )));
        }

        let start = matrix.colptr[col];
        let end = matrix.colptr[col + 1];

        for idx in start..end {
            let row = matrix.rowval[idx];
            // Only include if row is in keep_rows
            if let Some(new_row) = row_map.get(row).and_then(|&r| r) {
                row_ind.push(new_row);
                values.push(matrix.nzval[idx]);
            }
        }

        col_ptr.push(row_ind.len());
    }

    Ok(CscMatrix::new(
        keep_rows.len(),
        keep_cols.len(),
        col_ptr,
        row_ind,
        values,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lp_builder::LpBuilderInput;
    use crate::types::{ConsolidatedDemand, ConsolidatedLink};

    #[test]
    fn test_solver_creation() {
        // Create simple test data
        let links = vec![ConsolidatedLink {
            device1: "A".to_string(),
            device2: "B".to_string(),
            latency: 1.0,
            bandwidth: 10.0,
            operator1: "Op1".to_string(),
            operator2: "Op1".to_string(),
            shared: 1,
            link_type: 0,
        }];

        let demands = vec![ConsolidatedDemand {
            start: "A".to_string(),
            end: "B".to_string(),
            receivers: 1,
            traffic: 5.0,
            priority: 1.0,
            kind: 1,
            multicast: false,
            original: 1,
        }];

        let lp_builder = LpBuilderInput::new(&links, &demands);
        let primitives = lp_builder
            .build()
            .expect("LP builder should succeed in tests");
        let solver = LpSolver::new(&primitives);

        assert!(solver.is_ok());
    }
}
