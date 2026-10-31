use super::{LAYER_PREVIEW_SIZE, tiles::TileSet};
use crate::BlendMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

/// Identifies one allocation of a layer's GPU resources. Unlike `LayerId`, this
/// value is never reused when a document is reset or replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerResourceId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEdge {
    Above,
    Below,
}

pub(crate) struct LayerProperties {
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
    pub(crate) blend_mode: BlendMode,
}

impl LayerProperties {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: BlendMode::Normal,
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
    pub(crate) blend_mode: BlendMode,
    pub(crate) settings_buffer: wgpu::Buffer,
    pub(crate) tiles: TileSet,
    pub(crate) blit_bind_group: wgpu::BindGroup,
    // Keep the allocation alongside its view so history owns the complete preview resource.
    pub(crate) _preview_texture: wgpu::Texture,
    pub(crate) preview_view: wgpu::TextureView,
    pub(crate) preview_dirty: bool,
}

impl PaintLayer {
    pub(crate) fn byte_len(&self) -> u64 {
        let preview = u64::from(LAYER_PREVIEW_SIZE) * u64::from(LAYER_PREVIEW_SIZE) * 4;
        self.tiles.byte_len() + preview
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub clipped: bool,
    pub blend_mode: BlendMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerSnapshot {
    pub layers: Vec<LayerInfo>,
    pub selection: LayerId,
    pub background_color: [f32; 4],
}

pub(crate) fn insertion_index(selected_index: Option<usize>, layer_count: usize) -> usize {
    selected_index.map_or(layer_count, |index| index + 1)
}

pub fn merge_down_target_index(layer_index: usize) -> Option<usize> {
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
mod tests;
