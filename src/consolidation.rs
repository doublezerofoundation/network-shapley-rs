use crate::{
    error::{Result, ShapleyError},
    types::{ConsolidatedDemand, ConsolidatedLink, Demands, Devices, PrivateLinks, PublicLinks},
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Consolidate demand table for LP construction
pub(crate) fn consolidate_demand(
    demands: &Demands,
    demand_multiplier: f64,
) -> Result<Vec<ConsolidatedDemand>> {
    let mut consolidated = Vec::new();

    // Group by type, end, and rounded priority to merge duplicates
    let mut groups: BTreeMap<(u32, String, i64), Vec<usize>> = BTreeMap::new();

    for (idx, demand) in demands.iter().enumerate() {
        let priority_rounded = (demand.priority * 100.0).round() as i64;
        let key = (demand.kind, demand.end.clone(), priority_rounded);
        groups.entry(key).or_default().push(idx);
    }

    // Process groups - merge demands with same type, end, and priority
    let mut indices_to_skip = HashSet::new();

    for ((_kind, _end, _priority), indices) in groups.iter() {
        if indices.len() > 1 {
            // Aggregate receivers, use first demand for other fields
            let first_idx = indices[0];
            let first = &demands[first_idx];

            let total_receivers: u32 = indices.iter().map(|&i| demands[i].receivers).sum();

            let avg_priority =
                indices.iter().map(|&i| demands[i].priority).sum::<f64>() / indices.len() as f64;

            consolidated.push(ConsolidatedDemand {
                start: first.start.clone(),
                end: first.end.clone(),
                receivers: total_receivers,
                traffic: first.traffic,
                priority: avg_priority,
                kind: first.kind,
                multicast: first.multicast,
                original: first.kind,
            });

            // Mark all indices as processed
            for &idx in indices {
                indices_to_skip.insert(idx);
            }
        }
    }

    // Add non-aggregated demands
    for (idx, demand) in demands.iter().enumerate() {
        if !indices_to_skip.contains(&idx) {
            consolidated.push(ConsolidatedDemand {
                start: demand.start.clone(),
                end: demand.end.clone(),
                receivers: demand.receivers,
                traffic: demand.traffic,
                priority: demand.priority,
                kind: demand.kind,
                multicast: demand.multicast,
                original: demand.kind,
            });
        }
    }

    // Adjust types for unicast with different priorities
    let mut max_type = consolidated.iter().map(|d| d.kind).max().unwrap_or(0);

    // Group unicast demands by type
    let mut unicast_by_type: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (idx, demand) in consolidated.iter().enumerate() {
        if !demand.multicast {
            unicast_by_type.entry(demand.kind).or_default().push(idx);
        }
    }

    // Split unicast types by rounded priority
    for (type_id, indices) in unicast_by_type {
        let mut priority_groups: BTreeMap<i64, Vec<usize>> = BTreeMap::new();

        for &idx in &indices {
            let priority_rounded = (consolidated[idx].priority * 100.0).round() as i64;
            priority_groups
                .entry(priority_rounded)
                .or_default()
                .push(idx);
        }

        if priority_groups.len() > 1 {
            let sorted_priorities: Vec<_> = {
                let mut keys: Vec<_> = priority_groups.keys().cloned().collect();
                keys.sort();
                keys
            };

            for (i, &priority) in sorted_priorities.iter().enumerate() {
                let new_type = if i == 0 { type_id } else { max_type + i as u32 };
                for &idx in &priority_groups[&priority] {
                    consolidated[idx].kind = new_type;
                }
            }
            max_type += (sorted_priorities.len() - 1) as u32;
        }
    }

    // Split multicast into unique types for each row
    let multicast_indices: Vec<usize> = consolidated
        .iter()
        .enumerate()
        .filter(|(_, d)| d.multicast)
        .map(|(i, _)| i)
        .collect();

    // Group multicast by type
    let mut multicast_by_type: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for &idx in &multicast_indices {
        multicast_by_type
            .entry(consolidated[idx].kind)
            .or_default()
            .push(idx);
    }

    // Assign unique types to multicast demands
    for (type_id, indices) in multicast_by_type {
        if indices.len() > 1 {
            for (i, &idx) in indices.iter().enumerate() {
                if i == 0 {
                    consolidated[idx].kind = type_id;
                } else {
                    max_type += 1;
                    consolidated[idx].kind = max_type;
                }
            }
        }
    }

    // Apply demand multiplier
    for demand in &mut consolidated {
        demand.traffic *= demand_multiplier;
    }

    Ok(consolidated)
}

/// Consolidate links for LP construction
pub(crate) fn consolidate_links(
    private_links: &PrivateLinks,
    devices: &Devices,
    demands: &[ConsolidatedDemand],
    public_links: &PublicLinks,
    contiguity_bonus: f64,
) -> Result<Vec<ConsolidatedLink>> {
    let mut consolidated = Vec::new();

    // Create device to operator mapping
    let device_to_operator: HashMap<&str, &str> = devices
        .iter()
        .map(|d| (d.device.as_str(), d.operator.as_str()))
        .collect();

    // Process private links - create bidirectional flows
    let mut max_shared = 0u32;

    // First pass: find max shared ID and assign shared IDs to links without them
    let mut private_links_with_shared = Vec::new();
    for link in private_links {
        if let Some(shared) = link.shared {
            max_shared = max_shared.max(shared);
            private_links_with_shared.push((link, shared));
        } else {
            // Assign new shared ID to links without one
            private_links_with_shared.push((link, 0)); // Will be assigned later
        }
    }

    let mut next_shared = max_shared + 1;
    for pvt_link in &mut private_links_with_shared {
        if pvt_link.1 == 0 {
            pvt_link.1 = next_shared;
            next_shared += 1
        }
    }

    // Add forward direction
    for (link, shared_id) in &private_links_with_shared {
        let operator1 = device_to_operator
            .get(link.device1.as_str())
            .unwrap_or(&"Unknown");
        let operator2 = device_to_operator
            .get(link.device2.as_str())
            .unwrap_or(&"Unknown");

        // Adjust bandwidth for uptime
        let adjusted_bandwidth = link.bandwidth * link.uptime;

        consolidated.push(ConsolidatedLink {
            device1: link.device1.clone(),
            device2: link.device2.clone(),
            latency: link.latency,
            bandwidth: adjusted_bandwidth,
            operator1: operator1.to_string(),
            operator2: operator2.to_string(),
            shared: *shared_id,
            link_type: 0, // Available to all traffic types
        });
    }

    // Update max_shared to include newly assigned IDs
    max_shared = next_shared - 1;

    // Add reverse direction with adjusted shared IDs
    let forward_count = consolidated.len();
    for i in 0..forward_count {
        let link = consolidated[i].clone();
        consolidated.push(ConsolidatedLink {
            device1: link.device2,
            device2: link.device1,
            latency: link.latency,
            bandwidth: link.bandwidth,
            operator1: link.operator2,
            operator2: link.operator1,
            shared: link.shared + max_shared,
            link_type: 0,
        });
    }

    // Update max_shared after reverse links
    max_shared *= 2;

    // Create device shared ID mapping (matching Python's approach)
    // Python duplicates devices with Outbound flag and assigns shared IDs to all
    let mut device_shared_map: HashMap<(String, bool), u32> = HashMap::new();
    let mut device_shared_id = max_shared + 1;

    // First pass: inbound devices (Outbound = False)
    for device in devices {
        device_shared_map.insert((device.device.clone(), false), device_shared_id);
        device_shared_id += 1;
    }

    // Second pass: outbound devices (Outbound = True)
    for device in devices {
        device_shared_map.insert((device.device.clone(), true), device_shared_id);
        device_shared_id += 1;
    }

    // Note: max_shared is no longer used after this point

    // Store public links to add at the end (matching Python order)
    let mut public_links_consolidated = Vec::new();

    // Process public links - create bidirectional flows
    for link in public_links {
        // Forward direction
        public_links_consolidated.push(ConsolidatedLink {
            device1: format!("{}00", link.city1),
            device2: format!("{}00", link.city2),
            latency: link.latency,
            bandwidth: 0.0, // Public links have no bandwidth limit
            operator1: "Public".to_string(),
            operator2: "Public".to_string(),
            shared: 0,
            link_type: 0,
        });

        // Reverse direction
        public_links_consolidated.push(ConsolidatedLink {
            device1: format!("{}00", link.city2),
            device2: format!("{}00", link.city1),
            latency: link.latency,
            bandwidth: 0.0,
            operator1: "Public".to_string(),
            operator2: "Public".to_string(),
            shared: 0,
            link_type: 0,
        });
    }

    // Add on-ramps and off-ramps for demand endpoints
    let unique_types: HashSet<u32> = demands.iter().map(|d| d.kind).collect();
    let mut unique_types_vec: Vec<u32> = unique_types.into_iter().collect();
    unique_types_vec.sort();

    for type_id in unique_types_vec {
        let type_demands: Vec<&ConsolidatedDemand> =
            demands.iter().filter(|d| d.kind == type_id).collect();

        if let Some(first_demand) = type_demands.first() {
            let src = &first_demand.start;
            let destinations: HashSet<&str> = type_demands.iter().map(|d| d.end.as_str()).collect();
            let mut destinations_vec: Vec<&str> = destinations.iter().copied().collect();
            destinations_vec.sort();

            // Public on-ramp for source
            public_links_consolidated.push(ConsolidatedLink {
                device1: src.clone(),
                device2: format!("{src}00"),
                latency: 0.0,
                bandwidth: 0.0,
                operator1: "Public".to_string(),
                operator2: "Public".to_string(),
                shared: 0,
                link_type: type_id,
            });

            // Public off-ramps for destinations
            for dst in &destinations_vec {
                public_links_consolidated.push(ConsolidatedLink {
                    device1: format!("{dst}00"),
                    device2: dst.to_string(),
                    latency: 0.0,
                    bandwidth: 0.0,
                    operator1: "Public".to_string(),
                    operator2: "Public".to_string(),
                    shared: 0,
                    link_type: type_id,
                });
            }

            // Private on-ramps for source city devices (inbound)
            for device in devices {
                if device.device.starts_with(src) && !device.device.ends_with("00") {
                    // Use device's shared ID from mapping (inbound = false)
                    let shared_id = device_shared_map
                        .get(&(device.device.clone(), false))
                        .copied()
                        .ok_or_else(|| ShapleyError::MissingDevice(device.device.clone()))?;
                    consolidated.push(ConsolidatedLink {
                        device1: src.clone(),
                        device2: device.device.clone(),
                        latency: 0.0,
                        bandwidth: device.edge as f64,
                        operator1: device.operator.clone(),
                        operator2: device.operator.clone(),
                        shared: shared_id,
                        link_type: type_id,
                    });
                }
            }

            // Private off-ramps for destination city devices (outbound)
            for dst in &destinations_vec {
                for device in devices {
                    if device.device.starts_with(dst) && !device.device.ends_with("00") {
                        // Use device's shared ID from mapping (outbound = true)
                        let shared_id = device_shared_map
                            .get(&(device.device.clone(), true))
                            .copied()
                            .ok_or_else(|| ShapleyError::MissingDevice(device.device.clone()))?;
                        let new_link = ConsolidatedLink {
                            device1: device.device.clone(),
                            device2: dst.to_string(),
                            latency: 0.0,
                            bandwidth: device.edge as f64,
                            operator1: device.operator.clone(),
                            operator2: device.operator.clone(),
                            shared: shared_id,
                            link_type: type_id,
                        };
                        consolidated.push(new_link);
                    }
                }
            }
        }
    }

    // Add crossover points between private and public networks
    let private_cities: HashSet<&str> = devices.iter().map(|d| &d.device[..3]).collect();

    let public_cities: HashSet<&str> = public_links
        .iter()
        .flat_map(|l| [l.city1.as_str(), l.city2.as_str()])
        .collect();

    let mut crossover_cities: Vec<&str> = private_cities
        .intersection(&public_cities)
        .cloned()
        .collect();
    crossover_cities.sort();

    for city in crossover_cities {
        for device in devices {
            if device.device.starts_with(city) && !device.device.ends_with("00") {
                // Device to public (outbound)
                let outbound_shared_id = device_shared_map
                    .get(&(device.device.clone(), true))
                    .copied()
                    .ok_or_else(|| ShapleyError::MissingDevice(device.device.clone()))?;
                consolidated.push(ConsolidatedLink {
                    device1: device.device.clone(),
                    device2: format!("{city}00"),
                    latency: contiguity_bonus,
                    bandwidth: device.edge as f64,
                    operator1: device.operator.clone(),
                    operator2: device.operator.clone(),
                    shared: outbound_shared_id,
                    link_type: 0,
                });

                // Public to device (inbound)
                let inbound_shared_id = device_shared_map
                    .get(&(device.device.clone(), false))
                    .copied()
                    .ok_or_else(|| ShapleyError::MissingDevice(device.device.clone()))?;
                consolidated.push(ConsolidatedLink {
                    device1: format!("{city}00"),
                    device2: device.device.clone(),
                    latency: contiguity_bonus,
                    bandwidth: device.edge as f64,
                    operator1: device.operator.clone(),
                    operator2: device.operator.clone(),
                    shared: inbound_shared_id,
                    link_type: 0,
                });
            }
        }
    }

    // Add public links at the end to match Python ordering
    consolidated.extend(public_links_consolidated);

    // Compact shared IDs to consecutive integers
    let unique_shared: Vec<u32> = {
        let shared_ids: HashSet<u32> = consolidated
            .iter()
            .filter(|l| l.shared > 0)
            .map(|l| l.shared)
            .collect();

        let mut sorted: Vec<u32> = shared_ids.into_iter().collect();
        sorted.sort();
        sorted
    };

    if !unique_shared.is_empty() {
        let shared_map: HashMap<u32, u32> = unique_shared
            .into_iter()
            .enumerate()
            .map(|(i, old)| (old, (i + 1) as u32))
            .collect();

        for link in &mut consolidated {
            if link.shared > 0 {
                link.shared = *shared_map.get(&link.shared).unwrap_or(&link.shared);
            }
        }
    }

    Ok(consolidated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Demand;

    #[test]
    fn test_consolidate_demand_basic() {
        let demands = vec![
            Demand::new("A".to_string(), "B".to_string(), 1, 1.0, 1.0, 1, false),
            Demand::new("A".to_string(), "C".to_string(), 2, 1.0, 1.0, 1, false),
        ];

        let result = consolidate_demand(&demands, 2.0)
            .expect("Demand consolidation should succeed in tests");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].traffic, 2.0); // Multiplied by 2
        assert_eq!(result[1].traffic, 2.0);
    }

    #[test]
    fn test_consolidate_demand_with_multicast() {
        let demands = vec![
            // Multicast demands of the same type
            Demand::new("A".to_string(), "B".to_string(), 1, 1.0, 1.0, 1, true),
            Demand::new("A".to_string(), "C".to_string(), 1, 1.0, 1.0, 1, true),
            Demand::new("A".to_string(), "D".to_string(), 1, 1.0, 1.0, 1, true),
            // Another multicast group
            Demand::new("X".to_string(), "Y".to_string(), 1, 2.0, 1.0, 2, true),
            Demand::new("X".to_string(), "Z".to_string(), 1, 2.0, 1.0, 2, true),
        ];

        let result = consolidate_demand(&demands, 1.0)
            .expect("Multicast demand consolidation should succeed");

        // Check that multicast demands are properly consolidated
        // The first 3 should have the same type, last 2 should have different types
        let multicast_results: Vec<_> = result.iter().filter(|d| d.multicast).collect();
        assert_eq!(multicast_results.len(), 5);

        // Check that multicast demands with same original type get unique types
        let types: Vec<_> = multicast_results.iter().map(|d| d.kind).collect();
        let unique_types: std::collections::HashSet<_> = types.iter().cloned().collect();

        // Should have assigned unique types to multicast demands
        assert!(unique_types.len() >= 2);
    }

    #[test]
    fn test_consolidate_demand_empty() {
        let demands = vec![];
        let result = consolidate_demand(&demands, 1.0).expect("Empty demands should succeed");

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_mixed_multicast_and_regular() {
        let demands = vec![
            // Regular demands
            Demand::new("A".to_string(), "B".to_string(), 1, 1.0, 1.0, 1, false),
            Demand::new("C".to_string(), "D".to_string(), 2, 2.0, 1.0, 2, false),
            // Multicast demand
            Demand::new("E".to_string(), "F".to_string(), 3, 3.0, 1.0, 3, true),
            Demand::new("E".to_string(), "G".to_string(), 3, 3.0, 1.0, 3, true),
        ];

        let result = consolidate_demand(&demands, 1.0).expect("Mixed demands should succeed");

        assert_eq!(result.len(), 4);

        // Check that multicast demands have unique types assigned
        let multicast_types: Vec<_> = result
            .iter()
            .filter(|d| d.multicast)
            .map(|d| d.kind)
            .collect();

        let unique_multicast_types: std::collections::HashSet<_> =
            multicast_types.iter().cloned().collect();
        assert_eq!(unique_multicast_types.len(), multicast_types.len());
    }
}
