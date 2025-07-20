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
