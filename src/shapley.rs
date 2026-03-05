use crate::{
    consolidation::{consolidate_demand, consolidate_links},
    error::{Result, ShapleyError},
    lp_builder::LpBuilderInput,
    solver::{SolveStatus, create_coalition_solver},
    types::{Demands, Devices, PrivateLinks, PublicLinks},
    utils::factorial,
    validation::check_inputs,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::{Display, Formatter},
};

#[cfg(feature = "serde")]
use {
    serde::{Deserialize, Serialize},
    tabled::Tabled,
};

/// Sentinel bit for operators that are always included in every coalition
/// (Public, Private, empty). Set in bit 31 so it never collides with
/// operator index bits 0..19.
const ALWAYS_BIT: u32 = 1 << 31;

// For clarity
pub type Operator = String;

// Since shapley value is per operator, we just use a hashmap
pub type ShapleyOutput = BTreeMap<Operator, ShapleyValue>;

/// Input parameters for Shapley computation
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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

        // Pre-compute operator bitmasks (once, before the parallel loop)
        let op_index: HashMap<&str, u8> = operators
            .iter()
            .enumerate()
            .map(|(i, op)| (op.as_str(), i as u8))
            .collect();

        let operator_mask = |op: &str| -> u32 {
            if op == "Public" || op == "Private" || op.is_empty() {
                ALWAYS_BIT
            } else if let Some(&idx) = op_index.get(op) {
                1u32 << idx
            } else {
                0
            }
        };

        let col_op1_mask: Vec<u32> = primitives
            .col_op1
            .iter()
            .map(|s| operator_mask(s))
            .collect();
        let col_op2_mask: Vec<u32> = primitives
            .col_op2
            .iter()
            .map(|s| operator_mask(s))
            .collect();
        let row_op1_mask: Vec<u32> = primitives
            .row_op1
            .iter()
            .map(|s| operator_mask(s))
            .collect();
        let row_op2_mask: Vec<u32> = primitives
            .row_op2
            .iter()
            .map(|s| operator_mask(s))
            .collect();

        let n_coalitions = 1 << n_operators;

        // Solve LP for each coalition
        let coalition_values: Vec<Option<f64>> = (0..n_coalitions)
            .into_par_iter()
            .map(|coalition_idx| {
                let coalition_mask = (coalition_idx as u32) | ALWAYS_BIT;

                match create_coalition_solver(
                    &primitives,
                    coalition_mask,
                    &col_op1_mask,
                    &col_op2_mask,
                    &row_op1_mask,
                    &row_op2_mask,
                ) {
                    Ok(solver) => {
                        // Solve and return the optimal value
                        match solver.solve() {
                            Ok(solution) => {
                                if matches!(solution.status, SolveStatus::Solved) {
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
        let shapley_values = compute_shapley_values(&expected_values, n_operators);

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

/// Compute expected values considering operator uptime.
///
/// For each coalition S, computes:
///   evalue[S] = Σ_{T⊆S} uptime^|T| × (1-uptime)^(|S\T|) × svalue[T]
///
/// Uses Gosper's subset iteration (`t = (t-1) & s`) for O(3^n) total work
/// instead of O(4^n) dense matrix operations.
fn compute_expected_values(
    svalue: &[Option<f64>],
    n_operators: usize,
    operator_uptime: f64,
) -> Result<Vec<f64>> {
    let n_coal = 1 << n_operators;
    let downtime = 1.0 - operator_uptime;

    let svalue_vec: Vec<f64> = svalue
        .iter()
        .map(|&v| v.unwrap_or(f64::NEG_INFINITY))
        .collect();

    let mut evalue = vec![0.0; n_coal];

    for (s, ev) in evalue.iter_mut().enumerate() {
        let s_size = (s as u32).count_ones() as i32;
        let mut sum = 0.0;

        // Iterate over all subsets t of s (including empty set)
        let mut t = s;
        loop {
            let val = svalue_vec[t];
            if val.is_finite() {
                let t_size = (t as u32).count_ones() as i32;
                let prob = operator_uptime.powi(t_size) * downtime.powi(s_size - t_size);
                sum += prob * val;
            }
            if t == 0 {
                break;
            }
            t = (t - 1) & s;
        }

        *ev = sum;
    }

    // Preserve empty coalition value
    if let Some(v) = svalue[0]
        && v.is_finite()
    {
        evalue[0] = v;
    }

    Ok(evalue)
}

/// Compute Shapley values from coalition values
fn compute_shapley_values(coalition_values: &[f64], n_operators: usize) -> Vec<f64> {
    let mut shapley_values = vec![0.0; n_operators];
    let fact_n = factorial(n_operators);

    for (k, sv) in shapley_values.iter_mut().enumerate() {
        let mut value = 0.0;

        // Find coalitions with this operator
        for (coalition_idx, &with_value) in coalition_values.iter().enumerate() {
            if (coalition_idx >> k) & 1 == 1 {
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

        *sv = value;
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
