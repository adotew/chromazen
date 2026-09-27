use futures_channel::oneshot;

use super::{
    layers::{LayerId, PaintLayer},
    tiles::tile_document_rect,
};

const BYTES_PER_PIXEL: u32 = 4;

pub struct LayerReadback {
    device: wgpu::Device,
    layers: Vec<PendingLayerReadback>,
    size: [u32; 2],
    unpadded_bytes_per_row: usize,
    padded_bytes_per_row: usize,
}

struct PendingLayerReadback {
    id: LayerId,
    buffer: wgpu::Buffer,
    completion: oneshot::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl LayerReadback {
    pub fn finish(self) -> Result<Vec<(LayerId, image::RgbaImage)>, String> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("failed to wait for layer readback: {error}"))?;

        let mut images = Vec::with_capacity(self.layers.len());
        for mut layer in self.layers {
            layer
                .completion
                .try_recv()
                .map_err(|error| format!("failed to receive layer readback: {error}"))?
                .ok_or_else(|| "layer readback did not complete".to_owned())?
                .map_err(|error| format!("failed to map layer readback: {error}"))?;
            images.push(finish_layer(
                layer,
                self.size,
                self.unpadded_bytes_per_row,
                self.padded_bytes_per_row,
            )?);
        }
        Ok(images)
    }

    pub async fn finish_async(self) -> Result<Vec<(LayerId, image::RgbaImage)>, String> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        let mut images = Vec::with_capacity(self.layers.len());
        for mut layer in self.layers {
            (&mut layer.completion)
                .await
                .map_err(|error| format!("failed to receive layer readback: {error}"))?
                .map_err(|error| format!("failed to map layer readback: {error}"))?;
            images.push(finish_layer(
                layer,
                self.size,
                self.unpadded_bytes_per_row,
                self.padded_bytes_per_row,
            )?);
        }
        Ok(images)
    }
}

fn finish_layer(
    layer: PendingLayerReadback,
    size: [u32; 2],
    unpadded_bytes_per_row: usize,
    padded_bytes_per_row: usize,
) -> Result<(LayerId, image::RgbaImage), String> {
    let mapped = layer.buffer.slice(..).get_mapped_range();
    let pixels = unpack_rows(
        &mapped,
        unpadded_bytes_per_row,
        padded_bytes_per_row,
        size[1] as usize,
    );
    drop(mapped);
    layer.buffer.unmap();
    let image = image::RgbaImage::from_raw(size[0], size[1], pixels)
        .ok_or_else(|| "layer readback produced an invalid image size".to_owned())?;
    Ok((layer.id, image))
}

pub(super) fn begin_read_layers(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layers: &[PaintLayer],
    size: [u32; 2],
) -> LayerReadback {
    let unpadded_bytes_per_row = size[0] * BYTES_PER_PIXEL;
    let padded_bytes_per_row = aligned_bytes_per_row(unpadded_bytes_per_row);
    let buffer_size = u64::from(padded_bytes_per_row) * u64::from(size[1]);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("artwork layer readback encoder"),
    });
    let mut pending = Vec::with_capacity(layers.len());

    for layer in layers {
        // New buffers are zero-filled, so coordinates without a tile read back as transparent.
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("artwork layer readback buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        for (coord, tile) in layer.tiles.iter() {
            let rect = tile_document_rect(coord, size);
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &tile.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        // Offsets stay texel aligned and rows keep the padded image stride.
                        offset: u64::from(rect.y) * u64::from(padded_bytes_per_row)
                            + u64::from(rect.x * BYTES_PER_PIXEL),
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(rect.height),
                    },
                },
                wgpu::Extent3d {
                    width: rect.width,
                    height: rect.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        pending.push((layer.id, buffer));
    }
    queue.submit(std::iter::once(encoder.finish()));

    let layers = pending
        .into_iter()
        .map(|(id, buffer)| {
            let (sender, completion) = oneshot::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
            PendingLayerReadback {
                id,
                buffer,
                completion,
            }
        })
        .collect();

    LayerReadback {
        device: device.clone(),
        layers,
        size,
        unpadded_bytes_per_row: unpadded_bytes_per_row as usize,
        padded_bytes_per_row: padded_bytes_per_row as usize,
    }
}

fn aligned_bytes_per_row(bytes_per_row: u32) -> u32 {
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    bytes_per_row.div_ceil(alignment) * alignment
}

fn unpack_rows(source: &[u8], row_bytes: usize, padded_row_bytes: usize, rows: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(row_bytes * rows);
    for row in source.chunks_exact(padded_row_bytes).take(rows) {
        pixels.extend_from_slice(&row[..row_bytes]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_alignment_rounds_up_to_wgpu_requirement() {
        assert_eq!(aligned_bytes_per_row(16_000), 16_128);
        assert_eq!(aligned_bytes_per_row(256), 256);
    }

    #[test]
    fn row_padding_is_removed() {
        let source = [1, 2, 3, 4, 9, 9, 5, 6, 7, 8, 9, 9];
        assert_eq!(unpack_rows(&source, 4, 6, 2), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
