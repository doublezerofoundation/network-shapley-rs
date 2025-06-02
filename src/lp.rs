use crate::{
    link_preparation::{
        generate_helper_links, merge_link_components, prepare_private_links, prepare_public_links,
    },
    lp_construction::{
        build_bandwidth_constraints, build_flow_constraints, build_node_index,
        build_objective_coefficients, extract_operator_indices,
    },
    types::{DemandMatrix, LPPrimitives, Link, PrivateLinks, PublicLinks, Result},
    validation::{
        validate_endpoint_naming, validate_private_links, validate_public_links,
        validate_public_pathway_coverage, validate_switch_naming, validate_traffic_types,
    },
};
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
pub fn primitives(
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
