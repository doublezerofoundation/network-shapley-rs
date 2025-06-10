use crate::{
    error::ShapleyError,
    types::{DemandMatrix, Link, Result},
    utils::decimal_to_f64,
};
use faer::{
    Col, Unbind,
    sparse::{SparseColMat, Triplet},
};
use std::collections::{HashMap, HashSet};

/// Build mapping from node names to indices
pub fn build_node_index(link_map: &[Link], demand: &DemandMatrix) -> HashMap<String, usize> {
    let mut nodes = HashSet::new();

    // Collect all unique nodes
    for link in link_map {
        nodes.insert(link.start.clone());
        nodes.insert(link.end.clone());
    }

    for d in &demand.demands {
        nodes.insert(d.start.clone());
        nodes.insert(d.end.clone());
    }

    // Sort nodes for deterministic ordering
    let mut sorted_nodes: Vec<String> = nodes.into_iter().collect();
    sorted_nodes.sort();

    // Create index mapping
    sorted_nodes
        .into_iter()
        .enumerate()
        .map(|(idx, node)| (node, idx))
        .collect()
}

/// Result type for flow constraints
pub type FlowConstraints = (SparseColMat<usize, f64>, Col<f64>, Vec<usize>);

/// Build flow conservation constraint matrix and demand vector
pub fn build_flow_constraints(
    link_map: &[Link],
    demand: &DemandMatrix,
    node_idx: &HashMap<String, usize>,
) -> Result<FlowConstraints> {
    let n_nodes = node_idx.len();
    let n_links = link_map.len();

    // Build single commodity constraint matrix
    let a_single = build_single_commodity_matrix(link_map, node_idx, n_nodes, n_links)?;

    // Get unique commodities (traffic types)
    let commodities = demand.unique_types();
    let n_commodities = commodities.len();

    // Replicate for all commodities using block diagonal
    let mut blocks = Vec::with_capacity(n_commodities);
    for _ in 0..n_commodities {
        blocks.push(a_single.clone());
    }
    let a_full = block_diagonal(&blocks)?;

    // Filter columns for traffic type compatibility
    let keep = get_valid_columns(link_map, &commodities, n_links);
    let a_eq = select_columns(&a_full, &keep)?;

    // Build demand vector
    let b_eq = build_demand_vector(demand, node_idx, &commodities)?;

    Ok((a_eq, b_eq, keep))
}

/// Build bandwidth constraint matrix and capacity vector
pub fn build_bandwidth_constraints(
    link_map: &[Link],
    n_private: usize,
    commodities: &[usize],
    keep: &[usize],
) -> Result<(SparseColMat<usize, f64>, Col<f64>)> {
    // Get shared IDs for private links
    let shared_ids: Vec<usize> = link_map[..n_private]
        .iter()
        .map(|link| link.shared)
        .collect();

    if shared_ids.is_empty() {
        return Ok((
            SparseColMat::try_new_from_triplets(0, 0, &[]).map_err(|_| {
                ShapleyError::ComputationError("Failed to create sparse matrix".to_string())
            })?,
            Col::zeros(0),
        ));
    }

    let max_shared = *shared_ids.iter().max().unwrap_or(&0);
    let n_links = link_map.len();

    // Build constraint matrix for shared bandwidth
    let mut triplets = Vec::new();
    for (col, &shared) in shared_ids.iter().enumerate() {
        if shared > 0 {
            triplets.push(Triplet::new(shared - 1, col, 1.0));
        }
    }

    let i_single =
        SparseColMat::try_new_from_triplets(max_shared, n_links, &triplets).map_err(|_| {
            ShapleyError::ComputationError("Failed to create sparse matrix".to_string())
        })?;

    // Replicate for all commodities
    let mut blocks = Vec::with_capacity(commodities.len());
    for _ in commodities {
        blocks.push(i_single.clone());
    }
    let i_full = horizontal_concat(&blocks)?;
    let a_ub = select_columns(&i_full, keep)?;

    // Get capacities for unique shared groups
    let mut shared_bandwidth = HashMap::new();
    for link in &link_map[..n_private] {
        shared_bandwidth.insert(link.shared, decimal_to_f64(link.bandwidth));
    }

    let mut capacities = Vec::with_capacity(max_shared);
    for shared in 1..=max_shared {
        capacities.push(shared_bandwidth.get(&shared).copied().unwrap_or(0.0));
    }

    let b_ub = Col::from_iter(capacities);

    Ok((a_ub, b_ub))
}

/// Extract operator indices for rows and columns
pub fn extract_operator_indices(
    link_map: &[Link],
    n_private: usize,
    commodities: &[usize],
    keep: &[usize],
) -> HashMap<String, Vec<String>> {
    // Get row operators from unique shared groups
    let mut shared_operators = HashMap::new();
    for link in &link_map[..n_private] {
        shared_operators.insert(
            link.shared,
            (link.operator1.clone(), link.operator2.clone()),
        );
    }

    let mut sorted_shared: Vec<usize> = shared_operators.keys().copied().collect();
    sorted_shared.sort();

    let row_index1: Vec<String> = sorted_shared
        .iter()
        .map(|&shared| shared_operators[&shared].0.clone())
        .collect();

    let row_index2: Vec<String> = sorted_shared
        .iter()
        .map(|&shared| shared_operators[&shared].1.clone())
        .collect();

    // Get column operators (replicated for commodities)
    let all_op1: Vec<String> = commodities
        .iter()
        .flat_map(|_| link_map.iter().map(|l| l.operator1.clone()))
        .collect();

    let all_op2: Vec<String> = commodities
        .iter()
        .flat_map(|_| link_map.iter().map(|l| l.operator2.clone()))
        .collect();

    let col_index1: Vec<String> = keep.iter().map(|&i| all_op1[i].clone()).collect();
    let col_index2: Vec<String> = keep.iter().map(|&i| all_op2[i].clone()).collect();

    let mut result = HashMap::new();
    result.insert("row_index1".to_string(), row_index1);
    result.insert("row_index2".to_string(), row_index2);
    result.insert("col_index1".to_string(), col_index1);
    result.insert("col_index2".to_string(), col_index2);

    result
}

/// Build objective function coefficients (costs)
pub fn build_objective_coefficients(
    link_map: &[Link],
    commodities: &[usize],
    keep: &[usize],
) -> Col<f64> {
    let all_costs: Vec<f64> = commodities
        .iter()
        .flat_map(|_| link_map.iter().map(|l| decimal_to_f64(l.cost)))
        .collect();

    let costs: Vec<f64> = keep.iter().map(|&i| all_costs[i]).collect();
    Col::from_iter(costs)
}

// Helper functions

fn build_single_commodity_matrix(
    link_map: &[Link],
    node_idx: &HashMap<String, usize>,
    n_nodes: usize,
    n_links: usize,
) -> Result<SparseColMat<usize, f64>> {
    let mut triplets = Vec::new();

    for (j, link) in link_map.iter().enumerate() {
        let start_idx = node_idx.get(&link.start).ok_or_else(|| {
            ShapleyError::ComputationError(format!("Node {} not found", link.start))
        })?;
        let end_idx = node_idx.get(&link.end).ok_or_else(|| {
            ShapleyError::ComputationError(format!("Node {} not found", link.end))
        })?;

        triplets.push(Triplet::new(*start_idx, j, 1.0));
        triplets.push(Triplet::new(*end_idx, j, -1.0));
    }

    SparseColMat::try_new_from_triplets(n_nodes, n_links, &triplets)
        .map_err(|_| ShapleyError::ComputationError("Failed to create sparse matrix".to_string()))
}

#[inline]
fn get_valid_columns(link_map: &[Link], commodities: &[usize], n_links: usize) -> Vec<usize> {
    let mut keep = Vec::new();

    for (k, &t) in commodities.iter().enumerate() {
        for (j, link) in link_map.iter().enumerate() {
            if link.link_type == t || link.link_type == 0 {
                keep.push(j + k * n_links);
            }
        }
    }

    keep
}

fn build_demand_vector(
    demand: &DemandMatrix,
    node_idx: &HashMap<String, usize>,
    commodities: &[usize],
) -> Result<Col<f64>> {
    let n_nodes = node_idx.len();
    let mut b_flows = Vec::new();

    for &t in commodities {
        let mut vec = Col::<f64>::zeros(n_nodes);

        for d in demand.demands.iter().filter(|d| d.demand_type == t) {
            let start_idx = node_idx.get(&d.start).ok_or_else(|| {
                ShapleyError::ComputationError(format!("Node {} not found", d.start))
            })?;
            let end_idx = node_idx.get(&d.end).ok_or_else(|| {
                ShapleyError::ComputationError(format!("Node {} not found", d.end))
            })?;

            let traffic = decimal_to_f64(d.traffic);
            vec[*start_idx] += traffic;
            vec[*end_idx] -= traffic;
        }

        b_flows.extend(vec.as_ref().iter().copied());
    }

    Ok(Col::from_iter(b_flows))
}

fn block_diagonal(matrices: &[SparseColMat<usize, f64>]) -> Result<SparseColMat<usize, f64>> {
    if matrices.is_empty() {
        return SparseColMat::try_new_from_triplets(0, 0, &[]).map_err(|_| {
            ShapleyError::ComputationError("Failed to create sparse matrix".to_string())
        });
    }

    let total_rows: usize = matrices.iter().map(|m| m.nrows()).sum();
    let total_cols: usize = matrices.iter().map(|m| m.ncols()).sum();

    // Pre-calculate total number of non-zeros for better memory allocation
    let total_nnz: usize = matrices.iter().map(|m| m.triplet_iter().count()).sum();
    let mut triplets = Vec::with_capacity(total_nnz);

    let mut row_offset = 0;
    let mut col_offset = 0;

    for mat in matrices {
        for triplet in mat.triplet_iter() {
            triplets.push(Triplet::new(
                row_offset + triplet.row.unbound(),
                col_offset + triplet.col.unbound(),
                *triplet.val,
            ));
        }
        row_offset += mat.nrows();
        col_offset += mat.ncols();
    }

    SparseColMat::try_new_from_triplets(total_rows, total_cols, &triplets)
        .map_err(|_| ShapleyError::ComputationError("Failed to create sparse matrix".to_string()))
}

fn horizontal_concat(matrices: &[SparseColMat<usize, f64>]) -> Result<SparseColMat<usize, f64>> {
    if matrices.is_empty() {
        return SparseColMat::try_new_from_triplets(0, 0, &[]).map_err(|_| {
            ShapleyError::ComputationError("Failed to create sparse matrix".to_string())
        });
    }

    let n_rows = matrices[0].nrows();
    let total_cols: usize = matrices.iter().map(|m| m.ncols()).sum();

    // Pre-calculate total number of non-zeros for better memory allocation
    let total_nnz: usize = matrices.iter().map(|m| m.triplet_iter().count()).sum();
    let mut triplets = Vec::with_capacity(total_nnz);

    let mut col_offset = 0;

    for mat in matrices {
        for triplet in mat.triplet_iter() {
            triplets.push(Triplet::new(
                triplet.row.unbound(),
                col_offset + triplet.col.unbound(),
                *triplet.val,
            ));
        }
        col_offset += mat.ncols();
    }

    SparseColMat::try_new_from_triplets(n_rows, total_cols, &triplets)
        .map_err(|_| ShapleyError::ComputationError("Failed to create sparse matrix".to_string()))
}

fn select_columns(
    matrix: &SparseColMat<usize, f64>,
    keep: &[usize],
) -> Result<SparseColMat<usize, f64>> {
    let n_rows = matrix.nrows();
    let n_cols = keep.len();

    // Build a reverse mapping for O(1) column lookups
    // Since keep is sorted, we can use binary search, but HashMap is simpler and fast enough
    let col_map: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new_idx, &old_idx)| (old_idx, new_idx))
        .collect();

    // Collect all triplets from the selected columns
    let mut all_triplets = Vec::with_capacity(matrix.triplet_iter().count());

    for triplet in matrix.triplet_iter() {
        let col = triplet.col.unbound();
        if let Some(&new_col) = col_map.get(&col) {
            all_triplets.push(Triplet::new(triplet.row.unbound(), new_col, *triplet.val));
        }
    }

    SparseColMat::try_new_from_triplets(n_rows, n_cols, &all_triplets)
        .map_err(|_| ShapleyError::ComputationError("Failed to create sparse matrix".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DemandBuilder, LinkBuilder};
    use rust_decimal::dec;

    #[test]
    fn test_build_node_index() {
        let links = vec![
            LinkBuilder::default()
                .start("A".to_string())
                .end("B".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("B".to_string())
                .end("C".to_string())
                .build()
                .unwrap(),
        ];

        let demands = vec![
            DemandBuilder::default()
                .start("A".to_string())
                .end("C".to_string())
                .traffic(dec!(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let demand_matrix = DemandMatrix::from_demands(demands);

        let node_idx = build_node_index(&links, &demand_matrix);

        assert_eq!(node_idx.len(), 3);
        assert_eq!(node_idx["A"], 0);
        assert_eq!(node_idx["B"], 1);
        assert_eq!(node_idx["C"], 2);
    }

    #[test]
    fn test_build_flow_constraints() {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("A".to_string())
                    .end("B".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("B".to_string())
                    .end("C".to_string())
                    .build()
                    .unwrap()
            },
        ];

        let demands = vec![
            DemandBuilder::default()
                .start("A".to_string())
                .end("C".to_string())
                .traffic(dec!(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let demand_matrix = DemandMatrix::from_demands(demands);

        let node_idx = build_node_index(&links, &demand_matrix);
        let (a_eq, b_eq, keep) = build_flow_constraints(&links, &demand_matrix, &node_idx).unwrap();

        // Check matrix dimensions
        assert_eq!(a_eq.nrows(), 3); // 3 nodes
        assert_eq!(a_eq.ncols(), 2); // 2 links

        // Check demand vector
        assert_eq!(b_eq[0], 10.0); // Source A
        assert_eq!(b_eq[2], -10.0); // Sink C
        assert_eq!(b_eq[1], 0.0); // Intermediate B

        // Check keep vector
        assert_eq!(keep, vec![0, 1]);
    }

    #[test]
    fn test_build_bandwidth_constraints() {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("A".to_string())
                    .end("B".to_string())
                    .shared(1)
                    .bandwidth(dec!(100))
                    .operator1("Op1".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("B".to_string())
                    .end("C".to_string())
                    .shared(1)
                    .bandwidth(dec!(100))
                    .operator1("Op1".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("C".to_string())
                    .end("D".to_string())
                    .shared(2)
                    .bandwidth(dec!(50))
                    .operator1("Op2".to_string())
                    .build()
                    .unwrap()
            },
        ];

        let commodities = vec![1];
        let keep = vec![0, 1, 2];
        let (a_ub, b_ub) = build_bandwidth_constraints(&links, 3, &commodities, &keep).unwrap();

        // Check matrix dimensions
        assert_eq!(a_ub.nrows(), 2); // 2 shared groups
        assert_eq!(a_ub.ncols(), 3); // 3 links

        // Check capacities
        assert_eq!(b_ub.nrows(), 2);
        assert_eq!(b_ub[0], 100.0);
        assert_eq!(b_ub[1], 50.0);
    }

    #[test]
    fn test_extract_operator_indices() {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("A".to_string())
                    .end("B".to_string())
                    .shared(1)
                    .operator1("Op1".to_string())
                    .operator2("Op1".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("B".to_string())
                    .end("C".to_string())
                    .shared(2)
                    .operator1("Op2".to_string())
                    .operator2("Op3".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("C".to_string())
                    .end("D".to_string())
                    .operator1("0".to_string())
                    .operator2("0".to_string())
                    .build()
                    .unwrap()
            },
        ];

        let commodities = vec![1];
        let keep = vec![0, 1, 2];
        let indices = extract_operator_indices(&links, 2, &commodities, &keep);

        assert_eq!(indices["row_index1"], vec!["Op1", "Op2"]);
        assert_eq!(indices["row_index2"], vec!["Op1", "Op3"]);
        assert_eq!(indices["col_index1"], vec!["Op1", "Op2", "0"]);
        assert_eq!(indices["col_index2"], vec!["Op1", "Op3", "0"]);
    }

    #[test]
    fn test_build_objective_coefficients() {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("A".to_string())
                    .end("B".to_string())
                    .cost(dec!(10))
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("B".to_string())
                    .end("C".to_string())
                    .cost(dec!(20))
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("C".to_string())
                    .end("D".to_string())
                    .cost(dec!(30))
                    .build()
                    .unwrap()
            },
        ];

        let commodities = vec![1, 2];
        let keep = vec![0, 1, 3, 4]; // Keep some columns

        let costs = build_objective_coefficients(&links, &commodities, &keep);

        assert_eq!(costs.nrows(), 4);
        assert_eq!(costs[0], 10.0);
        assert_eq!(costs[1], 20.0);
        assert_eq!(costs[2], 10.0); // Second commodity
        assert_eq!(costs[3], 20.0);
    }
}
