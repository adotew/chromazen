use super::DOCUMENT_FORMAT;

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

struct HistoryEntry {
    rect: TextureRect,
    pixels: wgpu::Texture,
    bytes: u64,
}

pub(crate) struct PaintHistory {
    entries: Vec<HistoryEntry>,
    cursor: usize,
    used_bytes: u64,
    mirror: wgpu::Texture,
    stroke_active: bool,
}

impl PaintHistory {
    pub(crate) fn new(device: &wgpu::Device, document_size: [u32; 2]) -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            used_bytes: 0,
            mirror: create_texture(device, "paint history mirror", document_size),
            stroke_active: false,
        }
    }

    pub(crate) fn begin_stroke(&mut self) -> bool {
        if self.stroke_active {
            return false;
        }
        self.stroke_active = true;
        true
    }

    pub(crate) fn end_empty_stroke(&mut self) {
        self.stroke_active = false;
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.used_bytes = 0;
        self.stroke_active = false;
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.stroke_active && self.cursor > 0
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.stroke_active && self.cursor < self.entries.len()
    }

    pub(crate) fn sync_canvas(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
        rect: TextureRect,
    ) {
        copy_rect(encoder, canvas, rect, &self.mirror, [rect.x, rect.y]);
    }

    pub(crate) fn commit_stroke(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::Texture,
        rect: TextureRect,
    ) {
        for entry in self.entries.drain(self.cursor..) {
            self.used_bytes -= entry.bytes;
        }

        let pixels = create_texture(device, "paint history entry", [rect.width, rect.height]);
        copy_rect(encoder, &self.mirror, rect, &pixels, [0, 0]);
        self.sync_canvas(encoder, canvas, rect);

        let bytes = rect.byte_len();
        self.entries.push(HistoryEntry {
            rect,
            pixels,
            bytes,
        });
        self.used_bytes += bytes;
        self.cursor = self.entries.len();
        self.evict_to_budget();
        self.stroke_active = false;
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
        swap_entry(encoder, &self.mirror, canvas, &self.entries[self.cursor]);
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
        swap_entry(encoder, &self.mirror, canvas, &self.entries[self.cursor]);
        self.cursor += 1;
        true
    }

    fn evict_to_budget(&mut self) {
        let count = eviction_count(
            self.used_bytes,
            HISTORY_BUDGET_BYTES,
            self.entries.len(),
            self.entries.iter().map(|entry| entry.bytes),
        );
        for entry in self.entries.drain(..count) {
            self.used_bytes -= entry.bytes;
        }
        self.cursor -= count;
    }
}

fn swap_entry(
    encoder: &mut wgpu::CommandEncoder,
    mirror: &wgpu::Texture,
    canvas: &wgpu::Texture,
    entry: &HistoryEntry,
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
