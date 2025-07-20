use crate::{
    error::{Result, ShapleyError},
    multicast::{
        build_j1_matrix, build_j2_matrix, compute_j1_minus_j2, extract_mcast_eligible_columns,
        hstack_matrices,
    },
    types::{ConsolidatedDemand, ConsolidatedLink},
};
use clarabel::algebra::CscMatrix;
use std::collections::{BTreeMap, HashMap, HashSet};

type Constraints = (CscMatrix<f64>, Vec<f64>, Vec<String>, Vec<String>);

/// Input parameters for LP builder
#[derive(Debug)]
pub(crate) struct LpBuilderInput<'a> {
    pub links: &'a [ConsolidatedLink],
    pub demands: &'a [ConsolidatedDemand],
}

impl<'a> LpBuilderInput<'a> {
    pub(crate) fn new(links: &'a [ConsolidatedLink], demands: &'a [ConsolidatedDemand]) -> Self {
        Self { links, demands }
    }

    /// Build LP problem using the new API
    pub(crate) fn build(&self) -> Result<LpBuilderOutput> {
        let links = self.links;
        let demands = self.demands;

        // Count private links (non-public operators)
        let n_private = links.iter().filter(|l| l.operator1 != "Public").count();

        // Identify multicast eligible/ineligible links
        let mcast_eligible: Vec<usize> = links
            .iter()
            .enumerate()
            .filter(|(_i, l)| {
                // Python checks: ~(str[3:] == "00") & ~(str[3:] == "") & (op1 != "Public")
                // This means: device2[3:] is not "00" AND device2[3:] is not empty
                let device2_suffix = if l.device2.len() > 3 {
                    &l.device2[3..]
                } else {
                    ""
                };
                device2_suffix != "00" && !device2_suffix.is_empty() && l.operator1 != "Public"
            })
            .map(|(i, _)| i)
            .collect();

        let mcast_ineligible: Vec<usize> = links
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                // Python checks: (str[3:] == "00") | (str[3:] == "") & (op1 != "Public")
                let device2_suffix = if l.device2.len() > 3 {
                    &l.device2[3..]
                } else {
                    ""
                };
                (device2_suffix == "00" || device2_suffix.is_empty()) && l.operator1 != "Public"
            })
            .map(|(i, _)| i)
            .collect();

        let n_links = links.len();

        // Enumerate all nodes with indices
        let mut nodes_set = HashSet::new();
        for link in links {
            nodes_set.insert(link.device1.as_str());
            nodes_set.insert(link.device2.as_str());
        }
        for demand in demands {
            nodes_set.insert(demand.start.as_str());
            nodes_set.insert(demand.end.as_str());
        }

        let mut nodes: Vec<&str> = nodes_set.into_iter().collect();
        nodes.sort();

        let node_idx: HashMap<&str, usize> =
            nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();

        let n_nodes = nodes.len();

        // Build single commodity flow conservation matrix
        let a_single = build_single_commodity_matrix(links, &node_idx, n_nodes)?;

        // Enumerate commodities (unique demand types)
        let mut commodities: Vec<u32> = demands
            .iter()
            .map(|d| d.kind)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        commodities.sort();

        let n_commodities = commodities.len();
        let k_of_type: HashMap<u32, usize> = commodities
            .iter()
            .enumerate()
            .map(|(k, &t)| (t, k))
            .collect();

        // Get multicast flags for each commodity type
        let commodity_multicast_flag: HashMap<u32, bool> = {
            let mut flags = HashMap::new();
            for demand in demands {
                flags.insert(demand.kind, demand.multicast);
            }
            flags
        };

        // Get unique original types for multicast commodities
        let multicast_commodities: Vec<u32> = {
            let mut originals = HashSet::new();
            for demand in demands {
                if demand.multicast {
                    originals.insert(demand.original);
                }
            }
            let mut vec: Vec<_> = originals.into_iter().collect();
            vec.sort();
            vec
        };

        // Replicate constraint matrix for each commodity (block diagonal)
        let a_eq = block_diagonal_csc(&a_single, n_commodities)?;

        // Filter edges by traffic type
        let mut keep = Vec::new();
        for (k, &t) in commodities.iter().enumerate() {
            for (i, link) in links.iter().enumerate() {
                if link.link_type == t || link.link_type == 0 {
                    keep.push(i + k * n_links);
                }
            }
        }

        // Build bandwidth constraints
        let (mut a_ub, mut b_ub, mut row_op1, mut row_op2) = build_bandwidth_constraints(
            links,
            n_private,
            &commodity_multicast_flag,
            &commodities,
            &mcast_eligible,
            &mcast_ineligible,
            multicast_commodities.len(),
        )?;

        // Add "within-group" multicast constraints if needed
        let n_multicast_groups = multicast_commodities.len();
        if n_multicast_groups > 0 {
            let n_mcast_eligible = mcast_eligible.len();
            let total_cols = n_links * n_commodities + n_multicast_groups * n_mcast_eligible;

            let within_group_constraints = build_within_group_constraints(
                demands,
                &k_of_type,
                &multicast_commodities,
                &mcast_eligible,
                n_links,
                n_commodities,
                total_cols,
            )?;

            if within_group_constraints.m > 0 {
                // Add the new constraint rows to A_ub
                a_ub = vstack_matrices(&[&a_ub, &within_group_constraints])?;
                // Extend b_ub with zeros for these new constraints (since they are <= 0)
                b_ub.extend(vec![0.0; within_group_constraints.m]);

                // Extend row operators with actual operators from multicast eligible links
                // This matches Python's row_op1_multicast = _rep(link_df["Operator1"].iloc[mcast_eligible], demand_df["Multicast"].sum())
                let n_multicast_demands = demands.iter().filter(|d| d.multicast).count();
                for _ in 0..n_multicast_demands {
                    for &idx in &mcast_eligible {
                        if idx < links.len() {
                            row_op1.push(links[idx].operator1.clone());
                            row_op2.push(links[idx].operator2.clone());
                        }
                    }
                }
            }
        }

        // Pad A_eq to match A_ub's new auxiliary columns
        let total_cols = n_links * n_commodities + n_multicast_groups * mcast_eligible.len();
        let a_eq_padded = if total_cols > a_eq.n {
            let padding = CscMatrix::new(
                a_eq.m,
                total_cols - a_eq.n,
                vec![0; total_cols - a_eq.n + 1],
                vec![],
                vec![],
            );
            hstack_matrices(&[&a_eq, &padding])?
        } else {
            a_eq
        };

        // Build extended keep vector
        let mut keep_final = keep.clone();
        if n_multicast_groups > 0 {
            let aux_start = n_links * n_commodities;
            for i in 0..(n_multicast_groups * mcast_eligible.len()) {
                keep_final.push(aux_start + i);
            }
        }

        // Filter columns based on extended keep vector
        let a_eq_final = filter_columns(&a_eq_padded, &keep_final)?;
        let a_ub_final = filter_columns(&a_ub, &keep_final)?;

        // Build column operators
        let col_op1 = build_column_operators1(
            links,
            &commodities,
            &multicast_commodities,
            &mcast_eligible,
            &keep_final,
            n_multicast_groups,
        );
        let col_op2 = build_column_operators2(
            links,
            &commodities,
            &multicast_commodities,
            &mcast_eligible,
            &keep_final,
            n_multicast_groups,
        );

        // Build RHS vector for flow requirements
        let b_eq = build_flow_requirements(demands, &commodities, &k_of_type, &node_idx, n_nodes)?;

        // Build objective function coefficients
        let cost = build_objective_coefficients(
            links,
            demands,
            &commodities,
            &multicast_commodities,
            &mcast_eligible,
            &keep_final,
            n_multicast_groups,
        )?;

        Ok(LpPrimitives {
            a_eq: a_eq_final,
            a_ub: a_ub_final,
            b_eq,
            b_ub,
            cost,
            row_op1,
            row_op2,
            col_op1,
            col_op2,
        })
    }
}

/// Holds all components of the linear program
#[derive(Debug)]
pub(crate) struct LpBuilderOutput {
    pub a_eq: CscMatrix<f64>,
    pub a_ub: CscMatrix<f64>,
    pub b_eq: Vec<f64>,
    pub b_ub: Vec<f64>,
    pub cost: Vec<f64>,
    pub row_op1: Vec<String>,
    pub row_op2: Vec<String>,
    pub col_op1: Vec<String>,
    pub col_op2: Vec<String>,
}

// Keep LpPrimitives as an alias for backward compatibility
pub(crate) type LpPrimitives = LpBuilderOutput;

/// Build single commodity flow conservation matrix
fn build_single_commodity_matrix(
    links: &[ConsolidatedLink],
    node_idx: &HashMap<&str, usize>,
    n_nodes: usize,
) -> Result<CscMatrix<f64>> {
    let n_links = links.len();
    let mut triplets = Vec::new();

    for (j, link) in links.iter().enumerate() {
        let i1 = *node_idx.get(link.device1.as_str()).ok_or_else(|| {
            ShapleyError::MatrixConstructionError(format!(
                "Node {} not found in index",
                link.device1
            ))
        })?;
        let i2 = *node_idx.get(link.device2.as_str()).ok_or_else(|| {
            ShapleyError::MatrixConstructionError(format!(
                "Node {} not found in index",
                link.device2
            ))
        })?;

        triplets.push((i1, j, 1.0));
        triplets.push((i2, j, -1.0));
    }

    // Build CSC matrix from triplets using clarabel's API
    build_csc_from_triplets(&triplets, n_nodes, n_links)
}

/// Create block diagonal matrix from a single matrix repeated n times
fn block_diagonal_csc(matrix: &CscMatrix<f64>, n: usize) -> Result<CscMatrix<f64>> {
    let (m, k) = (matrix.m, matrix.n);
    let nnz = matrix.nnz() * n;

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::with_capacity(nnz);
    let mut values = Vec::with_capacity(nnz);

    for block in 0..n {
        let row_offset = block * m;

        for col in 0..k {
            let start = matrix.colptr[col];
            let end = matrix.colptr[col + 1];

            for idx in start..end {
                row_ind.push(matrix.rowval[idx] + row_offset);
                values.push(matrix.nzval[idx]);
            }

            col_ptr.push(row_ind.len());
        }
    }

    Ok(CscMatrix::new(m * n, k * n, col_ptr, row_ind, values))
}

/// Build bandwidth constraint matrix and related data with proper multicast handling
#[allow(clippy::too_many_arguments)]
fn build_bandwidth_constraints(
    links: &[ConsolidatedLink],
    n_private: usize,
    commodity_multicast_flag: &HashMap<u32, bool>,
    commodities: &[u32],
    mcast_eligible: &[usize],
    mcast_ineligible: &[usize],
    n_multicast_groups: usize,
) -> Result<Constraints> {
    if n_private == 0 {
        // No private links - return empty constraint matrix
        return Ok((
            CscMatrix::new(0, commodities.len() * links.len(), vec![0], vec![], vec![]),
            vec![],
            vec![],
            vec![],
        ));
    }

    // Find max shared ID
    let max_shared = links
        .iter()
        .filter(|l| l.shared > 0)
        .map(|l| l.shared)
        .max()
        .unwrap_or(0) as usize;

    // Build J1 matrix (all private links)
    let j1 = build_j1_matrix(links, n_private, max_shared)?;
    // Build J2 matrix (multicast ineligible links only)
    let j2 = build_j2_matrix(links, mcast_ineligible, max_shared)?;

    // Stack J1 or J2 horizontally for each commodity based on multicast flag
    let mut i_blocks = Vec::new();

    for t in commodities {
        let is_multicast = commodity_multicast_flag.get(t).copied().unwrap_or(false);
        if is_multicast {
            i_blocks.push(&j2);
        } else {
            i_blocks.push(&j1);
        }
    }

    let mut i = if !i_blocks.is_empty() {
        hstack_matrices(&i_blocks)?
    } else {
        CscMatrix::new(0, commodities.len() * links.len(), vec![0], vec![], vec![])
    };

    let mut b_ub = Vec::new();
    let mut row_op1 = Vec::new();
    let mut row_op2 = Vec::new();

    // Build initial bandwidth capacity vector
    // We need to create a vector with max_shared elements, filling in the bandwidth
    // for each shared ID that exists
    let mut bandwidth_by_shared: BTreeMap<usize, f64> = BTreeMap::new();
    let mut op1_by_shared: BTreeMap<usize, String> = BTreeMap::new();
    let mut op2_by_shared: BTreeMap<usize, String> = BTreeMap::new();

    // Debug: collect all shared IDs in private links
    let mut all_shared_ids: HashSet<u32> = HashSet::new();

    for link in links[..n_private].iter() {
        if link.shared > 0 && link.shared as usize <= max_shared {
            all_shared_ids.insert(link.shared);
            let shared_idx = link.shared as usize - 1; // 0-based index
            // Only keep the first occurrence of each shared ID
            bandwidth_by_shared
                .entry(shared_idx)
                .or_insert(link.bandwidth);
            op1_by_shared
                .entry(shared_idx)
                .or_insert(link.operator1.clone());
            op2_by_shared
                .entry(shared_idx)
                .or_insert(link.operator2.clone());
        }
    }

    // Fill b_ub, row_op1, row_op2 only for existing shared IDs (matching Python's drop_duplicates behavior)
    let mut existing_shared: Vec<usize> = bandwidth_by_shared.keys().copied().collect();
    existing_shared.sort();

    for shared_id in existing_shared {
        b_ub.push(bandwidth_by_shared.get(&shared_id).copied().unwrap_or(0.0));
        row_op1.push(op1_by_shared.get(&shared_id).cloned().unwrap_or_default());
        row_op2.push(op2_by_shared.get(&shared_id).cloned().unwrap_or_default());
    }

    // Handle multicast constraints if needed
    if n_multicast_groups > 0 && !mcast_eligible.is_empty() {
        // Compute (J1 - J2) matrix
        let j1_minus_j2 = compute_j1_minus_j2(&j1, &j2)?;

        // Extract columns for multicast eligible links
        let j1_minus_j2_mcast = extract_mcast_eligible_columns(&j1_minus_j2, mcast_eligible)?;

        // Stack (J1-J2)[:, mcast_eligible] for each unique multicast group
        let mut extend_blocks = Vec::new();
        for _ in 0..n_multicast_groups {
            extend_blocks.push(&j1_minus_j2_mcast);
        }

        if !extend_blocks.is_empty() {
            let extension = hstack_matrices(&extend_blocks)?;
            i = hstack_matrices(&[&i, &extension])?;

            // Note: We don't extend row_op* or b_ub here because hstack adds columns, not rows
            // The Python code doesn't extend b_ub for the hstack part either
        }
    }

    Ok((i, b_ub, row_op1, row_op2))
}

/// Build CSC matrix from triplets
fn build_csc_from_triplets(
    triplets: &[(usize, usize, f64)],
    n_rows: usize,
    n_cols: usize,
) -> Result<CscMatrix<f64>> {
    if triplets.is_empty() {
        return Ok(CscMatrix::new(
            n_rows,
            n_cols,
            vec![0; n_cols + 1],
            vec![],
            vec![],
        ));
    }

    // Sort triplets by column, then row
    let mut sorted_triplets = triplets.to_vec();
    sorted_triplets.sort_by_key(|&(r, c, _)| (c, r));

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    let mut current_col = 0;

    for &(row, col, val) in &sorted_triplets {
        // Fill in empty columns
        while current_col < col {
            col_ptr.push(row_ind.len());
            current_col += 1;
        }

        row_ind.push(row);
        values.push(val);
    }

    // Fill remaining columns
    while current_col < n_cols {
        col_ptr.push(row_ind.len());
        current_col += 1;
    }

    Ok(CscMatrix::new(n_rows, n_cols, col_ptr, row_ind, values))
}

/// Vertically stack multiple CSC matrices. All matrices must have the same number of columns.
fn vstack_matrices(matrices: &[&CscMatrix<f64>]) -> Result<CscMatrix<f64>> {
    if matrices.is_empty() {
        return Ok(CscMatrix::new(0, 0, vec![0], vec![], vec![]));
    }

    let n_cols = matrices[0].n;
    if !matrices.iter().all(|m| m.n == n_cols) {
        return Err(ShapleyError::MatrixConstructionError(
            "All matrices must have the same number of columns to vstack".to_string(),
        ));
    }

    let mut total_rows = 0;
    let mut total_nnz = 0;
    for m in matrices {
        total_rows += m.m;
        total_nnz += m.nnz();
    }

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::with_capacity(total_nnz);
    let mut values = Vec::with_capacity(total_nnz);

    for col in 0..n_cols {
        let mut current_row_offset = 0;
        for matrix in matrices {
            let start = matrix.colptr[col];
            let end = matrix.colptr[col + 1];
            for i in start..end {
                row_ind.push(matrix.rowval[i] + current_row_offset);
                values.push(matrix.nzval[i]);
            }
            current_row_offset += matrix.m;
        }
        col_ptr.push(row_ind.len());
    }

    Ok(CscMatrix::new(total_rows, n_cols, col_ptr, row_ind, values))
}

/// Builds the "within-group" constraints that link individual multicast demands to their master auxiliary flow.
/// Creates a sparse matrix representing the constraints: K*x_k/receivers - K_aux*x_orig <= 0
fn build_within_group_constraints(
    demands: &[ConsolidatedDemand],
    k_of_type: &HashMap<u32, usize>,
    multicast_groups: &[u32],
    mcast_eligible: &[usize],
    n_links: usize,
    n_commodities: usize,
    n_total_cols: usize,
) -> Result<CscMatrix<f64>> {
    let n_multicast_groups = multicast_groups.len();
    if n_multicast_groups == 0 {
        return Ok(CscMatrix::new(
            0,
            n_total_cols,
            vec![0; n_total_cols + 1],
            vec![],
            vec![],
        ));
    }

    let k_multicast: HashMap<u32, usize> = multicast_groups
        .iter()
        .enumerate()
        .map(|(i, &g)| (g, i))
        .collect();
    let mut triplets = Vec::new();
    let mut n_rows = 0;

    let n_mcast_eligible = mcast_eligible.len();
    let regular_vars_count = n_commodities * n_links;

    for demand in demands.iter().filter(|d| d.multicast) {
        let k = *k_of_type.get(&demand.kind).ok_or_else(|| {
            ShapleyError::MatrixConstructionError(format!(
                "Commodity kind {} not found",
                demand.kind
            ))
        })?;
        let k_orig_idx = *k_multicast.get(&demand.original).ok_or_else(|| {
            ShapleyError::MatrixConstructionError(format!(
                "Multicast group {} not found",
                demand.original
            ))
        })?;

        let receivers = demand.receivers as f64;
        if receivers.abs() < 1e-9 {
            continue; // Avoid division by zero
        }

        // Add one constraint row for each multicast-eligible link for this demand
        for (mcast_col_idx, &link_idx) in mcast_eligible.iter().enumerate() {
            let row_idx = n_rows;

            // Part 1: (1/receivers) * x_k
            let regular_var_col_idx = k * n_links + link_idx;
            triplets.push((row_idx, regular_var_col_idx, 1.0 / receivers));

            // Part 2: -1 * x_orig
            let aux_var_col_idx =
                regular_vars_count + (k_orig_idx * n_mcast_eligible) + mcast_col_idx;
            triplets.push((row_idx, aux_var_col_idx, -1.0));

            n_rows += 1;
        }
    }

    build_csc_from_triplets(&triplets, n_rows, n_total_cols)
}

/// Filter columns of a CSC matrix
fn filter_columns(matrix: &CscMatrix<f64>, keep: &[usize]) -> Result<CscMatrix<f64>> {
    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    for &col in keep {
        if col >= matrix.n {
            return Err(ShapleyError::MatrixConstructionError(format!(
                "Column index {col} out of bounds",
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

/// Build column operator tags for operator1
fn build_column_operators1(
    links: &[ConsolidatedLink],
    commodities: &[u32],
    _multicast_commodities: &[u32],
    mcast_eligible: &[usize],
    keep: &[usize],
    n_multicast_groups: usize,
) -> Vec<String> {
    let mut col_op = Vec::new();

    // Regular commodity columns
    for _ in commodities {
        for link in links {
            col_op.push(link.operator1.clone());
        }
    }

    // Multicast auxiliary variable columns
    for _ in 0..n_multicast_groups {
        for &idx in mcast_eligible {
            if idx < links.len() {
                col_op.push(links[idx].operator1.clone());
            }
        }
    }

    // Filter by keep indices
    let result: Vec<String> = keep
        .iter()
        .filter_map(|&i| col_op.get(i).cloned())
        .collect();

    result
}

/// Build column operator tags for operator2
fn build_column_operators2(
    links: &[ConsolidatedLink],
    commodities: &[u32],
    _multicast_commodities: &[u32],
    mcast_eligible: &[usize],
    keep: &[usize],
    n_multicast_groups: usize,
) -> Vec<String> {
    let mut col_op = Vec::new();

    // Regular commodity columns
    for _ in commodities {
        for link in links {
            col_op.push(link.operator2.clone());
        }
    }

    // Multicast auxiliary variable columns
    for _ in 0..n_multicast_groups {
        for &idx in mcast_eligible {
            if idx < links.len() {
                col_op.push(links[idx].operator2.clone());
            }
        }
    }

    // Filter by keep indices
    keep.iter()
        .filter_map(|&i| col_op.get(i).cloned())
        .collect()
}

/// Build RHS vector for flow requirements
fn build_flow_requirements(
    demands: &[ConsolidatedDemand],
    commodities: &[u32],
    k_of_type: &HashMap<u32, usize>,
    node_idx: &HashMap<&str, usize>,
    n_nodes: usize,
) -> Result<Vec<f64>> {
    let mut b_eq = vec![0.0; n_nodes * commodities.len()];

    for &t in commodities {
        let k = *k_of_type.get(&t).ok_or_else(|| {
            ShapleyError::MatrixConstructionError(format!("Commodity type {t} not found"))
        })?;

        let offset = k * n_nodes;

        for demand in demands.iter().filter(|d| d.kind == t) {
            let qty = demand.traffic * demand.receivers as f64;

            let src_idx = *node_idx.get(demand.start.as_str()).ok_or_else(|| {
                ShapleyError::MatrixConstructionError(format!(
                    "Source node {} not found",
                    demand.start
                ))
            })?;

            let dst_idx = *node_idx.get(demand.end.as_str()).ok_or_else(|| {
                ShapleyError::MatrixConstructionError(format!(
                    "Destination node {} not found",
                    demand.end
                ))
            })?;

            b_eq[offset + src_idx] += qty;
            b_eq[offset + dst_idx] -= qty;
        }
    }

    Ok(b_eq)
}

/// Build objective function coefficients
fn build_objective_coefficients(
    links: &[ConsolidatedLink],
    demands: &[ConsolidatedDemand],
    commodities: &[u32],
    _multicast_commodities: &[u32],
    mcast_eligible: &[usize],
    keep: &[usize],
    n_multicast_groups: usize,
) -> Result<Vec<f64>> {
    // Compute average priority for each commodity type
    let mut priority_by_type: BTreeMap<u32, (f64, usize)> = BTreeMap::new();

    for demand in demands {
        let entry = priority_by_type.entry(demand.kind).or_insert((0.0, 0));
        entry.0 += demand.priority;
        entry.1 += 1;
    }

    let avg_priority: BTreeMap<u32, f64> = priority_by_type
        .into_iter()
        .map(|(k, (sum, count))| (k, sum / count as f64))
        .collect();

    // Build cost vector
    let mut cost = Vec::new();

    // Regular commodity costs
    for &t in commodities {
        let priority = avg_priority.get(&t).copied().unwrap_or(1.0);

        for link in links {
            let latency = link.latency;
            cost.push(latency * priority);
        }
    }

    // Multicast auxiliary variable costs (zero)
    let multicast_cost_size = n_multicast_groups * mcast_eligible.len();
    cost.extend(vec![0.0; multicast_cost_size]);

    // Filter by keep indices
    Ok(keep.iter().filter_map(|&i| cost.get(i).copied()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_single_commodity_matrix() {
        let links = vec![
            ConsolidatedLink {
                device1: "A".to_string(),
                device2: "B".to_string(),
                latency: 1.0,
                bandwidth: 10.0,
                operator1: "Op1".to_string(),
                operator2: "Op1".to_string(),
                shared: 1,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "B".to_string(),
                device2: "C".to_string(),
                latency: 1.0,
                bandwidth: 10.0,
                operator1: "Op1".to_string(),
                operator2: "Op1".to_string(),
                shared: 1,
                link_type: 0,
            },
        ];

        let mut node_idx = HashMap::new();
        node_idx.insert("A", 0);
        node_idx.insert("B", 1);
        node_idx.insert("C", 2);

        let matrix = build_single_commodity_matrix(&links, &node_idx, 3)
            .expect("Matrix construction should succeed in tests");

        assert_eq!(matrix.m, 3);
        assert_eq!(matrix.n, 2);
        assert_eq!(matrix.nnz(), 4); // 2 entries per link
    }
}
