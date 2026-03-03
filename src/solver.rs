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
    /// Create a new LP solver from primitives
    pub(crate) fn new(primitives: &LpPrimitives) -> Result<Self> {
        let mut problem = microlp::Problem::new(OptimizationDirection::Minimize);

        // Add variables with cost coefficients and non-negativity bounds
        let vars: Vec<Variable> = primitives
            .cost
            .iter()
            .map(|&c| problem.add_var(c, (0.0, f64::INFINITY)))
            .collect();

        // Add equality constraints (A_eq * x = b_eq)
        let eq_rows = rows_from_csc(&primitives.a_eq);
        for (row_idx, entries) in eq_rows.iter().enumerate() {
            let terms: Vec<(Variable, f64)> =
                entries.iter().map(|&(col, val)| (vars[col], val)).collect();
            problem.add_constraint(&terms, ComparisonOp::Eq, primitives.b_eq[row_idx]);
        }

        // Add inequality constraints (A_ub * x <= b_ub)
        let ub_rows = rows_from_csc(&primitives.a_ub);
        for (row_idx, entries) in ub_rows.iter().enumerate() {
            let terms: Vec<(Variable, f64)> =
                entries.iter().map(|&(col, val)| (vars[col], val)).collect();
            problem.add_constraint(&terms, ComparisonOp::Le, primitives.b_ub[row_idx]);
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
