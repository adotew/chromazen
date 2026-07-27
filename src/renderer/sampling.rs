use std::sync::mpsc;

use super::PaintLayer;

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
    if layers.is_empty() {
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
    for (index, layer) in layers.iter().enumerate() {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &layer.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: pixel[0],
                    y: pixel[1],
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
    let color = composite_premultiplied(background, &mapped);
    drop(mapped);
    readback.unmap();
    Some(color)
}

fn composite_premultiplied(background: [f32; 4], layer_pixels: &[u8]) -> [u8; 3] {
    // Paint textures are premultiplied RGBA and arrive in bottom-to-top render order.
    let mut color = background;
    for pixel in layer_pixels.chunks_exact(4) {
        let alpha = f32::from(pixel[3]) / 255.0;
        let inverse_alpha = 1.0 - alpha;
        for channel in 0..3 {
            let source = f32::from(pixel[channel]) / 255.0;
            color[channel] = source + color[channel] * inverse_alpha;
        }
        color[3] = alpha + color[3] * inverse_alpha;
    }
    rgb8(color)
}

fn rgb8(color: [f32; 4]) -> [u8; 3] {
    [color[0], color[1], color[2]].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_document_points_to_pixels() {
        assert_eq!(document_pixel([12.9, 4.1], [100, 50]), Some([12, 4]));
        assert_eq!(document_pixel([0.0, 0.0], [100, 50]), Some([0, 0]));
        assert_eq!(document_pixel([99.99, 49.99], [100, 50]), Some([99, 49]));
    }

    #[test]
    fn rejects_points_outside_the_document() {
        for point in [
            [-0.1, 2.0],
            [2.0, -0.1],
            [100.0, 2.0],
            [2.0, 50.0],
            [f32::NAN, 2.0],
        ] {
            assert_eq!(document_pixel(point, [100, 50]), None);
        }
    }

    #[test]
    fn transparent_layers_reveal_the_background() {
        assert_eq!(
            composite_premultiplied([0.2, 0.4, 0.8, 1.0], &[0, 0, 0, 0]),
            [51, 102, 204]
        );
    }

    #[test]
    fn composites_premultiplied_layers_bottom_to_top() {
        // Half-red over blue, followed by half-green over that result.
        let pixels = [128, 0, 0, 128, 0, 128, 0, 128];
        assert_eq!(
            composite_premultiplied([0.0, 0.0, 1.0, 1.0], &pixels),
            [64, 128, 63]
        );
    }

    #[test]
    fn opaque_top_layer_wins() {
        let pixels = [255, 0, 0, 255, 12, 34, 56, 255];
        assert_eq!(
            composite_premultiplied([1.0, 1.0, 1.0, 1.0], &pixels),
            [12, 34, 56]
        );
    }
}
