use super::DOCUMENT_FORMAT;

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

    fn extent(self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        }
    }
}

struct HistoryEntry {
    _rect: TextureRect,
    _pixels: wgpu::Texture,
}

pub(crate) struct PaintHistory {
    entries: Vec<HistoryEntry>,
    mirror: wgpu::Texture,
    stroke_active: bool,
}

impl PaintHistory {
    pub(crate) fn new(device: &wgpu::Device, document_size: [u32; 2]) -> Self {
        Self {
            entries: Vec::new(),
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
        self.stroke_active = false;
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
        let pixels = create_texture(device, "paint history entry", [rect.width, rect.height]);
        copy_rect(encoder, &self.mirror, rect, &pixels, [0, 0]);
        self.sync_canvas(encoder, canvas, rect);
        self.entries.push(HistoryEntry {
            _rect: rect,
            _pixels: pixels,
        });
        self.stroke_active = false;
    }
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
}
