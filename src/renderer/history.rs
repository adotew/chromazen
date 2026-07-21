use super::{DOCUMENT_FORMAT, layers::LayerId};

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
}

impl HistoryAction {
    fn bytes(&self) -> u64 {
        match self {
            Self::Stroke(entry) => entry.bytes,
        }
    }

    fn layer_id(&self) -> LayerId {
        match self {
            Self::Stroke(entry) => entry.layer_id,
        }
    }
}

pub(crate) struct PaintHistory {
    actions: Vec<HistoryAction>,
    cursor: usize,
    used_bytes: u64,
    mirror: wgpu::Texture,
    mirrored_layer: Option<LayerId>,
    active_stroke: Option<LayerId>,
}

impl PaintHistory {
    pub(crate) fn new(device: &wgpu::Device, document_size: [u32; 2]) -> Self {
        Self {
            actions: Vec::new(),
            cursor: 0,
            used_bytes: 0,
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
        self.used_bytes = 0;
        self.mirrored_layer = None;
        self.active_stroke = None;
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.active_stroke.is_none() && self.cursor > 0
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.active_stroke.is_none() && self.cursor < self.actions.len()
    }

    pub(crate) fn undo_layer(&self) -> Option<LayerId> {
        self.can_undo()
            .then(|| self.actions[self.cursor - 1].layer_id())
    }

    pub(crate) fn redo_layer(&self) -> Option<LayerId> {
        self.can_redo()
            .then(|| self.actions[self.cursor].layer_id())
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
        for action in self.actions.drain(self.cursor..) {
            self.used_bytes -= action.bytes();
        }

        let pixels = create_texture(device, "paint history entry", [rect.width, rect.height]);
        copy_rect(encoder, &self.mirror, rect, &pixels, [0, 0]);
        self.sync_layer(encoder, layer_id, canvas, rect);

        let bytes = rect.byte_len();
        self.actions.push(HistoryAction::Stroke(StrokeEntry {
            layer_id,
            rect,
            pixels,
            bytes,
        }));
        self.used_bytes += bytes;
        self.cursor = self.actions.len();
        self.evict_to_budget();
        self.active_stroke = None;
    }

    pub(crate) fn undo(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
    ) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.cursor -= 1;
        let HistoryAction::Stroke(entry) = &self.actions[self.cursor];
        swap_entry(encoder, &self.mirror, canvas, entry);
        self.mirrored_layer = Some(entry.layer_id);
        true
    }

    pub(crate) fn redo(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
    ) -> bool {
        if !self.can_redo() {
            return false;
        }
        let HistoryAction::Stroke(entry) = &self.actions[self.cursor];
        swap_entry(encoder, &self.mirror, canvas, entry);
        self.mirrored_layer = Some(entry.layer_id);
        self.cursor += 1;
        true
    }

    fn evict_to_budget(&mut self) {
        let count = eviction_count(
            self.used_bytes,
            HISTORY_BUDGET_BYTES,
            self.actions.len(),
            self.actions.iter().map(HistoryAction::bytes),
        );
        for action in self.actions.drain(..count) {
            self.used_bytes -= action.bytes();
        }
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
}
