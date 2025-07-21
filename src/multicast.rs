use crate::{
    error::{Result, ShapleyError},
    types::ConsolidatedLink,
};
use clarabel::algebra::CscMatrix;

/// Build J1 matrix - all private links grouped by shared ID
pub(crate) fn build_j1_matrix(
    links: &[ConsolidatedLink],
    n_private: usize,
    max_shared: usize,
) -> Result<CscMatrix<f64>> {
    let n_links = links.len();
    let mut triplets = Vec::new();

    // J1 includes all private links (first n_private links)
    for (col, link) in links[..n_private].iter().enumerate() {
        if link.shared > 0 && link.shared as usize <= max_shared {
            // Row index is shared_id - 1 (0-based)
            triplets.push((link.shared as usize - 1, col, 1.0));
        }
    }

    build_csc_from_triplets(&triplets, max_shared, n_links)
}

/// Build J2 matrix - only multicast ineligible links grouped by shared ID
pub(crate) fn build_j2_matrix(
    links: &[ConsolidatedLink],
    mcast_ineligible: &[usize],
    max_shared: usize,
) -> Result<CscMatrix<f64>> {
    let n_links = links.len();
    let mut triplets = Vec::new();

    // J2 includes only multicast ineligible links
    for &idx in mcast_ineligible {
        if idx < links.len() {
            let link = &links[idx];
            if link.shared > 0 && link.shared as usize <= max_shared {
                triplets.push((link.shared as usize - 1, idx, 1.0));
            }
        }
    }

    build_csc_from_triplets(&triplets, max_shared, n_links)
}

/// Compute (J1 - J2) matrix for multicast constraints
pub(crate) fn compute_j1_minus_j2(
    j1: &CscMatrix<f64>,
    j2: &CscMatrix<f64>,
) -> Result<CscMatrix<f64>> {
    if j1.m != j2.m || j1.n != j2.n {
        return Err(ShapleyError::MatrixConstructionError(
            "J1 and J2 dimensions must match".to_string(),
        ));
    }

    // Build triplets for the difference
    let mut triplets = Vec::new();

    // Add J1 entries
    for col in 0..j1.n {
        let start = j1.colptr[col];
        let end = j1.colptr[col + 1];

        for idx in start..end {
            let row = j1.rowval[idx];
            let val = j1.nzval[idx];
            triplets.push((row, col, val));
        }
    }

    // Subtract J2 entries
    for col in 0..j2.n {
        let start = j2.colptr[col];
        let end = j2.colptr[col + 1];

        for idx in start..end {
            let row = j2.rowval[idx];
            let val = j2.nzval[idx];
            // Find if this (row, col) exists in triplets and subtract
            let mut found = false;
            for triplet in &mut triplets {
                if triplet.0 == row && triplet.1 == col {
                    triplet.2 -= val;
                    found = true;
                    break;
                }
            }
            if !found {
                triplets.push((row, col, -val));
            }
        }
    }

    // Remove zero entries
    triplets.retain(|&(_, _, val)| val.abs() > 1e-10);

    build_csc_from_triplets(&triplets, j1.m, j1.n)
}

/// Extract columns from a matrix for multicast eligible links
pub(crate) fn extract_mcast_eligible_columns(
    matrix: &CscMatrix<f64>,
    mcast_eligible: &[usize],
) -> Result<CscMatrix<f64>> {
    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    for &col in mcast_eligible {
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
        mcast_eligible.len(),
        col_ptr,
        row_ind,
        values,
    ))
}

/// Build CSC matrix from triplets (helper function)
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

/// Horizontally stack matrices
pub(crate) fn hstack_matrices(matrices: &[&CscMatrix<f64>]) -> Result<CscMatrix<f64>> {
    if matrices.is_empty() {
        return Err(ShapleyError::MatrixConstructionError(
            "Cannot stack empty matrix list".to_string(),
        ));
    }

    let n_rows = matrices[0].m;

    // Check all matrices have same number of rows
    for matrix in matrices {
        if matrix.m != n_rows {
            return Err(ShapleyError::MatrixConstructionError(
                "All matrices must have same number of rows".to_string(),
            ));
        }
    }

    let mut col_ptr = vec![0];
    let mut row_ind = Vec::new();
    let mut values = Vec::new();

    for &matrix in matrices {
        for col in 0..matrix.n {
            let start = matrix.colptr[col];
            let end = matrix.colptr[col + 1];

            for idx in start..end {
                row_ind.push(matrix.rowval[idx]);
                values.push(matrix.nzval[idx]);
            }

            col_ptr.push(row_ind.len());
        }
    }

    let total_cols = matrices.iter().map(|m| m.n).sum();

    Ok(CscMatrix::new(n_rows, total_cols, col_ptr, row_ind, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConsolidatedLink;

    #[test]
    fn test_build_j2_matrix_empty_ineligible() {
        let links = vec![
            ConsolidatedLink {
                device1: "A".to_string(),
                device2: "B".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op1".to_string(),
                operator2: "Op1".to_string(),
                shared: 1,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "B".to_string(),
                device2: "C".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op2".to_string(),
                operator2: "Op2".to_string(),
                shared: 2,
                link_type: 0,
            },
        ];

        // Empty multicast ineligible list
        let mcast_ineligible: Vec<usize> = vec![];
        let max_shared = 2;

        let j2 = build_j2_matrix(&links, &mcast_ineligible, max_shared).unwrap();

        // J2 should be a zero matrix when no links are ineligible
        assert_eq!(j2.nnz(), 0);
        assert_eq!(j2.m, max_shared);
        assert_eq!(j2.n, links.len());
    }

    #[test]
    fn test_build_j2_matrix_all_ineligible() {
        let links = vec![
            ConsolidatedLink {
                device1: "A".to_string(),
                device2: "B".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op1".to_string(),
                operator2: "Op1".to_string(),
                shared: 1,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "B".to_string(),
                device2: "C".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op2".to_string(),
                operator2: "Op2".to_string(),
                shared: 2,
                link_type: 0,
            },
        ];

        // All links are multicast ineligible
        let mcast_ineligible = vec![0, 1];
        let max_shared = 2;

        let j2 = build_j2_matrix(&links, &mcast_ineligible, max_shared).unwrap();

        // J2 should have entries for all ineligible links
        assert_eq!(j2.nnz(), 2);
        assert_eq!(j2.m, max_shared);
        assert_eq!(j2.n, links.len());
    }

    #[test]
    fn test_compute_j1_minus_j2_subtraction() {
        let links = vec![
            ConsolidatedLink {
                device1: "A".to_string(),
                device2: "B".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op1".to_string(),
                operator2: "Op1".to_string(),
                shared: 1,
                link_type: 0,
            },
            ConsolidatedLink {
                device1: "B".to_string(),
                device2: "C".to_string(),
                latency: 0.0,
                bandwidth: 10.0,
                operator1: "Op2".to_string(),
                operator2: "Op2".to_string(),
                shared: 2,
                link_type: 0,
            },
        ];

        let n_private = 2;
        let mcast_ineligible = vec![0]; // First link is ineligible
        let max_shared = 2;

        let j1 = build_j1_matrix(&links, n_private, max_shared).unwrap();
        let j2 = build_j2_matrix(&links, &mcast_ineligible, max_shared).unwrap();
        let result = compute_j1_minus_j2(&j1, &j2).unwrap();

        // The result should have J1 entries minus J2 entries
        // J1 has all private links, J2 has only the first link
        // So the difference should have an entry for the second link only
        assert_eq!(result.m, max_shared);
        assert_eq!(result.n, links.len());
    }

    #[test]
    fn test_compute_j1_minus_j2_error_propagation() {
        let links = vec![ConsolidatedLink {
            device1: "A".to_string(),
            device2: "B".to_string(),
            latency: 0.0,
            bandwidth: 10.0,
            operator1: "Op1".to_string(),
            operator2: "Op1".to_string(),
            shared: 3, // Shared ID exceeds max_shared
            link_type: 0,
        }];

        let n_private = 1;
        let max_shared = 2; // Too small for shared ID 3

        // Build J1 - it should succeed but skip the link with invalid shared ID
        let j1_result = build_j1_matrix(&links, n_private, max_shared);

        assert!(j1_result.is_ok());
        let j1 = j1_result.unwrap();
        // The matrix should have no entries since the link was skipped
        assert_eq!(j1.nnz(), 0);
    }

    #[test]
    fn test_concatenate_horizontal_mismatched_rows() {
        // Create two matrices with different number of rows
        let matrix1 = CscMatrix::<f64>::from(&[[1.0, 2.0], [3.0, 4.0]]);

        let matrix2 = CscMatrix::<f64>::from(&[
            [5.0],
            [6.0],
            [7.0], // Extra row
        ]);

        let result = hstack_matrices(&[&matrix1, &matrix2]);

        assert!(result.is_err());
        match result.unwrap_err() {
            ShapleyError::MatrixConstructionError(msg) => {
                assert!(msg.contains("same number of rows"));
            }
            _ => panic!("Expected MatrixConstructionError"),
        }
    }
}
