#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayerId(pub(crate) u64);

/// Identifies one allocation of a layer's GPU resources. Unlike `LayerId`, this
/// value is never reused when a document is reset or replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayerResourceId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropEdge {
    Above,
    Below,
}

pub(crate) struct LayerProperties {
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
}

impl LayerProperties {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            visible: true,
            opacity: 100,
            clipped: false,
        }
    }
}

pub(crate) struct PaintLayer {
    pub(crate) id: LayerId,
    pub(crate) resource_id: LayerResourceId,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
    pub(crate) settings_buffer: wgpu::Buffer,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) blit_bind_group: wgpu::BindGroup,
    // Keep the allocation alongside its view so history owns the complete preview resource.
    pub(crate) _preview_texture: wgpu::Texture,
    pub(crate) preview_view: wgpu::TextureView,
    pub(crate) preview_bind_group: wgpu::BindGroup,
    pub(crate) preview_dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayerInfo {
    pub(crate) id: LayerId,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerSnapshot {
    pub(crate) layers: Vec<LayerInfo>,
    pub(crate) selection: LayerId,
    pub(crate) background_color: [f32; 4],
}

pub(crate) fn insertion_index(selected_index: Option<usize>, layer_count: usize) -> usize {
    selected_index.map_or(layer_count, |index| index + 1)
}

pub(crate) fn merge_down_target_index(layer_index: usize) -> Option<usize> {
    layer_index.checked_sub(1)
}

pub(crate) fn replacement_index_after_delete(
    layer_count: usize,
    deleted_index: usize,
) -> Option<usize> {
    (layer_count > 1).then(|| {
        if deleted_index > 0 {
            deleted_index - 1
        } else {
            1
        }
    })
}

pub(crate) fn layer_name(number: u64) -> String {
    format!("Layer {number}")
}

pub(crate) fn normalized_layer_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

pub(crate) fn relative_insertion_index(
    dragged_index: usize,
    target_index: usize,
    edge: DropEdge,
) -> Option<usize> {
    if dragged_index == target_index {
        return None;
    }
    let target_after_removal = target_index - usize::from(dragged_index < target_index);
    let insertion = match edge {
        // The internal vector is bottom-to-top, opposite the visual layer list.
        DropEdge::Above => target_after_removal + 1,
        DropEdge::Below => target_after_removal,
    };
    (insertion != dragged_index).then_some(insertion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_above_selection() {
        assert_eq!(insertion_index(Some(1), 3), 2);
        assert_eq!(insertion_index(None, 3), 3);
    }

    #[test]
    fn merge_down_targets_the_next_lower_internal_layer() {
        assert_eq!(merge_down_target_index(2), Some(1));
        assert_eq!(merge_down_target_index(1), Some(0));
        assert_eq!(merge_down_target_index(0), None);
    }

    #[test]
    fn deletion_prefers_layer_below() {
        assert_eq!(replacement_index_after_delete(3, 2), Some(1));
        assert_eq!(replacement_index_after_delete(3, 0), Some(1));
        assert_eq!(replacement_index_after_delete(1, 0), None);
    }

    #[test]
    fn names_are_monotonic() {
        assert_eq!(layer_name(1), "Layer 1");
        assert_eq!(layer_name(42), "Layer 42");
    }

    #[test]
    fn layer_names_are_trimmed_and_must_not_be_empty() {
        assert_eq!(
            normalized_layer_name("  Sketch  "),
            Some("Sketch".to_owned())
        );
        assert_eq!(normalized_layer_name("   "), None);
    }

    #[test]
    fn relative_moves_translate_visual_edges_to_internal_order() {
        assert_eq!(relative_insertion_index(0, 1, DropEdge::Above), Some(1));
        assert_eq!(relative_insertion_index(2, 1, DropEdge::Below), Some(1));
        assert_eq!(relative_insertion_index(0, 1, DropEdge::Below), None);
        assert_eq!(relative_insertion_index(1, 1, DropEdge::Above), None);
    }
}
