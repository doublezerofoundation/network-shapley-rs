use crate::{
    LinkBuilder,
    types::{Demand, DemandMatrix, Link, Result},
};
use rayon::prelude::*;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

/// Prepare private links by handling operators, duplicating for bidirectionality, and adjusting bandwidth
pub fn prepare_private_links(links: &mut [Link]) -> Result<Vec<Link>> {
    // Fill missing secondary operators
    for link in links.iter_mut() {
        if link.operator2 == "0" || link.operator2.is_empty() {
            link.operator2 = link.operator1.clone();
        }
    }

    // Get max shared value before duplication
    let max_shared = links
        .iter()
        .filter_map(|l| match l.shared > 0 {
            false => None,
            true => Some(l.shared),
        })
        .max()
        .unwrap_or(0);

    // Create reverse links for bidirectionality
    let mut all_links = Vec::with_capacity(links.len() * 2);

    for link in links.iter() {
        // Original link
        let mut forward = link.clone();
        forward.bandwidth *= forward.uptime;
        forward.link_type = 0; // Available to all traffic types
        all_links.push(forward);

        // Reverse link
        let mut reverse = link.clone();
        std::mem::swap(&mut reverse.start, &mut reverse.end);
        reverse.bandwidth *= reverse.uptime;
        reverse.link_type = 0;

        // Adjust shared ID for reverse link
        if reverse.shared > 0 {
            reverse.shared += max_shared;
        }

        all_links.push(reverse);
    }

    // Compact shared IDs
    compact_shared_ids(&mut all_links);

    Ok(all_links)
}

/// Prepare public links by duplicating for bidirectionality
pub fn prepare_public_links(links: &[Link]) -> Result<Vec<Link>> {
    let mut all_links = Vec::with_capacity(links.len() * 2);

    for link in links {
        // Forward link
        let mut forward = link.clone();
        forward.link_type = 0; // Available to all traffic types
        all_links.push(forward);

        // Reverse link
        let mut reverse = link.clone();
        std::mem::swap(&mut reverse.start, &mut reverse.end);
        reverse.link_type = 0;
        all_links.push(reverse);
    }

    Ok(all_links)
}

/// Generate helper links and direct public paths per traffic type
pub fn generate_helper_links(public_links: &[Link], demand: &DemandMatrix) -> Result<Vec<Link>> {
    let traffic_types = demand.unique_types();

    // Process traffic types in parallel
    let helper_links: Vec<Vec<Link>> = traffic_types
        .par_iter()
        .filter_map(|&traffic_type| {
            // Get source city and destination cities for this traffic type
            let type_demands: Vec<&Demand> = demand
                .demands
                .iter()
                .filter(|d| d.demand_type == traffic_type)
                .collect();

            if type_demands.is_empty() {
                return None;
            }

            let src_city = &type_demands[0].start;
            let dst_cities: HashSet<&String> = type_demands.iter().map(|d| &d.end).collect();

            let mut type_helpers = Vec::new();

            // Find direct city-to-city public paths
            if let Ok(direct_paths) =
                find_direct_paths(public_links, src_city, &dst_cities, traffic_type)
            {
                type_helpers.extend(direct_paths);
            }

            // Create zero-cost helper links to/from switches
            if let Ok(src_helpers) = create_source_helpers(public_links, src_city, traffic_type) {
                type_helpers.extend(src_helpers);
            }

            if let Ok(dst_helpers) =
                create_destination_helpers(public_links, &dst_cities, traffic_type)
            {
                type_helpers.extend(dst_helpers);
            }

            Some(type_helpers)
        })
        .collect();

    // Flatten the results
    Ok(helper_links.into_iter().flatten().collect())
}

/// Merge all link components into final consolidated map
pub fn merge_link_components(
    private_links: Vec<Link>,
    public_links: Vec<Link>,
    helper_links: Vec<Link>,
    hybrid_penalty: Decimal,
) -> Result<Vec<Link>> {
    let mut all_links = Vec::new();

    // Add private links first
    all_links.extend(private_links);

    // Add public links with hybrid penalty
    for mut link in public_links {
        link.cost += hybrid_penalty;
        link.bandwidth = Decimal::ZERO;
        link.operator1 = "0".to_string();
        link.operator2 = "0".to_string();
        link.uptime = Decimal::ONE;
        link.shared = 0;
        all_links.push(link);
    }

    // Add helper links
    for mut link in helper_links {
        // Don't apply hybrid penalty to helper links - Python doesn't do this
        link.bandwidth = Decimal::ZERO;
        link.operator1 = "0".to_string();
        link.operator2 = "0".to_string();
        link.uptime = Decimal::ONE;
        link.shared = 0;
        all_links.push(link);
    }

    Ok(all_links)
}

/// Compact shared IDs to remove gaps in the sequence
fn compact_shared_ids(links: &mut [Link]) {
    // Get max shared value
    let max_shared = links
        .iter()
        .filter_map(|l| match l.shared > 0 {
            false => None,
            true => Some(l.shared),
        })
        .max()
        .unwrap_or(0);

    // For private links (those with operators), assign new shared IDs to previously unassigned links
    // This ensures each private link has bandwidth constraints
    let mut next_id = max_shared + 1;
    for link in links.iter_mut() {
        if link.shared == 0 && link.operator1 != "0" {
            link.shared = next_id;
            next_id += 1;
        }
    }

    // Create mapping from old to new IDs
    let unique_shared: Vec<usize> = links
        .iter()
        .filter_map(|l| match l.shared > 0 {
            false => None,
            true => Some(l.shared),
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if unique_shared.is_empty() {
        return;
    }

    let mut sorted_shared = unique_shared;
    sorted_shared.sort();

    let mapping: HashMap<usize, usize> = sorted_shared
        .into_iter()
        .enumerate()
        .map(|(idx, old_id)| (old_id, idx + 1))
        .collect();

    // Apply mapping
    for link in links.iter_mut() {
        if link.shared > 0 {
            link.shared = mapping[&link.shared];
        }
    }
}

/// Find quickest direct public paths between cities
#[inline]
fn find_direct_paths(
    public_links: &[Link],
    src_city: &str,
    dst_cities: &HashSet<&String>,
    traffic_type: usize,
) -> Result<Vec<Link>> {
    let mut city_paths: HashMap<(String, String), Decimal> = HashMap::new();

    // Find all paths from source city to destination cities
    for link in public_links {
        if link.start.len() >= 3 && link.end.len() >= 3 {
            let start_city = &link.start[..3];
            let end_city = &link.end[..3];

            if start_city == src_city && dst_cities.contains(&end_city.to_string()) {
                let key = (start_city.to_string(), end_city.to_string());
                city_paths
                    .entry(key)
                    .and_modify(|e| *e = (*e).min(link.cost))
                    .or_insert(link.cost);
            }
        }
    }

    // Create direct path links
    let mut direct_links = Vec::new();
    for ((start, end), cost) in city_paths {
        let link = LinkBuilder::default()
            .start(start)
            .end(end)
            .cost(cost)
            .link_type(traffic_type)
            .build()
            .unwrap();
        direct_links.push(link);
    }

    Ok(direct_links)
}

/// Create zero-cost helper links from source city to its switches
#[inline]
fn create_source_helpers(
    public_links: &[Link],
    src_city: &str,
    traffic_type: usize,
) -> Result<Vec<Link>> {
    let mut src_switches = HashSet::new();

    for link in public_links {
        if link.start.len() >= 3 && &link.start[..3] == src_city {
            src_switches.insert(link.start.clone());
        }
    }

    let mut helpers = Vec::new();
    for switch in src_switches {
        let link = LinkBuilder::default()
            .start(src_city.to_string())
            .end(switch)
            .cost(Decimal::ZERO)
            .link_type(traffic_type)
            .build()
            .unwrap();
        helpers.push(link);
    }

    Ok(helpers)
}

/// Create zero-cost helper links from switches to destination cities
#[inline]
fn create_destination_helpers(
    public_links: &[Link],
    dst_cities: &HashSet<&String>,
    traffic_type: usize,
) -> Result<Vec<Link>> {
    let mut dst_switches = HashSet::new();

    for link in public_links {
        if link.end.len() >= 3 {
            let city_code = &link.end[..3];
            if dst_cities.iter().any(|&city| city == city_code) {
                dst_switches.insert(link.end.clone());
            }
        }
    }

    let mut helpers = Vec::new();
    for switch in dst_switches.iter() {
        let city = &switch[..3];
        let link = LinkBuilder::default()
            .start(switch.to_string())
            .end(city.to_string())
            .cost(Decimal::ZERO)
            .link_type(traffic_type)
            .build()
            .unwrap();
        helpers.push(link);
    }

    Ok(helpers)
}

#[cfg(test)]
mod tests {
    use crate::types::DemandBuilder;

    use super::*;
    use rust_decimal::dec;

    #[test]
    fn test_prepare_private_links() {
        let mut links = vec![
            {
                LinkBuilder::default()
                    .start("NYC1".to_string())
                    .end("LAX1".to_string())
                    .cost(dec!(10))
                    .bandwidth(dec!(100))
                    .operator1("Op1".to_string())
                    .uptime(dec!(0.9))
                    .shared(0)
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("LAX1".to_string())
                    .end("CHI1".to_string())
                    .cost(dec!(20))
                    .bandwidth(dec!(200))
                    .operator1("Op2".to_string())
                    .operator2("Op3".to_string())
                    .uptime(dec!(1.0))
                    .shared(1)
                    .build()
                    .unwrap()
            },
        ];

        let result = prepare_private_links(&mut links).unwrap();

        // Should have 4 links (2 original + 2 reverse)
        assert_eq!(result.len(), 4);

        // Check operator2 filling
        assert_eq!(result[0].operator2, "Op1");

        // Check bandwidth adjustment for uptime
        assert_eq!(result[0].bandwidth, dec!(90)); // 100 * 0.9
        assert_eq!(result[2].bandwidth, dec!(200)); // 200 * 1.0

        // Check bidirectional links
        assert_eq!(result[0].start, "NYC1");
        assert_eq!(result[0].end, "LAX1");
        assert_eq!(result[1].start, "LAX1");
        assert_eq!(result[1].end, "NYC1");

        // Check shared ID compaction
        let shared_ids: HashSet<usize> = result.iter().map(|l| l.shared).collect();
        let mut sorted_shared: Vec<usize> = shared_ids.into_iter().collect();
        sorted_shared.sort();
        assert_eq!(sorted_shared, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_prepare_public_links() {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("NYC1".to_string())
                    .end("LAX1".to_string())
                    .cost(dec!(50))
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("LAX1".to_string())
                    .end("CHI1".to_string())
                    .cost(dec!(60))
                    .build()
                    .unwrap()
            },
        ];

        let result = prepare_public_links(&links).unwrap();

        // Should have 4 links (2 original + 2 reverse)
        assert_eq!(result.len(), 4);

        // Check bidirectional links
        assert_eq!(result[0].start, "NYC1");
        assert_eq!(result[0].end, "LAX1");
        assert_eq!(result[1].start, "LAX1");
        assert_eq!(result[1].end, "NYC1");

        // Check link type is set to 0
        assert!(result.iter().all(|l| l.link_type == 0));
    }

    #[test]
    fn test_generate_helper_links() {
        let public_links = vec![
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("LAX1".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("NYC2".to_string())
                .end("LAX2".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("LAX1".to_string())
                .end("NYC1".to_string())
                .build()
                .unwrap(),
        ];

        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(dec!(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let demand_matrix = DemandMatrix::from_demands(demands);

        let result = generate_helper_links(&public_links, &demand_matrix).unwrap();

        // Should have:
        // - 1 direct path (NYC->LAX)
        // - 2 source helpers (NYC->NYC1, NYC->NYC2)
        // - 2 destination helpers (LAX1->LAX, LAX2->LAX)
        assert!(result.len() >= 3); // At least these

        // Check for zero-cost helpers
        let zero_cost_links: Vec<&Link> = result.iter().filter(|l| l.cost == dec!(0)).collect();
        assert!(!zero_cost_links.is_empty());

        // Check traffic type assignment
        assert!(result.iter().all(|l| l.link_type == 1));
    }

    #[test]
    fn test_merge_link_components() {
        let private_links = vec![{
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("LAX1".to_string())
                .operator1("Op1".to_string())
                .bandwidth(dec!(100))
                .build()
                .unwrap()
        }];

        let public_links = vec![{
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("LAX1".to_string())
                .cost(dec!(50))
                .build()
                .unwrap()
        }];

        let helper_links = vec![{
            LinkBuilder::default()
                .start("NYC".to_string())
                .end("NYC1".to_string())
                .link_type(1)
                .build()
                .unwrap()
        }];

        let result =
            merge_link_components(private_links, public_links, helper_links, dec!(5)).unwrap();

        assert_eq!(result.len(), 3);

        // Check private link is first
        assert_eq!(result[0].operator1, "Op1");

        // Check public link has penalty and operator "0"
        assert_eq!(result[1].cost, dec!(55)); // 50 + 5
        assert_eq!(result[1].operator1, "0");
        assert_eq!(result[1].bandwidth, dec!(0));

        // Check helper link properties
        assert_eq!(result[2].cost, dec!(0));
        assert_eq!(result[2].operator1, "0");
    }

    #[test]
    fn test_compact_shared_ids() {
        let mut links = vec![
            {
                LinkBuilder::default()
                    .start("A".to_string())
                    .end("B".to_string())
                    .shared(5)
                    .operator1("Op1".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("B".to_string())
                    .end("C".to_string())
                    .operator1("Op2".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("C".to_string())
                    .end("D".to_string())
                    .shared(10)
                    .operator1("Op3".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("D".to_string())
                    .end("E".to_string())
                    .operator1("Op4".to_string())
                    .build()
                    .unwrap()
            },
        ];

        compact_shared_ids(&mut links);

        // Check that shared IDs are compacted to 1, 2, 3, 4
        let shared_ids: HashSet<usize> = links.iter().map(|l| l.shared).collect();
        assert_eq!(shared_ids, HashSet::from([1, 2, 3, 4]));
    }
}
