use crate::{
    consolidation::{consolidate_demand, consolidate_links},
    error::{Result, ShapleyError},
    lp_builder::LpBuilderInput,
    solver::create_coalition_solver,
    types::{Demands, Devices, PrivateLinks, PublicLinks},
    utils::{factorial, generate_bitmap},
    validation::check_inputs,
};
use clarabel::solver::SolverStatus;
use faer::prelude::*;
use rayon::prelude::*;
use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

#[cfg(feature = "serde")]
use {
    serde::{Deserialize, Serialize},
    tabled::Tabled,
};

// For clarity
pub type Operator = String;

// Since shapley value is per operator, we just use a hashmap
pub type ShapleyOutput = BTreeMap<Operator, ShapleyValue>;

/// Input parameters for Shapley computation
#[derive(Debug)]
pub struct ShapleyInput {
    pub private_links: PrivateLinks,
    pub devices: Devices,
    pub demands: Demands,
    pub public_links: PublicLinks,
    pub operator_uptime: f64,
    pub contiguity_bonus: f64,
    pub demand_multiplier: f64,
}

impl ShapleyInput {
    pub fn compute(&self) -> Result<ShapleyOutput> {
        let shapley = Shapley::new(
            self.private_links.clone(),
            self.devices.clone(),
            self.demands.clone(),
            self.public_links.clone(),
            self.operator_uptime,
            self.contiguity_bonus,
            self.demand_multiplier,
        );

        let output = shapley.compute()?;
        Ok(output)
    }
}

/// Individual Shapley value for an operator
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize, Tabled))]
#[derive(Debug, Clone, PartialEq)]
pub struct ShapleyValue {
    pub value: f64,
    pub proportion: f64,
}

impl Display for ShapleyValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "value: {}, proportion: {}", self.value, self.proportion)
    }
}

#[derive(Debug)]
struct Shapley {
    pub private_links: PrivateLinks,
    pub devices: Devices,
    pub demands: Demands,
    pub public_links: PublicLinks,
    pub operator_uptime: f64,
    pub contiguity_bonus: f64,
    pub demand_multiplier: f64,
}

impl Shapley {
    fn new(
        private_links: PrivateLinks,
        devices: Devices,
        demands: Demands,
        public_links: PublicLinks,
        operator_uptime: f64,
        contiguity_bonus: f64,
        demand_multiplier: f64,
    ) -> Self {
        Self {
            private_links,
            devices,
            demands,
            public_links,
            operator_uptime,
            contiguity_bonus,
            demand_multiplier,
        }
    }

    fn compute(&self) -> Result<ShapleyOutput> {
        // Validate inputs
        check_inputs(
            &self.private_links,
            &self.devices,
            &self.demands,
            &self.public_links,
            self.operator_uptime,
        )?;

        // Enumerate all operators (excluding "Private" and "Public")
        let mut operators: Vec<String> = self
            .devices
            .iter()
            .map(|d| d.operator.clone())
            .filter(|op| op != "Private" && op != "Public")
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        operators.sort();

        let n_operators = operators.len();
        if n_operators == 0 {
            return Ok(ShapleyOutput::new());
        }

        // Add hard limit to prevent computationally infeasible problems
        const MAX_OPERATORS: usize = 20;
        if n_operators > MAX_OPERATORS {
            return Err(ShapleyError::TooManyOperators {
                count: n_operators,
                limit: MAX_OPERATORS,
            });
        }

        // Consolidate demands and links
        let full_demand = consolidate_demand(&self.demands, self.demand_multiplier)?;
        let full_map = consolidate_links(
            &self.private_links,
            &self.devices,
            &full_demand,
            &self.public_links,
            self.contiguity_bonus,
        )?;

        // Build LP primitives
        let primitives = LpBuilderInput::new(&full_map, &full_demand).build()?;

        // Generate coalition bitmap
        let bitmap = generate_bitmap(n_operators);
        let n_coalitions = 1 << n_operators;

        // Solve LP for each coalition
        let coalition_values: Vec<Option<f64>> = (0..n_coalitions)
            .into_par_iter()
            .map(|coalition_idx| {
                // Check which operators are in this coalition
                let mut coalition_operators = Vec::new();
                for (op_idx, operator) in operators.iter().enumerate() {
                    if (coalition_idx & (1 << op_idx)) != 0 {
                        coalition_operators.push(operator.clone());
                    }
                }

                // Create solver for this coalition
                let coalition_bitmap = coalition_idx as u32;

                match create_coalition_solver(
                    &primitives,
                    coalition_bitmap,
                    &primitives.col_op1,
                    &coalition_operators,
                ) {
                    Ok(solver) => {
                        // Solve and return the optimal value
                        match solver.solve() {
                            Ok(solution) => {
                                if matches!(
                                    solution.status,
                                    SolverStatus::Solved | SolverStatus::AlmostSolved
                                ) {
                                    let value = -solution.objective_value; // Negative because we minimize
                                    Some(value)
                                } else {
                                    None // Infeasible coalition
                                }
                            }
                            Err(_) => None,
                        }
                    }
                    Err(_) => None,
                }
            })
            .collect();

        // Compute expected values with operator uptime
        let expected_values = if self.operator_uptime < 1.0 {
            compute_expected_values(&coalition_values, n_operators, self.operator_uptime)?
        } else {
            coalition_values
                .iter()
                .map(|&v| v.unwrap_or(f64::NEG_INFINITY))
                .collect()
        };

        // Compute Shapley values
        let shapley_values =
            compute_shapley_values(&expected_values, &bitmap, n_operators, &operators);

        // Convert to output format
        let total_value: f64 = shapley_values.iter().map(|v| v.max(0.0)).sum();

        let output = operators
            .into_iter()
            .zip(shapley_values)
            .map(|(operator, value)| {
                let proportion = if total_value > 0.0 {
                    (value.max(0.0) / total_value * 100.0) / 100.0
                } else {
                    0.0
                };

                (operator, ShapleyValue { value, proportion })
            })
            .collect();

        Ok(output)
    }
}

/// Compute expected values considering operator uptime
fn compute_expected_values(
    svalue: &[Option<f64>],
    n_operators: usize,
    operator_uptime: f64,
) -> Result<Vec<f64>> {
    let n_coal = 1 << n_operators;
    let uptime = operator_uptime;

    // Replace None with -inf
    let svalue_vec: Vec<f64> = svalue
        .iter()
        .map(|&v| v.unwrap_or(f64::NEG_INFINITY))
        .collect();

    // Count coalition sizes
    let size: Vec<usize> = (0..n_coal)
        .map(|i| (i as u32).count_ones() as usize)
        .collect();

    // Base probabilities
    let base_p: Vec<f64> = size.iter().map(|&s| uptime.powi(s as i32)).collect();

    // Build submask: submask[i, j] = 1 if coalition j is a subset of coalition i and j <= i (lower triangle)
    let submask = Mat::from_fn(n_coal, n_coal, |i, j| {
        if (j & i) == j && j <= i { 1.0 } else { 0.0 }
    });

    // Build bp_masked = base_p as column vector broadcasted across, then masked
    // Python: bp_masked = base_p * submask (broadcasts base_p across columns)
    // NumPy broadcasts 1D arrays along the last axis (columns)
    let mut bp_masked = Mat::zeros(n_coal, n_coal);
    for i in 0..n_coal {
        for j in 0..n_coal {
            bp_masked[(i, j)] = base_p[j] * submask[(i, j)];
        }
    }

    // Build coefficient matrix
    let coef_vec = build_coefficient_matrix(n_operators);
    let coef_dense = Mat::from_fn(n_coal, n_coal, |r, c| coef_vec[r][c] as f64);

    // Python: term = bp_masked @ (coef_dense * submask)
    // Use faer's zip! macro for element-wise multiplication
    let mut coef_masked = Mat::zeros(n_coal, n_coal);
    zip!(&mut coef_masked, &coef_dense, &submask).for_each(|unzip!(cm, cd, sm)| {
        *cm = cd * sm;
    });

    let term = &bp_masked * &coef_masked; // Matrix multiplication

    // Python: part = (bp_masked + term) * submask
    // Use faer's zip! macro for element-wise operations
    let mut part = Mat::zeros(n_coal, n_coal);
    zip!(&mut part, &bp_masked, &term, &submask).for_each(|unzip!(p, bp, t, sm)| {
        *p = (bp + t) * sm;
    });

    // Python: evalue = (svalue * part).sum(axis=1)
    // This broadcasts svalue as a row vector element-wise with each row of part
    // For row i: multiply [svalue[0], svalue[1], ..., svalue[n-1]] with [part[i,0], part[i,1], ..., part[i,n-1]]
    // Then sum the products
    let mut evalue = vec![0.0; n_coal];

    for i in 0..n_coal {
        let mut sum = 0.0;
        for j in 0..n_coal {
            if svalue_vec[j].is_finite() {
                sum += svalue_vec[j] * part[(i, j)];
            }
        }
        evalue[i] = sum;
    }

    // The Python implementation has a special case for the empty coalition's value
    if let Some(v) = svalue[0] {
        if v.is_finite() {
            evalue[0] = v;
        }
    }

    Ok(evalue)
}

/// Build coefficient matrix for expected value computation
fn build_coefficient_matrix(n_operators: usize) -> Vec<Vec<i32>> {
    let n_coalitions = 1 << n_operators;
    let mut coef = vec![vec![0i32; n_coalitions]; n_coalitions];

    // Initialize with 0 (Python starts with empty sparse matrix)
    coef[0][0] = 0;

    // Build recursively as in Python implementation
    for op in 0..n_operators {
        let size = 1 << op;
        let _new_size = size * 2;

        // Create new coefficient matrix
        let mut new_coef = vec![vec![0i32; n_coalitions]; n_coalitions];

        // Copy existing values
        for i in 0..size {
            for j in 0..size {
                new_coef[i][j] = coef[i][j];
            }
        }

        // Fill new quadrants
        for i in 0..size {
            for j in 0..size {
                // Bottom-left: -coef - I
                new_coef[size + i][j] = -coef[i][j];
                if i == j {
                    new_coef[size + i][j] -= 1;
                }

                // Bottom-right: coef
                new_coef[size + i][size + j] = coef[i][j];
            }
        }

        coef = new_coef;
    }

    coef
}

/// Compute Shapley values from coalition values
fn compute_shapley_values(
    coalition_values: &[f64],
    bitmap: &[Vec<u8>],
    n_operators: usize,
    operators: &[String],
) -> Vec<f64> {
    let mut shapley_values = vec![0.0; n_operators];
    let fact_n = factorial(n_operators);

    for (k, _operator) in operators.iter().enumerate() {
        let mut value = 0.0;

        // Find coalitions with this operator
        for coalition_idx in 0..coalition_values.len() {
            if bitmap[k][coalition_idx] == 1 {
                // Coalition with operator
                let with_value = coalition_values[coalition_idx];

                // Coalition without operator (remove bit k)
                let without_idx = coalition_idx ^ (1 << k);
                let without_value = coalition_values[without_idx];

                // Coalition size
                let coalition_size = (coalition_idx as u32).count_ones() as usize;

                // Weight calculation
                let weight = factorial(coalition_size - 1)
                    * factorial(n_operators - coalition_size)
                    / fact_n;

                value += weight * (with_value - without_value);
            }
        }

        shapley_values[k] = value;
    }

    shapley_values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Demand, Device, PrivateLink, PublicLink};

    #[test]
    fn test_shapley_computation() {
        // Create simple test data following the example format
        let private_links = vec![
            PrivateLink::new(
                "NYC1".to_string(),
                "LON1".to_string(),
                10.0,
                100.0,
                1.0,
                Some(1),
            ),
            PrivateLink::new(
                "LON1".to_string(),
                "PAR1".to_string(),
                10.0,
                100.0,
                1.0,
                Some(2),
            ),
        ];

        let devices = vec![
            Device::new("NYC1".to_string(), 1, "Operator1".to_string()),
            Device::new("LON1".to_string(), 1, "Operator1".to_string()),
            Device::new("PAR1".to_string(), 1, "Operator2".to_string()),
        ];

        let demands = vec![Demand::new(
            "NYC".to_string(),
            "PAR".to_string(),
            1,
            50.0,
            1.0,
            1,
            false,
        )];

        let public_links = vec![PublicLink::new("NYC".to_string(), "PAR".to_string(), 100.0)];

        let shapley = Shapley::new(private_links, devices, demands, public_links, 1.0, 5.0, 1.0);

        let result = shapley.compute();
        assert!(result.is_ok(), "Error in test: {result:?}");

        let values = result.expect("Shapley computation should succeed in tests");
        assert_eq!(values.len(), 2); // Two operators
    }

    #[test]
    fn test_compute_expected_values_simple() {
        // Test with 2 operators, uptime = 0.9
        let n_ops = 2;
        let uptime = 0.9; // 0.9

        // svalue for coalitions: {}, {B}, {A}, {A,B}
        let svalue = vec![Some(100.0), Some(120.0), Some(150.0), Some(200.0)];

        let evalue = compute_expected_values(&svalue, n_ops, uptime)
            .expect("Expected value computation should succeed in tests");

        // These expected values are derived from running the reference Python code
        // with the same inputs.
        let expected_evalue = vec![100.0, 118.0, 145.0, 187.3];

        for (i, (val, exp)) in evalue.iter().zip(expected_evalue).enumerate() {
            assert!(
                (val - exp).abs() < 1e-9,
                "Mismatch at index {i}: got {val}, expected {exp}",
            );
        }
    }
}
