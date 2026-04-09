use microlp::{ComparisonOp, StopReason, VarDomain};
#[cfg(test)]
use microlp::{OptimizationDirection, Variable};
use sprs::TriMatI;

use crate::{
    error::{Result, ShapleyError},
    lp_builder::LpPrimitives,
    sparse::CscMatrix,
};

/// Pre-computed row-oriented representation of the LP constraint matrices.
/// Built once from the full primitives, then reused for every coalition.
pub(crate) struct PrecomputedRows {
    /// Equality constraint rows: each entry is (original_col_index, coefficient)
    eq_rows: Vec<Vec<(usize, f64)>>,
    /// Inequality constraint rows: each entry is (original_col_index, coefficient)
    ub_rows: Vec<Vec<(usize, f64)>>,
}

impl PrecomputedRows {
    /// Build from the full (unfiltered) LP primitives. Call once before the coalition loop.
    pub(crate) fn new(primitives: &LpPrimitives) -> Self {
        Self {
            eq_rows: rows_from_csc(&primitives.a_eq),
            ub_rows: rows_from_csc(&primitives.a_ub),
        }
    }
}

/// Reusable per-thread buffers for coalition LP construction.
pub(crate) struct CoalitionBuffers {
    pub col_remap: Vec<usize>,
    pub cost: Vec<f64>,
    pub keep_rows: Vec<usize>,
    pub var_mins: Vec<f64>,
    pub var_maxs: Vec<f64>,
    pub var_domains: Vec<VarDomain>,
    pub ops: Vec<ComparisonOp>,
    pub rhs: Vec<f64>,
}

impl CoalitionBuffers {
    pub fn new(n_cols: usize) -> Self {
        Self {
            col_remap: vec![usize::MAX; n_cols],
            cost: Vec::with_capacity(n_cols),
            keep_rows: Vec::with_capacity(256),
            var_mins: Vec::with_capacity(n_cols),
            var_maxs: Vec::with_capacity(n_cols),
            var_domains: Vec::with_capacity(n_cols),
            ops: Vec::with_capacity(1024),
            rhs: Vec::with_capacity(1024),
        }
    }

    pub fn reset(&mut self) {
        self.col_remap.fill(usize::MAX);
        self.cost.clear();
        self.keep_rows.clear();
        self.var_mins.clear();
        self.var_maxs.clear();
        self.var_domains.clear();
        self.ops.clear();
        self.rhs.clear();
    }
}

/// Solver termination status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolveStatus {
    Solved,
    Infeasible,
}

/// LP solver wrapper for microlp (used in tests)
#[cfg(test)]
pub(crate) struct LpSolver {
    problem: microlp::Problem,
}

/// Result of solving an LP (used in tests)
#[cfg(test)]
#[derive(Debug)]
#[allow(dead_code)]
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

#[cfg(test)]
#[allow(dead_code)]
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

/// Solve result from the coalition solver.
pub(crate) struct CoalitionResult {
    pub status: SolveStatus,
    pub objective_value: f64,
}

/// Create and solve an LP for a specific coalition using pre-computed
/// row-oriented constraint data. Avoids rebuilding CSC matrices per coalition.
///
/// `coalition_mask` has bit i set for each operator i in the coalition,
/// plus `ALWAYS_BIT` so that Public/Private/empty operators always match.
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_coalition(
    primitives: &LpPrimitives,
    precomputed: &PrecomputedRows,
    buffers: &mut CoalitionBuffers,
    coalition_mask: u32,
    col_op1_mask: &[u32],
    col_op2_mask: &[u32],
    row_op1_mask: &[u32],
    row_op2_mask: &[u32],
) -> Result<CoalitionResult> {
    let n_cols = col_op1_mask.len();

    buffers.reset();

    // Ensure col_remap is large enough (may grow between calls if n_cols changes)
    if buffers.col_remap.len() < n_cols {
        buffers.col_remap.resize(n_cols, usize::MAX);
    }

    // Step 1: Compute keep_cols and build a remap array
    let mut new_col = 0usize;

    for i in 0..n_cols {
        if (col_op1_mask[i] & coalition_mask) != 0 && (col_op2_mask[i] & coalition_mask) != 0 {
            buffers.col_remap[i] = new_col;
            buffers.cost.push(primitives.cost[i]);
            new_col += 1;
        }
    }

    if new_col == 0 {
        return Err(ShapleyError::MatrixConstructionError(
            "No columns selected for coalition".to_string(),
        ));
    }

    // Step 2: Compute keep_rows for A_ub
    for i in 0..row_op1_mask.len() {
        if (row_op1_mask[i] & coalition_mask) != 0 && (row_op2_mask[i] & coalition_mask) != 0 {
            buffers.keep_rows.push(i);
        }
    }

    let n_kept = new_col;

    // Step 3: Build a single CSR constraint matrix via triplets, avoiding
    // per-row CsVec allocations.

    let n_eq_rows = precomputed.eq_rows.len();
    let n_ub_rows = buffers.keep_rows.len();
    let n_total_rows = n_eq_rows + n_ub_rows;

    let mut triplets = TriMatI::<f64, usize>::new((n_total_rows, n_kept));
    let mut row = 0;

    // Equality constraints — all rows, remap columns
    for (row_idx, entries) in precomputed.eq_rows.iter().enumerate() {
        for &(old_col, val) in entries {
            let nc = buffers.col_remap[old_col];
            if nc != usize::MAX {
                triplets.add_triplet(row, nc, val);
            }
        }
        buffers.ops.push(ComparisonOp::Eq);
        buffers.rhs.push(primitives.b_eq[row_idx]);
        row += 1;
    }

    // Inequality constraints — only kept rows, remap columns
    for keep_idx in 0..n_ub_rows {
        let row_idx = buffers.keep_rows[keep_idx];
        for &(old_col, val) in &precomputed.ub_rows[row_idx] {
            let nc = buffers.col_remap[old_col];
            if nc != usize::MAX {
                triplets.add_triplet(row, nc, val);
            }
        }
        buffers.ops.push(ComparisonOp::Le);
        buffers.rhs.push(primitives.b_ub[row_idx]);
        row += 1;
    }

    let constraint_matrix = triplets.to_csr();

    // Build variable bounds and domains
    buffers.var_mins.resize(n_kept, 0.0);
    buffers.var_maxs.resize(n_kept, f64::INFINITY);
    buffers.var_domains.resize(n_kept, VarDomain::Real);

    // Solve using the vendored solver directly with pre-built CSR matrix
    let solver_result = crate::simplex::solver::Solver::try_new_from_matrix(
        &buffers.cost,
        &buffers.var_mins,
        &buffers.var_maxs,
        constraint_matrix,
        &buffers.ops,
        &buffers.rhs,
        &buffers.var_domains,
        None,
    );

    match solver_result {
        Ok(mut solver) => match solver.initial_solve() {
            Ok(StopReason::Finished) => Ok(CoalitionResult {
                status: SolveStatus::Solved,
                objective_value: solver.cur_obj_val,
            }),
            Ok(StopReason::Limit) => Ok(CoalitionResult {
                status: SolveStatus::Solved,
                objective_value: solver.cur_obj_val,
            }),
            Err(microlp::Error::Infeasible) => Ok(CoalitionResult {
                status: SolveStatus::Infeasible,
                objective_value: 0.0,
            }),
            Err(e) => Err(ShapleyError::LpSolver(format!("LP solver error: {e}"))),
        },
        Err(microlp::Error::Infeasible) => Ok(CoalitionResult {
            status: SolveStatus::Infeasible,
            objective_value: 0.0,
        }),
        Err(e) => Err(ShapleyError::LpSolver(format!("LP solver error: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        lp_builder::LpBuilderInput,
        types::{ConsolidatedDemand, ConsolidatedLink},
    };

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
