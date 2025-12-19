//! Per-link Shapley value estimation for a specific operator.
//!
//! This module provides functionality to compute Shapley values for individual links
//! owned by a specific operator, rather than per-operator values. This enables
//! attribution of value contribution to each physical link, supporting capacity
//! planning and link valuation use cases.
//!
//! # Algorithm
//!
//! The implementation uses a `retag_links` transformation that:
//! 1. Collapses all non-focus operators into "Others"
//! 2. Assigns unique numeric IDs to each bidirectional link pair belonging to the focus operator
//! 3. Tags edge connections (on-ramps/off-ramps) as "Private"
//! 4. Runs standard Shapley computation where each "operator" is actually a single link
//! 5. Maps results back to per-link output format
//!
//! # Constraints
//!
//! - Maximum 20 links for the focus operator (2^20 coalitions limit)
//! - No shared-group links allowed for the focus operator
//! - No duplicate links (same device pair with same latency/bandwidth)
//! - Operator uptime is fixed at 1.0

use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::LazyLock;

use regex::Regex;

use crate::consolidation::{consolidate_demand, consolidate_links};
use crate::error::{Result, ShapleyError};
use crate::lp_builder::LpBuilderInput;
use crate::solver::create_coalition_solver;
use crate::types::{ConsolidatedLink, Demands, Devices, PrivateLinks, PublicLinks};
use crate::utils::{factorial, generate_bitmap};
use crate::validation::check_inputs;

use clarabel::solver::SolverStatus;
use rayon::prelude::*;

#[cfg(feature = "serde")]
use {
    serde::{Deserialize, Serialize},
    tabled::Tabled,
};

#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSerialize};

/// Device pattern: 3-letter city code followed by 1-2 digit integer (not 00)
/// Matches: NYC1, LON12, FRA01 (but not NYC00, NYC, or ABC)
static DEVICE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z]{3}(([1-9][0-9]*)|(0[1-9]))$").unwrap());

/// Input parameters for per-link Shapley value estimation.
///
/// Unlike [`crate::shapley::ShapleyInput`] which computes values per operator,
/// this computes values for each individual link owned by `operator_focus`.
#[derive(Debug, Clone)]
pub struct LinkEstimateInput {
    /// Private links in the network
    pub private_links: PrivateLinks,
    /// Device definitions with operator assignments
    pub devices: Devices,
    /// Network traffic demands
    pub demands: Demands,
    /// Public internet links
    pub public_links: PublicLinks,
    /// The operator whose links we're evaluating
    pub operator_focus: String,
    /// Extra latency penalty for mixing public/private links
    pub contiguity_bonus: f64,
    /// Multiplier to scale demand traffic
    pub demand_multiplier: f64,
}

/// Value attribution for a single link.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize, Tabled))]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct LinkValue {
    /// First device endpoint (lexicographically smaller)
    pub device1: String,
    /// Second device endpoint (lexicographically larger)
    pub device2: String,
    /// Link bandwidth capacity
    pub bandwidth: f64,
    /// Link latency
    pub latency: f64,
    /// Shapley value for this link
    pub value: f64,
    /// Proportion of total value (0.0 to 1.0)
    pub percent: f64,
}

impl Display for LinkValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-{}: value={:.6}, percent={:.4}%",
            self.device1,
            self.device2,
            self.value,
            self.percent * 100.0
        )
    }
}

/// Output of per-link Shapley value estimation.
pub type LinkEstimateOutput = Vec<LinkValue>;

impl LinkEstimateInput {
    /// Compute Shapley values for each link owned by the focus operator.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The focus operator has more than 20 links
    /// - The focus operator has shared-group links
    /// - There are duplicate links
    /// - The focus operator doesn't exist in the devices list
    /// - Standard validation fails (see [`crate::validation::check_inputs`])
    pub fn compute(&self) -> Result<LinkEstimateOutput> {
        // Force operator_uptime to 1.0 for link estimation
        let operator_uptime = 1.0;

        // Run standard validation first
        check_inputs(
            &self.private_links,
            &self.devices,
            &self.demands,
            &self.public_links,
            operator_uptime,
        )?;

        // Run link-specific validation
        self.validate_link_estimate()?;

        // Consolidate demands and links
        let full_demand = consolidate_demand(&self.demands, self.demand_multiplier)?;
        let mut full_map = consolidate_links(
            &self.private_links,
            &self.devices,
            &full_demand,
            &self.public_links,
            self.contiguity_bonus,
        )?;

        // Apply retag transformation
        retag_links(&mut full_map, &self.operator_focus);

        // Build LP primitives
        let primitives = LpBuilderInput::new(&full_map, &full_demand).build()?;

        // Enumerate operators (now each is a link ID like "1", "2", "3", ...)
        let mut operators: Vec<String> = full_map
            .iter()
            .flat_map(|l| [l.operator1.clone(), l.operator2.clone()])
            .filter(|op| op != "Private" && op != "Public" && op != "Others")
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        operators.sort_by(|a, b| {
            // Sort numerically if possible
            match (a.parse::<u32>(), b.parse::<u32>()) {
                (Ok(na), Ok(nb)) => na.cmp(&nb),
                _ => a.cmp(b),
            }
        });

        let n_operators = operators.len();
        if n_operators == 0 {
            return Ok(Vec::new());
        }

        // Generate coalition bitmap
        let bitmap = generate_bitmap(n_operators);
        let n_coalitions = 1 << n_operators;

        // Solve LP for each coalition in parallel
        let coalition_values: Vec<Option<f64>> = (0..n_coalitions)
            .into_par_iter()
            .map(|coalition_idx| {
                let mut coalition_operators = Vec::new();
                for (op_idx, operator) in operators.iter().enumerate() {
                    if (coalition_idx & (1 << op_idx)) != 0 {
                        coalition_operators.push(operator.clone());
                    }
                }

                let coalition_bitmap = coalition_idx as u32;

                match create_coalition_solver(
                    &primitives,
                    coalition_bitmap,
                    &primitives.col_op1,
                    &coalition_operators,
                ) {
                    Ok(solver) => match solver.solve() {
                        Ok(solution) => {
                            if matches!(
                                solution.status,
                                SolverStatus::Solved | SolverStatus::AlmostSolved
                            ) {
                                Some(-solution.objective_value)
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    },
                    Err(_) => None,
                }
            })
            .collect();

        // With operator_uptime = 1.0, expected values = coalition values
        let expected_values: Vec<f64> = coalition_values
            .iter()
            .map(|&v| v.unwrap_or(f64::NEG_INFINITY))
            .collect();

        // Compute Shapley values
        let shapley_values =
            compute_shapley_values(&expected_values, &bitmap, n_operators, &operators);

        // Build operator -> shapley value map
        let shapley_map: BTreeMap<String, f64> = operators
            .iter()
            .cloned()
            .zip(shapley_values.iter().copied())
            .collect();

        // Map back to link output format
        self.build_output(&full_map, &shapley_map)
    }

    /// Validate inputs specific to link estimation.
    fn validate_link_estimate(&self) -> Result<()> {
        // Check operator exists
        let operator_exists = self
            .devices
            .iter()
            .any(|d| d.operator == self.operator_focus);

        if !operator_exists {
            return Err(ShapleyError::OperatorNotFound {
                operator: self.operator_focus.clone(),
            });
        }

        // Build device -> operator map
        let device_to_op: std::collections::HashMap<&str, &str> = self
            .devices
            .iter()
            .map(|d| (d.device.as_str(), d.operator.as_str()))
            .collect();

        // Find links belonging to focus operator
        let focus_links: Vec<_> = self
            .private_links
            .iter()
            .filter(|link| {
                let op1 = device_to_op.get(link.device1.as_str()).copied();
                let op2 = device_to_op.get(link.device2.as_str()).copied();
                op1 == Some(self.operator_focus.as_str())
                    || op2 == Some(self.operator_focus.as_str())
            })
            .collect();

        // Check max links (unique bidirectional pairs)
        // Use to_bits() for f64 to make them hashable
        let unique_pairs: HashSet<_> = focus_links
            .iter()
            .map(|link| {
                let (d1, d2) = if link.device1 < link.device2 {
                    (&link.device1, &link.device2)
                } else {
                    (&link.device2, &link.device1)
                };
                (
                    d1.clone(),
                    d2.clone(),
                    link.bandwidth.to_bits(),
                    link.latency.to_bits(),
                )
            })
            .collect();

        const MAX_LINKS: usize = 20;
        if unique_pairs.len() > MAX_LINKS {
            return Err(ShapleyError::TooManyLinks {
                count: unique_pairs.len(),
                limit: MAX_LINKS,
            });
        }

        // Check no shared-group links for focus operator
        for link in &focus_links {
            if link.shared.is_some() {
                return Err(ShapleyError::SharedGroupNotAllowed {
                    operator: self.operator_focus.clone(),
                });
            }
        }

        // Check no duplicate links
        let mut seen_pairs: HashSet<(String, String, u64, u64)> = HashSet::new();
        for link in &self.private_links {
            let (d1, d2) = if link.device1 < link.device2 {
                (link.device1.clone(), link.device2.clone())
            } else {
                (link.device2.clone(), link.device1.clone())
            };
            // Use bits for f64 comparison
            let bw_bits = link.bandwidth.to_bits();
            let lat_bits = link.latency.to_bits();
            let key = (d1.clone(), d2.clone(), bw_bits, lat_bits);

            if !seen_pairs.insert(key) {
                return Err(ShapleyError::DuplicateLink {
                    device1: d1,
                    device2: d2,
                });
            }
        }

        Ok(())
    }

    /// Build output from retagged links and shapley values.
    fn build_output(
        &self,
        full_map: &[ConsolidatedLink],
        shapley_map: &BTreeMap<String, f64>,
    ) -> Result<LinkEstimateOutput> {
        let drop_tags: HashSet<&str> = ["Public", "Private", "Others"].into_iter().collect();

        // Filter to links where at least one operator is a numeric ID (our focus links)
        let mut output: Vec<LinkValue> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();

        for link in full_map {
            // Skip if both operators are in drop_tags
            if drop_tags.contains(link.operator1.as_str())
                && drop_tags.contains(link.operator2.as_str())
            {
                continue;
            }

            // Get canonical direction (device1 < device2)
            let (d1, d2) = if link.device1 < link.device2 {
                (link.device1.clone(), link.device2.clone())
            } else {
                (link.device2.clone(), link.device1.clone())
            };

            // Skip if already seen
            if seen.contains(&(d1.clone(), d2.clone())) {
                continue;
            }

            // Find the operator ID (the one that's not in drop_tags)
            let op = if !drop_tags.contains(link.operator1.as_str()) {
                &link.operator1
            } else {
                &link.operator2
            };

            // Get shapley value
            if let Some(&value) = shapley_map.get(op) {
                seen.insert((d1.clone(), d2.clone()));
                output.push(LinkValue {
                    device1: d1,
                    device2: d2,
                    bandwidth: link.bandwidth,
                    latency: link.latency,
                    value,
                    percent: 0.0, // Calculated below
                });
            }
        }

        // Calculate percentages
        let total_positive: f64 = output.iter().map(|lv| lv.value.max(0.0)).sum();
        if total_positive > 0.0 {
            for lv in &mut output {
                lv.percent = lv.value.max(0.0) / total_positive;
            }
        }

        // Sort by device names for consistent output
        output.sort_by(|a, b| (&a.device1, &a.device2).cmp(&(&b.device1, &b.device2)));

        Ok(output)
    }
}

/// Transform consolidated links for per-link Shapley computation.
///
/// This function modifies the operator tags so that:
/// - Non-focus operators become "Others"
/// - Each bidirectional link pair for the focus operator gets a unique numeric ID
/// - Edge connections (on-ramps/off-ramps) become "Private"
fn retag_links(links: &mut [ConsolidatedLink], operator_focus: &str) {
    // Step 1: Collapse non-focus operators to "Others"
    for link in links.iter_mut() {
        if link.operator1 != "Public" && link.operator1 != operator_focus {
            link.operator1 = "Others".to_string();
        }
        if link.operator2 != "Public" && link.operator2 != operator_focus {
            link.operator2 = "Others".to_string();
        }
    }

    // Step 2: Tag links needing processing
    let mut needs_processing: Vec<bool> = links
        .iter()
        .map(|l| l.operator1 == operator_focus || l.operator2 == operator_focus)
        .collect();

    // Step 3: Process links and assign IDs
    let mut counter = 0u32;

    while let Some(idx) = needs_processing.iter().position(|&p| p) {
        let d1 = links[idx].device1.clone();
        let d2 = links[idx].device2.clone();
        let bandwidth = links[idx].bandwidth;
        let latency = links[idx].latency;

        // Check if both devices match the pattern (real device links)
        if DEVICE_PATTERN.is_match(&d1) && DEVICE_PATTERN.is_match(&d2) {
            // Find symmetric link
            let sym_idx = links.iter().position(|l| {
                l.device1 == d2
                    && l.device2 == d1
                    && l.bandwidth == bandwidth
                    && l.latency == latency
            });

            counter += 1;
            let id_str = counter.to_string();

            // Retag based on which operator is the focus
            if links[idx].operator1 == operator_focus {
                links[idx].operator1 = id_str.clone();
            }
            if links[idx].operator2 == operator_focus {
                links[idx].operator2 = id_str.clone();
            }
            needs_processing[idx] = false;

            // Retag symmetric link if found
            if let Some(si) = sym_idx {
                if links[si].operator1 == operator_focus {
                    links[si].operator1 = id_str.clone();
                }
                if links[si].operator2 == operator_focus {
                    links[si].operator2 = id_str;
                }
                needs_processing[si] = false;
            }
        } else {
            // Edge connection - tag as Private
            links[idx].operator1 = "Private".to_string();
            links[idx].operator2 = "Private".to_string();
            needs_processing[idx] = false;
        }
    }
}

/// Compute Shapley values from coalition values (reused from shapley.rs logic).
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

        for coalition_idx in 0..coalition_values.len() {
            if bitmap[k][coalition_idx] == 1 {
                let with_value = coalition_values[coalition_idx];
                let without_idx = coalition_idx ^ (1 << k);
                let without_value = coalition_values[without_idx];
                let coalition_size = (coalition_idx as u32).count_ones() as usize;

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

    fn make_test_input() -> LinkEstimateInput {
        let private_links = vec![
            PrivateLink::new(
                "NYC1".to_string(),
                "LON1".to_string(),
                10.0,
                100.0,
                1.0,
                None,
            ),
            PrivateLink::new(
                "LON1".to_string(),
                "FRA1".to_string(),
                5.0,
                100.0,
                1.0,
                None,
            ),
        ];

        let devices = vec![
            Device::new("NYC1".to_string(), 100, "Alpha".to_string()),
            Device::new("LON1".to_string(), 100, "Alpha".to_string()),
            Device::new("FRA1".to_string(), 100, "Beta".to_string()),
        ];

        let public_links = vec![
            PublicLink::new("NYC".to_string(), "LON".to_string(), 50.0),
            PublicLink::new("LON".to_string(), "FRA".to_string(), 30.0),
        ];

        let demands = vec![Demand::new(
            "NYC".to_string(),
            "FRA".to_string(),
            1,
            10.0,
            1.0,
            1,
            false,
        )];

        LinkEstimateInput {
            private_links,
            devices,
            demands,
            public_links,
            operator_focus: "Alpha".to_string(),
            contiguity_bonus: 5.0,
            demand_multiplier: 1.0,
        }
    }

    #[test]
    fn test_device_pattern() {
        assert!(DEVICE_PATTERN.is_match("NYC1"));
        assert!(DEVICE_PATTERN.is_match("LON12"));
        assert!(DEVICE_PATTERN.is_match("FRA01"));
        assert!(!DEVICE_PATTERN.is_match("NYC00"));
        assert!(!DEVICE_PATTERN.is_match("NYC"));
        assert!(!DEVICE_PATTERN.is_match("AB1")); // Only 2 letters
        assert!(!DEVICE_PATTERN.is_match("ABCD1")); // 4 letters
    }

    #[test]
    fn test_validation_operator_not_found() {
        let mut input = make_test_input();
        input.operator_focus = "NonExistent".to_string();

        let result = input.compute();
        assert!(matches!(result, Err(ShapleyError::OperatorNotFound { .. })));
    }

    #[test]
    fn test_validation_shared_group_not_allowed() {
        let mut input = make_test_input();
        input.private_links[0].shared = Some(1);

        let result = input.compute();
        assert!(matches!(
            result,
            Err(ShapleyError::SharedGroupNotAllowed { .. })
        ));
    }

    #[test]
    fn test_link_estimate_basic() {
        let input = make_test_input();
        let result = input.compute();

        assert!(result.is_ok(), "Compute failed: {:?}", result);
        let output = result.unwrap();

        // Should have links for Alpha operator
        assert!(!output.is_empty());

        // All values should have percentages that sum to ~1.0
        let total_percent: f64 = output.iter().map(|lv| lv.percent).sum();
        if total_percent > 0.0 {
            assert!(
                (total_percent - 1.0).abs() < 1e-9,
                "Percentages should sum to 1.0"
            );
        }
    }

    #[test]
    fn test_retag_links_basic() {
        let mut links = vec![
            ConsolidatedLink {
                device1: "NYC1".to_string(),
                device2: "LON1".to_string(),
                latency: 10.0,
                bandwidth: 100.0,
                operator1: "Alpha".to_string(),
                operator2: "Alpha".to_string(),
                shared: 1,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "LON1".to_string(),
                device2: "NYC1".to_string(),
                latency: 10.0,
                bandwidth: 100.0,
                operator1: "Alpha".to_string(),
                operator2: "Alpha".to_string(),
                shared: 2,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "FRA1".to_string(),
                device2: "LON1".to_string(),
                latency: 5.0,
                bandwidth: 100.0,
                operator1: "Beta".to_string(),
                operator2: "Alpha".to_string(),
                shared: 3,
                link_type: 0,
            },
        ];

        retag_links(&mut links, "Alpha");

        // First two links should share the same numeric ID
        assert_eq!(links[0].operator1, "1");
        assert_eq!(links[0].operator2, "1");
        assert_eq!(links[1].operator1, "1");
        assert_eq!(links[1].operator2, "1");

        // Third link: Beta becomes Others, Alpha gets ID
        assert_eq!(links[2].operator1, "Others");
        assert_eq!(links[2].operator2, "2");
    }
}
