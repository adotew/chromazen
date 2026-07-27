use std::sync::mpsc;

use super::layers::{LayerId, PaintLayer};

const BYTES_PER_PIXEL: u32 = 4;

pub(super) fn read_layers(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layers: &[PaintLayer],
    size: [u32; 2],
) -> Result<Vec<(LayerId, image::RgbaImage)>, String> {
    let unpadded_bytes_per_row = size[0] * BYTES_PER_PIXEL;
    let padded_bytes_per_row = aligned_bytes_per_row(unpadded_bytes_per_row);
    let buffer_size = u64::from(padded_bytes_per_row) * u64::from(size[1]);
    let mut images = Vec::with_capacity(layers.len());

    for layer in layers {
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("artwork layer readback buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("artwork layer readback encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &layer.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let (sender, receiver) = mpsc::sync_channel(1);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("failed to wait for layer readback: {error}"))?;
        receiver
            .recv()
            .map_err(|error| format!("failed to receive layer readback: {error}"))?
            .map_err(|error| format!("failed to map layer readback: {error}"))?;

        let mapped = readback.slice(..).get_mapped_range();
        let pixels = unpack_rows(
            &mapped,
            unpadded_bytes_per_row as usize,
            padded_bytes_per_row as usize,
            size[1] as usize,
        );
        drop(mapped);
        readback.unmap();
        let image = image::RgbaImage::from_raw(size[0], size[1], pixels)
            .ok_or_else(|| "layer readback produced an invalid image size".to_owned())?;
        images.push((layer.id, image));
    }
    Ok(images)
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
