use std::ops::Deref;

use sprs::{CsVecBase, CsVecView};

/// Return a view of a sparse vector truncated to `len` elements.
///
/// Any entries whose index is >= `len` are dropped from the view.
/// This is used when a vector needs to be projected into a smaller
/// dimension (e.g. after resizing the constraint matrix).
pub(crate) fn resized_view<IStorage, DStorage>(
    vec: &CsVecBase<IStorage, DStorage, f64>,
    len: usize,
) -> CsVecView<'_, f64>
where
    IStorage: Deref<Target = [usize]>,
    DStorage: Deref<Target = [f64]>,
{
    let mut indices = vec.indices();
    let mut data = vec.data();
    while let Some(&i) = indices.last() {
        if i < len {
            // TODO: binary search
            break;
        }

        indices = &indices[..(indices.len() - 1)];
        data = &data[..(data.len() - 1)];
    }

    CsVecView::new(len, indices, data)
}

/// Convert a sparse vector into a dense `Vec<f64>` (zeros where no entry exists).
pub(crate) fn to_dense<IStorage, DStorage>(vec: &CsVecBase<IStorage, DStorage, f64>) -> Vec<f64>
where
    IStorage: Deref<Target = [usize]>,
    DStorage: Deref<Target = [f64]>,
{
    let mut dense = vec![0.0; vec.dim()];
    vec.scatter(&mut dense);
    dense
}
