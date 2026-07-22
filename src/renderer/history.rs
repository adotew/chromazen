use super::{
    layers::{LayerId, LayerSelection, PaintLayer},
    DOCUMENT_FORMAT,
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
        selection_before: LayerSelection,
        selection_after: LayerSelection,
        detached: Option<PaintLayer>,
        bytes: u64,
    },
    DeleteLayer {
        layer_id: LayerId,
        index: usize,
        selection_before: LayerSelection,
        selection_after: LayerSelection,
        detached: Option<PaintLayer>,
        bytes: u64,
    },
    BackgroundColor {
        before: [f32; 4],
        after: [f32; 4],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryTarget {
    Stroke(LayerId),
    Structure,
}

impl HistoryAction {
    fn bytes(&self) -> u64 {
        match self {
            Self::Stroke(entry) => entry.bytes,
            Self::AddLayer { bytes, .. } | Self::DeleteLayer { bytes, .. } => *bytes,
            Self::BackgroundColor { .. } => 0,
        }
    }

    fn target(&self) -> HistoryTarget {
        match self {
            Self::Stroke(entry) => HistoryTarget::Stroke(entry.layer_id),
            Self::AddLayer { .. } | Self::DeleteLayer { .. } | Self::BackgroundColor { .. } => {
                HistoryTarget::Structure
            }
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
        selection_before: LayerSelection,
        layer_bytes: u64,
    ) {
        self.discard_redo();
        self.actions.push(HistoryAction::AddLayer {
            layer_id,
            index,
            selection_before,
            selection_after: LayerSelection::Paint(layer_id),
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
        selection_before: LayerSelection,
        selection_after: LayerSelection,
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

    pub(crate) fn record_background_color(&mut self, before: [f32; 4], after: [f32; 4]) {
        if before == after {
            return;
        }
        self.discard_redo();
        self.actions
            .push(HistoryAction::BackgroundColor { before, after });
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
        selection: &mut LayerSelection,
        background_color: &mut [f32; 4],
    ) -> bool {
        if self.undo_target() != Some(HistoryTarget::Structure) {
            return false;
        }
        self.cursor -= 1;
        match &mut self.actions[self.cursor] {
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
            }
            HistoryAction::DeleteLayer {
                index,
                selection_before,
                detached,
                ..
            } => {
                let layer = detached.take().expect("deleted layer must be retained");
                layers.insert((*index).min(layers.len()), layer);
                *selection = *selection_before;
            }
            HistoryAction::BackgroundColor { before, .. } => {
                *background_color = *before;
            }
            HistoryAction::Stroke(_) => unreachable!(),
        }
        true
    }

    pub(crate) fn redo_structure(
        &mut self,
        layers: &mut Vec<PaintLayer>,
        selection: &mut LayerSelection,
        background_color: &mut [f32; 4],
    ) -> bool {
        if self.redo_target() != Some(HistoryTarget::Structure) {
            return false;
        }
        match &mut self.actions[self.cursor] {
            HistoryAction::AddLayer {
                index,
                selection_after,
                detached,
                ..
            } => {
                let layer = detached
                    .take()
                    .expect("undone added layer must be retained");
                layers.insert((*index).min(layers.len()), layer);
                *selection = *selection_after;
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
            }
            HistoryAction::BackgroundColor { after, .. } => {
                *background_color = *after;
            }
            HistoryAction::Stroke(_) => unreachable!(),
        }
        self.cursor += 1;
        true
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
    fn background_changes_are_structural_and_use_no_texture_budget() {
        let action = HistoryAction::BackgroundColor {
            before: [1.0; 4],
            after: [0.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(action.target(), HistoryTarget::Structure);
        assert_eq!(action.bytes(), 0);
    }
}
