use crate::{
    coalition_computation::{
        calculate_shapley_values, compute_expected_values, enumerate_operators,
        generate_coalition_bitmap, solve_coalition_values,
    },
    link_preparation::{
        generate_helper_links, merge_link_components, prepare_private_links, prepare_public_links,
    },
    lp_construction::{
        build_bandwidth_constraints, build_flow_constraints, build_node_index,
        build_objective_coefficients, extract_operator_indices,
    },
    types::{
        DemandMatrix, LPPrimitives, Link, PrivateLinks, PublicLinks, Result, ShapleyValue,
        decimal_to_f64,
    },
    validation::{
        validate_endpoint_naming, validate_operator_names, validate_private_links,
        validate_public_links, validate_public_pathway_coverage, validate_switch_naming,
        validate_traffic_types,
    },
};
use faer::Par;
use rust_decimal::Decimal;

/// Construct a single and fully-validated link table for LP primitives
pub fn consolidate_map(
    private_links: &PrivateLinks,
    public_links: &PublicLinks,
    demand: &DemandMatrix,
    hybrid_penalty: Decimal,
) -> Result<Vec<Link>> {
    // Validate input data
    validate_private_links(private_links)?;
    validate_public_links(public_links)?;
    validate_switch_naming(&private_links.links, "private")?;
    validate_switch_naming(&public_links.links, "public")?;
    validate_endpoint_naming(demand)?;
    validate_traffic_types(demand)?;

    // Prepare links
    let mut private_links_mut = private_links.links.clone();
    let private_df = prepare_private_links(&mut private_links_mut)?;
    let public_df = prepare_public_links(&public_links.links)?;

    // Validate public pathway coverage
    validate_public_pathway_coverage(&private_df, &public_df, demand)?;

    // Generate helper links
    let helper_df = generate_helper_links(&public_df, demand)?;

    // Merge all components
    merge_link_components(private_df, public_df, helper_df, hybrid_penalty)
}

/// Translate link map and demand into the core linear program primitives
pub fn lp_primitives(
    link_map: &[Link],
    demand: &DemandMatrix,
    demand_multiplier: Decimal,
) -> Result<LPPrimitives> {
    // Scale demand
    let mut scaled_demand = demand.clone();
    for d in &mut scaled_demand.demands {
        d.traffic *= demand_multiplier;
    }

    // Count private links (those with non-"0" operators)
    let n_private = link_map.iter().filter(|link| link.operator1 != "0").count();

    // Build node index
    let node_idx = build_node_index(link_map, &scaled_demand);

    // Build constraint matrices
    let commodities = scaled_demand.unique_types();
    let (a_eq, b_eq, keep) = build_flow_constraints(link_map, &scaled_demand, &node_idx)?;
    let (a_ub, b_ub) = build_bandwidth_constraints(link_map, n_private, &commodities, &keep)?;

    // Extract operator indices
    let op_indices = extract_operator_indices(link_map, n_private, &commodities, &keep);

    // Build objective coefficients
    let cost = build_objective_coefficients(link_map, &commodities, &keep);

    Ok(LPPrimitives {
        a_eq,
        a_ub,
        b_eq,
        b_ub,
        cost,
        row_index1: op_indices["row_index1"].clone(),
        row_index2: op_indices["row_index2"].clone(),
        col_index1: op_indices["col_index1"].clone(),
        col_index2: op_indices["col_index2"].clone(),
    })
}

/// Compute Shapley values per operator
pub fn network_shapley(
    private_links: &PrivateLinks,
    public_links: &PublicLinks,
    demand: &DemandMatrix,
    operator_uptime: Decimal,
    hybrid_penalty: Decimal,
    demand_multiplier: Decimal,
) -> Result<Vec<ShapleyValue>> {
    // Configure faer to use all available threads for matrix operations
    faer::set_global_parallelism(Par::rayon(0));

    // Enumerate operators and validate
    let operators = enumerate_operators(&private_links.links);
    validate_operator_names(&operators)?;
    let n_ops = operators.len();

    // Generate coalition bitmap
    let bitmap = generate_coalition_bitmap(n_ops);

    // Get LP primitives
    let full_map = consolidate_map(private_links, public_links, demand, hybrid_penalty)?;
    let primitives = lp_primitives(&full_map, demand, demand_multiplier)?;

    // Solve for coalition values
    let (svalue, size) = solve_coalition_values(&operators, &bitmap, &primitives)?;

    // Compute expected values with downtime
    let evalue = compute_expected_values(&svalue, &size, decimal_to_f64(operator_uptime), n_ops)?;

    // Calculate Shapley values
    calculate_shapley_values(&operators, &evalue, &size, n_ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_example_private_links() -> PrivateLinks {
        let links = vec![
            {
                let mut link = Link::new("FRA1".to_string(), "NYC1".to_string());
                link.cost = dec!(40);
                link.bandwidth = dec!(10);
                link.operator1 = "Alpha".to_string();
                link.operator2 = "0".to_string();
                link.uptime = dec!(1);
                link.shared = 0;
                link
            },
            {
                let mut link = Link::new("FRA1".to_string(), "SIN1".to_string());
                link.cost = dec!(50);
                link.bandwidth = dec!(10);
                link.operator1 = "Beta".to_string();
                link.operator2 = "0".to_string();
                link.uptime = dec!(1);
                link.shared = 0;
                link
            },
            {
                let mut link = Link::new("SIN1".to_string(), "NYC1".to_string());
                link.cost = dec!(80);
                link.bandwidth = dec!(10);
                link.operator1 = "Gamma".to_string();
                link.operator2 = "0".to_string();
                link.uptime = dec!(1);
                link.shared = 0;
                link
            },
        ];
        PrivateLinks::from_links(links)
    }

    fn create_example_public_links() -> PublicLinks {
        let links = vec![
            {
                let mut link = Link::new("FRA1".to_string(), "NYC1".to_string());
                link.cost = dec!(70);
                link
            },
            {
                let mut link = Link::new("FRA1".to_string(), "SIN1".to_string());
                link.cost = dec!(80);
                link
            },
            {
                let mut link = Link::new("SIN1".to_string(), "NYC1".to_string());
                link.cost = dec!(120);
                link
            },
        ];
        PublicLinks::from_links(links)
    }

    fn create_example_demand() -> DemandMatrix {
        let demands = vec![
            crate::types::Demand::new("SIN".to_string(), "NYC".to_string(), dec!(5), 1),
            crate::types::Demand::new("SIN".to_string(), "FRA".to_string(), dec!(5), 1),
        ];
        DemandMatrix::from_demands(demands)
    }

    #[test]
    fn test_consolidate_map() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let result = consolidate_map(&private_links, &public_links, &demand, dec!(5)).unwrap();

        // Should have private links (bidirectional), public links (bidirectional), and helper links
        assert!(result.len() > 6); // At least 6 for bidirectional private links

        // Check that private links are first and bidirectional
        assert_eq!(result[0].operator1, "Alpha");

        // Find the reverse of the first link
        let reverse_found = result
            .iter()
            .any(|link| link.start == "NYC1" && link.end == "FRA1" && link.operator1 == "Alpha");
        assert!(reverse_found);
    }

    #[test]
    fn test_lp_primitives() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let link_map = consolidate_map(&private_links, &public_links, &demand, dec!(5)).unwrap();
        let primitives = lp_primitives(&link_map, &demand, dec!(1)).unwrap();

        // Check that matrices have appropriate dimensions
        assert!(primitives.a_eq.nrows() > 0);
        assert!(primitives.a_eq.ncols() > 0);
        assert_eq!(primitives.b_eq.len(), primitives.a_eq.nrows());

        // Check operator indices
        assert!(!primitives.col_index1.is_empty());
        assert_eq!(primitives.col_index1.len(), primitives.col_index2.len());
    }

    #[test]
    fn test_network_shapley_example() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let result = network_shapley(
            &private_links,
            &public_links,
            &demand,
            dec!(0.98),
            dec!(5.0),
            dec!(1.0),
        )
        .unwrap();

        // Should have 3 operators
        assert_eq!(result.len(), 3);

        // Check operator names
        let operators: Vec<&str> = result.iter().map(|sv| sv.operator.as_str()).collect();
        assert!(operators.contains(&"Alpha"));
        assert!(operators.contains(&"Beta"));
        assert!(operators.contains(&"Gamma"));

        // Percentages should sum to 1
        let total: Decimal = result.iter().map(|sv| sv.percent).sum();
        assert_eq!(total, dec!(1.0));

        // All percentages should be non-negative
        assert!(result.iter().all(|sv| sv.percent >= dec!(0)));
    }
}
