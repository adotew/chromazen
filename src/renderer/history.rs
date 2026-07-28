use super::{
    DOCUMENT_FORMAT,
    layers::{LayerId, PaintLayer},
};

const HISTORY_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const BYTES_PER_PIXEL: u64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl TextureRect {
    pub(crate) fn from_inclusive(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        Self {
            x: min_x,
            y: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let max_x = (self.x + self.width).max(other.x + other.width);
        let max_y = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: max_x - x,
            height: max_y - y,
        }
    }

    fn at_origin(self) -> Self {
        Self { x: 0, y: 0, ..self }
    }

    fn extent(self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        }
    }

    fn byte_len(self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * BYTES_PER_PIXEL
    }
}

struct StrokeEntry {
    layer_id: LayerId,
    rect: TextureRect,
    pixels: wgpu::Texture,
    bytes: u64,
}

enum HistoryAction {
    Stroke(StrokeEntry),
    AddLayer {
        layer_id: LayerId,
        index: usize,
        selection_before: LayerId,
        selection_after: LayerId,
        detached: Option<PaintLayer>,
        bytes: u64,
    },
    DeleteLayer {
        layer_id: LayerId,
        index: usize,
        selection_before: LayerId,
        selection_after: LayerId,
        detached: Option<PaintLayer>,
        bytes: u64,
    },
    BackgroundColor {
        before: [f32; 4],
        after: [f32; 4],
    },
    RenameLayer {
        layer_id: LayerId,
        before: String,
        after: String,
    },
    LayerVisibility {
        layer_id: LayerId,
        before: bool,
        after: bool,
    },
    LayerClipping {
        layer_id: LayerId,
        before: bool,
        after: bool,
    },
    LayerOpacity {
        layer_id: LayerId,
        before: u8,
        after: u8,
    },
    MoveLayer {
        layer_id: LayerId,
        before: usize,
        after: usize,
    },
    MergeDown {
        upper_id: LayerId,
        lower_id: LayerId,
        lower_index: usize,
        selection_before: LayerId,
        detached_upper: Option<Box<PaintLayer>>,
        detached_lower: Option<Box<PaintLayer>>,
        detached_merged: Option<Box<PaintLayer>>,
        bytes: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryTarget {
    Stroke(LayerId),
    Structure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StructureEffect {
    MetadataOnly,
    LayerAdded(LayerId),
    LayerRemoved(LayerId),
    LayersMerged { result: LayerId, removed: LayerId },
    MergeUndone { lower: LayerId, upper: LayerId },
}

impl HistoryAction {
    fn bytes(&self) -> u64 {
        match self {
            Self::Stroke(entry) => entry.bytes,
            Self::AddLayer { bytes, .. }
            | Self::DeleteLayer { bytes, .. }
            | Self::MergeDown { bytes, .. } => *bytes,
            Self::BackgroundColor { .. }
            | Self::RenameLayer { .. }
            | Self::LayerVisibility { .. }
            | Self::LayerClipping { .. }
            | Self::LayerOpacity { .. }
            | Self::MoveLayer { .. } => 0,
        }
    }

    fn target(&self) -> HistoryTarget {
        match self {
            Self::Stroke(entry) => HistoryTarget::Stroke(entry.layer_id),
            Self::AddLayer { .. }
            | Self::DeleteLayer { .. }
            | Self::BackgroundColor { .. }
            | Self::RenameLayer { .. }
            | Self::LayerVisibility { .. }
            | Self::LayerClipping { .. }
            | Self::LayerOpacity { .. }
            | Self::MoveLayer { .. }
            | Self::MergeDown { .. } => HistoryTarget::Structure,
        }
    }
}

pub(crate) struct PaintHistory {
    actions: Vec<HistoryAction>,
    cursor: usize,
    mirror: wgpu::Texture,
    mirrored_layer: Option<LayerId>,
    active_stroke: Option<LayerId>,
}

impl PaintHistory {
    pub(crate) fn new(device: &wgpu::Device, document_size: [u32; 2]) -> Self {
        Self {
            actions: Vec::new(),
            cursor: 0,
            mirror: create_texture(device, "paint history mirror", document_size),
            mirrored_layer: None,
            active_stroke: None,
        }
    }

    pub(crate) fn begin_stroke(&mut self, layer_id: LayerId) -> bool {
        if self.active_stroke.is_some() {
            return false;
        }
        self.active_stroke = Some(layer_id);
        true
    }

    pub(crate) fn end_empty_stroke(&mut self) {
        self.active_stroke = None;
    }

    pub(crate) fn clear(&mut self) {
        self.actions.clear();
        self.cursor = 0;
        self.mirrored_layer = None;
        self.active_stroke = None;
    }

    pub(crate) fn stroke_active(&self) -> bool {
        self.active_stroke.is_some()
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.stroke_active() && self.cursor > 0
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.active_stroke.is_none() && self.cursor < self.actions.len()
    }

    pub(crate) fn undo_target(&self) -> Option<HistoryTarget> {
        self.can_undo()
            .then(|| self.actions[self.cursor - 1].target())
    }

    pub(crate) fn redo_target(&self) -> Option<HistoryTarget> {
        self.can_redo().then(|| self.actions[self.cursor].target())
    }

    pub(crate) fn layer_needs_sync(&self, layer_id: LayerId) -> bool {
        self.mirrored_layer != Some(layer_id)
    }

    pub(crate) fn ensure_layer_synced(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer_id: LayerId,
        canvas: &wgpu::Texture,
        document_size: [u32; 2],
    ) {
        if !self.layer_needs_sync(layer_id) {
            return;
        }
        self.sync_layer(
            encoder,
            layer_id,
            canvas,
            TextureRect {
                x: 0,
                y: 0,
                width: document_size[0],
                height: document_size[1],
            },
        );
    }

    pub(crate) fn sync_layer(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer_id: LayerId,
        canvas: &wgpu::Texture,
        rect: TextureRect,
    ) {
        copy_rect(encoder, canvas, rect, &self.mirror, [rect.x, rect.y]);
        self.mirrored_layer = Some(layer_id);
    }

    pub(crate) fn commit_stroke(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layer_id: LayerId,
        canvas: &wgpu::Texture,
        rect: TextureRect,
    ) {
        debug_assert_eq!(self.active_stroke, Some(layer_id));
        self.discard_redo();

        let pixels = create_texture(device, "paint history entry", [rect.width, rect.height]);
        copy_rect(encoder, &self.mirror, rect, &pixels, [0, 0]);
        self.sync_layer(encoder, layer_id, canvas, rect);

        self.actions.push(HistoryAction::Stroke(StrokeEntry {
            layer_id,
            rect,
            pixels,
            bytes: rect.byte_len(),
        }));
        self.cursor = self.actions.len();
        self.evict_to_budget();
        self.active_stroke = None;
    }

    pub(crate) fn record_add(
        &mut self,
        layer_id: LayerId,
        index: usize,
        selection_before: LayerId,
        layer_bytes: u64,
    ) {
        self.discard_redo();
        self.actions.push(HistoryAction::AddLayer {
            layer_id,
            index,
            selection_before,
            selection_after: layer_id,
            detached: None,
            bytes: layer_bytes,
        });
        self.cursor = self.actions.len();
        self.evict_to_budget();
    }

    pub(crate) fn record_delete(
        &mut self,
        layer: PaintLayer,
        index: usize,
        selection_before: LayerId,
        selection_after: LayerId,
        layer_bytes: u64,
    ) {
        let layer_id = layer.id;
        self.discard_redo();
        self.actions.push(HistoryAction::DeleteLayer {
            layer_id,
            index,
            selection_before,
            selection_after,
            detached: Some(layer),
            bytes: layer_bytes,
        });
        self.cursor = self.actions.len();
        self.evict_to_budget();
    }

    pub(crate) fn record_merge_down(
        &mut self,
        upper: PaintLayer,
        lower: PaintLayer,
        lower_index: usize,
        selection_before: LayerId,
        layer_bytes: u64,
    ) {
        let upper_id = upper.id;
        let lower_id = lower.id;
        self.discard_redo();
        self.actions.push(HistoryAction::MergeDown {
            upper_id,
            lower_id,
            lower_index,
            selection_before,
            detached_upper: Some(Box::new(upper)),
            detached_lower: Some(Box::new(lower)),
            detached_merged: None,
            bytes: layer_bytes.saturating_mul(2),
        });
        self.cursor = self.actions.len();
        self.evict_to_budget();
    }

    pub(crate) fn record_background_color(&mut self, before: [f32; 4], after: [f32; 4]) {
        if before == after {
            return;
        }
        self.push_structure(HistoryAction::BackgroundColor { before, after });
    }

    pub(crate) fn record_rename_layer(&mut self, layer_id: LayerId, before: String, after: String) {
        if before != after {
            self.push_structure(HistoryAction::RenameLayer {
                layer_id,
                before,
                after,
            });
        }
    }

    pub(crate) fn record_layer_visibility(&mut self, layer_id: LayerId, before: bool, after: bool) {
        if before != after {
            self.push_structure(HistoryAction::LayerVisibility {
                layer_id,
                before,
                after,
            });
        }
    }

    pub(crate) fn record_layer_clipping(&mut self, layer_id: LayerId, before: bool, after: bool) {
        if before != after {
            self.push_structure(HistoryAction::LayerClipping {
                layer_id,
                before,
                after,
            });
        }
    }

    pub(crate) fn record_layer_opacity(&mut self, layer_id: LayerId, before: u8, after: u8) {
        if before != after {
            self.push_structure(HistoryAction::LayerOpacity {
                layer_id,
                before,
                after,
            });
        }
    }

    pub(crate) fn record_move_layer(&mut self, layer_id: LayerId, before: usize, after: usize) {
        if before != after {
            self.push_structure(HistoryAction::MoveLayer {
                layer_id,
                before,
                after,
            });
        }
    }

    fn push_structure(&mut self, action: HistoryAction) {
        self.discard_redo();
        self.actions.push(action);
        self.cursor = self.actions.len();
        self.evict_to_budget();
    }

    pub(crate) fn undo_stroke(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
    ) -> bool {
        if !matches!(self.undo_target(), Some(HistoryTarget::Stroke(_))) {
            return false;
        }
        self.cursor -= 1;
        let HistoryAction::Stroke(entry) = &self.actions[self.cursor] else {
            unreachable!();
        };
        swap_entry(encoder, &self.mirror, canvas, entry);
        self.mirrored_layer = Some(entry.layer_id);
        true
    }

    pub(crate) fn redo_stroke(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
    ) -> bool {
        if !matches!(self.redo_target(), Some(HistoryTarget::Stroke(_))) {
            return false;
        }
        let HistoryAction::Stroke(entry) = &self.actions[self.cursor] else {
            unreachable!();
        };
        swap_entry(encoder, &self.mirror, canvas, entry);
        self.mirrored_layer = Some(entry.layer_id);
        self.cursor += 1;
        true
    }

    pub(crate) fn undo_structure(
        &mut self,
        layers: &mut Vec<PaintLayer>,
        selection: &mut LayerId,
        background_color: &mut [f32; 4],
    ) -> Option<StructureEffect> {
        if self.undo_target() != Some(HistoryTarget::Structure) {
            return None;
        }
        self.cursor -= 1;
        self.mirrored_layer = None;
        let effect = match &mut self.actions[self.cursor] {
            HistoryAction::AddLayer {
                layer_id,
                selection_before,
                detached,
                ..
            } => {
                let index = layers
                    .iter()
                    .position(|layer| layer.id == *layer_id)
                    .expect("added layer must exist before undo");
                *detached = Some(layers.remove(index));
                *selection = *selection_before;
                StructureEffect::LayerRemoved(*layer_id)
            }
            HistoryAction::DeleteLayer {
                index,
                selection_before,
                detached,
                ..
            } => {
                let layer = detached.take().expect("deleted layer must be retained");
                let layer_id = layer.id;
                layers.insert((*index).min(layers.len()), layer);
                *selection = *selection_before;
                StructureEffect::LayerAdded(layer_id)
            }
            HistoryAction::BackgroundColor { before, .. } => {
                *background_color = *before;
                StructureEffect::MetadataOnly
            }
            HistoryAction::RenameLayer {
                layer_id, before, ..
            } => {
                layer_mut(layers, *layer_id).name.clone_from(before);
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerVisibility {
                layer_id, before, ..
            } => {
                layer_mut(layers, *layer_id).visible = *before;
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerClipping {
                layer_id, before, ..
            } => {
                layer_mut(layers, *layer_id).clipped = *before;
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerOpacity {
                layer_id, before, ..
            } => {
                layer_mut(layers, *layer_id).opacity = *before;
                StructureEffect::MetadataOnly
            }
            HistoryAction::MoveLayer {
                layer_id, before, ..
            } => {
                move_layer_to_index(layers, *layer_id, *before);
                StructureEffect::MetadataOnly
            }
            HistoryAction::MergeDown {
                upper_id,
                lower_id,
                lower_index,
                selection_before,
                detached_upper,
                detached_lower,
                detached_merged,
                ..
            } => {
                let merged_index = layers
                    .iter()
                    .position(|layer| layer.id == *lower_id)
                    .expect("merged layer must exist before undo");
                *detached_merged = Some(Box::new(layers.remove(merged_index)));
                let lower = *detached_lower.take().expect("lower layer must be retained");
                let upper = *detached_upper.take().expect("upper layer must be retained");
                layers.insert((*lower_index).min(layers.len()), lower);
                layers.insert((*lower_index + 1).min(layers.len()), upper);
                *selection = *selection_before;
                StructureEffect::MergeUndone {
                    lower: *lower_id,
                    upper: *upper_id,
                }
            }
            HistoryAction::Stroke(_) => unreachable!(),
        };
        Some(effect)
    }

    pub(crate) fn redo_structure(
        &mut self,
        layers: &mut Vec<PaintLayer>,
        selection: &mut LayerId,
        background_color: &mut [f32; 4],
    ) -> Option<StructureEffect> {
        if self.redo_target() != Some(HistoryTarget::Structure) {
            return None;
        }
        self.mirrored_layer = None;
        let effect = match &mut self.actions[self.cursor] {
            HistoryAction::AddLayer {
                index,
                selection_after,
                detached,
                ..
            } => {
                let layer = detached
                    .take()
                    .expect("undone added layer must be retained");
                let layer_id = layer.id;
                layers.insert((*index).min(layers.len()), layer);
                *selection = *selection_after;
                StructureEffect::LayerAdded(layer_id)
            }
            HistoryAction::DeleteLayer {
                layer_id,
                selection_after,
                detached,
                ..
            } => {
                let index = layers
                    .iter()
                    .position(|layer| layer.id == *layer_id)
                    .expect("restored deleted layer must exist before redo");
                *detached = Some(layers.remove(index));
                *selection = *selection_after;
                StructureEffect::LayerRemoved(*layer_id)
            }
            HistoryAction::BackgroundColor { after, .. } => {
                *background_color = *after;
                StructureEffect::MetadataOnly
            }
            HistoryAction::RenameLayer {
                layer_id, after, ..
            } => {
                layer_mut(layers, *layer_id).name.clone_from(after);
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerVisibility {
                layer_id, after, ..
            } => {
                layer_mut(layers, *layer_id).visible = *after;
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerClipping {
                layer_id, after, ..
            } => {
                layer_mut(layers, *layer_id).clipped = *after;
                StructureEffect::MetadataOnly
            }
            HistoryAction::LayerOpacity {
                layer_id, after, ..
            } => {
                layer_mut(layers, *layer_id).opacity = *after;
                StructureEffect::MetadataOnly
            }
            HistoryAction::MoveLayer {
                layer_id, after, ..
            } => {
                move_layer_to_index(layers, *layer_id, *after);
                StructureEffect::MetadataOnly
            }
            HistoryAction::MergeDown {
                upper_id,
                lower_id,
                lower_index,
                detached_upper,
                detached_lower,
                detached_merged,
                ..
            } => {
                let upper_index = layers
                    .iter()
                    .position(|layer| layer.id == *upper_id)
                    .expect("restored upper layer must exist before redo");
                *detached_upper = Some(Box::new(layers.remove(upper_index)));
                let current_lower_index = layers
                    .iter()
                    .position(|layer| layer.id == *lower_id)
                    .expect("restored lower layer must exist before redo");
                *detached_lower = Some(Box::new(layers.remove(current_lower_index)));
                let merged = *detached_merged
                    .take()
                    .expect("merged layer must be retained");
                layers.insert((*lower_index).min(layers.len()), merged);
                *selection = *lower_id;
                StructureEffect::LayersMerged {
                    result: *lower_id,
                    removed: *upper_id,
                }
            }
            HistoryAction::Stroke(_) => unreachable!(),
        };
        self.cursor += 1;
        Some(effect)
    }

    fn discard_redo(&mut self) {
        self.actions.truncate(self.cursor);
    }

    fn evict_to_budget(&mut self) {
        let used_bytes = self.actions.iter().map(HistoryAction::bytes).sum();
        let count = eviction_count(
            used_bytes,
            HISTORY_BUDGET_BYTES,
            self.actions.len(),
            self.actions.iter().map(HistoryAction::bytes),
        );
        self.actions.drain(..count);
        self.cursor -= count;
    }
}

fn layer_mut(layers: &mut [PaintLayer], id: LayerId) -> &mut PaintLayer {
    layers
        .iter_mut()
        .find(|layer| layer.id == id)
        .expect("history layer must exist")
}

fn move_layer_to_index(layers: &mut Vec<PaintLayer>, id: LayerId, index: usize) {
    let current = layers
        .iter()
        .position(|layer| layer.id == id)
        .expect("moved history layer must exist");
    let layer = layers.remove(current);
    layers.insert(index.min(layers.len()), layer);
}

fn swap_entry(
    encoder: &mut wgpu::CommandEncoder,
    mirror: &wgpu::Texture,
    canvas: &wgpu::Texture,
    entry: &StrokeEntry,
) {
    copy_rect(
        encoder,
        &entry.pixels,
        entry.rect.at_origin(),
        canvas,
        [entry.rect.x, entry.rect.y],
    );
    copy_rect(encoder, mirror, entry.rect, &entry.pixels, [0, 0]);
    copy_rect(
        encoder,
        canvas,
        entry.rect,
        mirror,
        [entry.rect.x, entry.rect.y],
    );
}

fn eviction_count(
    mut used_bytes: u64,
    budget: u64,
    entry_count: usize,
    oldest_bytes: impl Iterator<Item = u64>,
) -> usize {
    let mut count = 0;
    for bytes in oldest_bytes.take(entry_count.saturating_sub(1)) {
        if used_bytes <= budget {
            break;
        }
        used_bytes -= bytes;
        count += 1;
    }
    count
}

fn create_texture(device: &wgpu::Device, label: &str, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DOCUMENT_FORMAT,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn copy_rect(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    source_rect: TextureRect,
    destination: &wgpu::Texture,
    destination_origin: [u32; 2],
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: source_rect.x,
                y: source_rect.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: destination_origin[0],
                y: destination_origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        source_rect.extent(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inclusive_bounds_have_nonzero_copy_dimensions() {
        assert_eq!(
            TextureRect::from_inclusive(10, 20, 10, 20),
            TextureRect {
                x: 10,
                y: 20,
                width: 1,
                height: 1,
            }
        );
    }

    #[test]
    fn rectangles_union() {
        assert_eq!(
            TextureRect {
                x: 10,
                y: 20,
                width: 5,
                height: 10,
            }
            .union(TextureRect {
                x: 2,
                y: 25,
                width: 10,
                height: 10,
            }),
            TextureRect {
                x: 2,
                y: 20,
                width: 13,
                height: 15,
            }
        );
    }

    #[test]
    fn budget_evicts_oldest_entries_and_keeps_newest() {
        assert_eq!(eviction_count(300, 200, 3, [100, 100, 100].into_iter()), 1);
        assert_eq!(eviction_count(300, 50, 1, [300].into_iter()), 0);
    }

    #[test]
    fn metadata_changes_are_structural_and_use_no_texture_budget() {
        let actions = [
            HistoryAction::BackgroundColor {
                before: [1.0; 4],
                after: [0.0, 0.0, 0.0, 1.0],
            },
            HistoryAction::RenameLayer {
                layer_id: LayerId(1),
                before: "Before".to_owned(),
                after: "After".to_owned(),
            },
            HistoryAction::LayerVisibility {
                layer_id: LayerId(1),
                before: true,
                after: false,
            },
            HistoryAction::LayerClipping {
                layer_id: LayerId(1),
                before: false,
                after: true,
            },
            HistoryAction::LayerOpacity {
                layer_id: LayerId(1),
                before: 100,
                after: 50,
            },
            HistoryAction::MoveLayer {
                layer_id: LayerId(1),
                before: 0,
                after: 1,
            },
        ];
        for action in actions {
            assert_eq!(action.target(), HistoryTarget::Structure);
            assert_eq!(action.bytes(), 0);
        }
    }
}
