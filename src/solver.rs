use crate::{
    error::{Result, ShapleyError},
    lp_builder::LpPrimitives,
    sparse::CscMatrix,
};
use microlp::{ComparisonOp, OptimizationDirection, Variable};

/// Solver termination status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolveStatus {
    Solved,
    Infeasible,
}

/// LP solver wrapper for microlp
pub(crate) struct LpSolver {
    problem: microlp::Problem,
}

/// Result of solving an LP
#[derive(Debug)]
pub(crate) struct LpSolution {
    pub status: SolveStatus,
    pub objective_value: f64,
}

/// Collect all entries from a CSC matrix into row-oriented form.
/// Returns a Vec indexed by row, each containing (col_index, value) pairs.
fn rows_from_csc(matrix: &CscMatrix<f64>) -> Vec<Vec<(usize, f64)>> {
    let mut rows: Vec<Vec<(usize, f64)>> = vec![Vec::new(); matrix.m];
    for col in 0..matrix.n {
        let start = matrix.colptr[col];
        let end = matrix.colptr[col + 1];
        for idx in start..end {
            rows[matrix.rowval[idx]].push((col, matrix.nzval[idx]));
        }
    }
    rows
}

impl LpSolver {
    /// Create a new LP solver from individual components
    pub(crate) fn new(
        cost: &[f64],
        a_eq: &CscMatrix<f64>,
        b_eq: &[f64],
        a_ub: &CscMatrix<f64>,
        b_ub: &[f64],
    ) -> Result<Self> {
        let mut problem = microlp::Problem::new(OptimizationDirection::Minimize);

        // Add variables with cost coefficients and non-negativity bounds
        let vars: Vec<Variable> = cost
            .iter()
            .map(|&c| problem.add_var(c, (0.0, f64::INFINITY)))
            .collect();

        // Add equality constraints (A_eq * x = b_eq)
        let eq_rows = rows_from_csc(a_eq);
        for (row_idx, entries) in eq_rows.iter().enumerate() {
            let terms: Vec<(Variable, f64)> =
                entries.iter().map(|&(col, val)| (vars[col], val)).collect();
            problem.add_constraint(&terms, ComparisonOp::Eq, b_eq[row_idx]);
        }

        // Add inequality constraints (A_ub * x <= b_ub)
        let ub_rows = rows_from_csc(a_ub);
        for (row_idx, entries) in ub_rows.iter().enumerate() {
            let terms: Vec<(Variable, f64)> =
                entries.iter().map(|&(col, val)| (vars[col], val)).collect();
            problem.add_constraint(&terms, ComparisonOp::Le, b_ub[row_idx]);
        }

        Ok(Self { problem })
    }

    /// Solve the LP problem
    pub(crate) fn solve(self) -> Result<LpSolution> {
        match self.problem.solve() {
            Ok(solution) => Ok(LpSolution {
                status: SolveStatus::Solved,
                objective_value: solution.objective(),
            }),
            Err(microlp::Error::Infeasible) => Ok(LpSolution {
                status: SolveStatus::Infeasible,
                objective_value: 0.0,
            }),
            Err(e) => Err(ShapleyError::LpSolver(format!("LP solver error: {e}"))),
        }
    }
}

/// Create LP solver for a specific coalition using precomputed bitmasks.
///
/// `coalition_mask` has bit i set for each operator i in the coalition,
/// plus `ALWAYS_BIT` so that Public/Private/empty operators always match.
pub(crate) fn create_coalition_solver(
    primitives: &LpPrimitives,
    coalition_mask: u32,
    col_op1_mask: &[u32],
    col_op2_mask: &[u32],
    row_op1_mask: &[u32],
    row_op2_mask: &[u32],
) -> Result<LpSolver> {
    // Filter columns: keep if BOTH operators match the coalition
    let keep_cols: Vec<usize> = (0..col_op1_mask.len())
        .filter(|&i| {
            (col_op1_mask[i] & coalition_mask) != 0 && (col_op2_mask[i] & coalition_mask) != 0
        })
        .collect();

    if keep_cols.is_empty() {
        return Err(ShapleyError::MatrixConstructionError(
            "No columns selected for coalition".to_string(),
        ));
    }

    // Filter rows for A_ub: keep if BOTH operators match the coalition
    let keep_rows: Vec<usize> = (0..row_op1_mask.len())
        .filter(|&i| {
            (row_op1_mask[i] & coalition_mask) != 0 && (row_op2_mask[i] & coalition_mask) != 0
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

    LpSolver::new(
        &cost_filtered,
        &a_eq_filtered,
        &primitives.b_eq,
        &a_ub_filtered,
        &b_ub_filtered,
    )
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
        let solver = LpSolver::new(
            &primitives.cost,
            &primitives.a_eq,
            &primitives.b_eq,
            &primitives.a_ub,
            &primitives.b_ub,
        );

        assert!(solver.is_ok());
    }
}
