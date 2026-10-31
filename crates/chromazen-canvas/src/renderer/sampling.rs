use std::sync::mpsc;

use crate::{LayerSample, composite_samples};

use super::{
    PaintLayer,
    tiles::{TILE_SIZE, TileCoord},
};

const BYTES_PER_PIXEL: u64 = 4;

pub(super) fn document_pixel(point: [f32; 2], document_size: [u32; 2]) -> Option<[u32; 2]> {
    if !point[0].is_finite()
        || !point[1].is_finite()
        || point[0] < 0.0
        || point[1] < 0.0
        || point[0] >= document_size[0] as f32
        || point[1] >= document_size[1] as f32
    {
        return None;
    }
    Some([point[0].floor() as u32, point[1].floor() as u32])
}

pub(super) fn read_composited_color(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layers: &[PaintLayer],
    pixel: [u32; 2],
    background: [f32; 4],
) -> Option<[u8; 3]> {
    if layers.iter().all(|layer| !layer.visible) {
        return Some(rgb8(background));
    }

    let buffer_size = layers.len() as u64 * BYTES_PER_PIXEL;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("eyedropper readback buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("eyedropper readback encoder"),
    });
    let coord = TileCoord::containing(pixel);
    // The buffer starts zero-filled, so layers without a tile here sample as transparent.
    for (index, layer) in layers.iter().enumerate() {
        let Some(tile) = layer.tiles.get(coord) else {
            continue;
        };
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tile.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: pixel[0] % TILE_SIZE,
                    y: pixel[1] % TILE_SIZE,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: index as u64 * BYTES_PER_PIXEL,
                    bytes_per_row: None,
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
    queue.submit(std::iter::once(encoder.finish()));

    // Input throttles drag sampling so this immediate readback does not run for every pointer event.
    let (sender, receiver) = mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    if let Err(error) = device.poll(wgpu::PollType::wait_indefinitely()) {
        log::error!("failed to wait for eyedropper readback: {error}");
        return None;
    }
    match receiver.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::error!("failed to map eyedropper readback: {error}");
            return None;
        }
        Err(error) => {
            log::error!("failed to receive eyedropper readback: {error}");
            return None;
        }
    }

    let mapped = readback.slice(..).get_mapped_range();
    let samples: Vec<_> = mapped
        .as_chunks::<4>()
        .0
        .iter()
        .zip(layers)
        .map(|(pixel, layer)| LayerSample {
            pixel: *pixel,
            opacity: layer.opacity,
            visible: layer.visible,
            clipped: layer.clipped,
            blend_mode: layer.blend_mode,
        })
        .collect();
    let color = rgb8(composite_samples(background, &samples));
    drop(mapped);
    readback.unmap();
    Some(color)
}

fn rgb8(color: [f32; 4]) -> [u8; 3] {
    [color[0], color[1], color[2]].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests;
