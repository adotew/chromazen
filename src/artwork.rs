mod format;
mod raster;
mod store;

pub(crate) use format::{DOCUMENT_SCHEMA_VERSION, DocumentManifest, LayerManifest};
pub(crate) use raster::{CompositeLayer, encode_png, flatten_premultiplied_layers};
pub(crate) use store::{
    ArtworkId, ArtworkStore, ArtworkSummary, LayerSource, LayerWrite, RevisionWrite,
};

/// Resolves a clipped layer to the nearest non-clipped layer below it.
pub(crate) fn clipping_base_index(clipped: &[bool], layer_index: usize) -> Option<usize> {
    if !clipped.get(layer_index).copied().unwrap_or(false) {
        return None;
    }
    (0..layer_index).rev().find(|index| !clipped[*index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_chains_resolve_to_the_nearest_unclipped_base() {
        let clipped = [false, true, true, false, true];
        assert_eq!(clipping_base_index(&clipped, 0), None);
        assert_eq!(clipping_base_index(&clipped, 1), Some(0));
        assert_eq!(clipping_base_index(&clipped, 2), Some(0));
        assert_eq!(clipping_base_index(&clipped, 3), None);
        assert_eq!(clipping_base_index(&clipped, 4), Some(3));
        assert_eq!(clipping_base_index(&[true], 0), None);
    }
}
