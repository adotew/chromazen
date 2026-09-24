use super::*;
use crate::{
    StrokePoint,
    renderer::{persistence, stamps::StampQueue},
};
use std::sync::Arc;

#[test]
#[ignore = "requires a wgpu adapter"]
fn tile_origins_and_independent_smudge_sources_match_document_pixels() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    const SIZE: [u32; 2] = [16, 16];
    const DOCUMENT: [u32; 2] = [16384, 16384];
    let resources = RenderResources::new(
        &device,
        &queue,
        SIZE,
        SIZE,
        DOCUMENT_FORMAT,
        &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
    )
    .unwrap();
    let (source, source_view) = create_paint_texture(&device, SIZE);
    let (target, target_view) = create_paint_texture(&device, SIZE);
    let source_pixels = image::RgbaImage::from_fn(16, 16, |x, y| {
        image::Rgba([0, x as u8 * 8, y as u8 * 8, 192])
    });
    let target_pixels = image::RgbaImage::from_pixel(16, 16, image::Rgba([160, 0, 0, 192]));
    for (texture, pixels) in [(&source, source_pixels), (&target, target_pixels)] {
        queue.write_texture(
            texture.as_image_copy(),
            pixels.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16 * 4),
                rows_per_image: Some(16),
            },
            texture.size(),
        );
    }
    queue.write_buffer(
        &resources.stroke_uniform_buffer,
        0,
        bytemuck::bytes_of(&StrokeUniform {
            color: [1.0, 0.0, 0.0, 1.0],
        }),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut outputs = Vec::new();

    // Both draws are submitted together with distinct immutable uniforms/stamps.
    // Reusing one queue-written uniform location would move the first dab too.
    for origin in [[512.0, 512.0], [1024.0, 768.0]] {
        let paint = PaintUniform {
            dims: [16.0; 2],
            origin,
            ..PaintUniform::full_document(DOCUMENT)
        };
        let center = [origin[0] + 2.0, origin[1] + 2.0];
        let group = stamp_group(
            &device,
            &resources,
            paint,
            center,
            center,
            2.0,
            [&source_view, &target_view],
        );
        draw(
            &mut encoder,
            &resources.stroke_mask_view,
            &resources.mask_pipeline,
            &group,
            6,
        );
        let (output, view) = create_paint_texture(&device, SIZE);
        draw(
            &mut encoder,
            &view,
            &resources.brush_commit_pipeline,
            &resources.stroke_commit_bind_group,
            3,
        );
        outputs.push(output);
    }

    // A source far from the destination, plus padded source regions crossing
    // both actual document edges. Neither case requires a union-sized texture.
    for (source_origin, source_center) in [
        ([12000.0, 9000.0], [12008.0, 9008.0]),
        ([-8.0, -8.0], [-100.0, -100.0]),
        ([16376.0, 16376.0], [16484.0, 16484.0]),
    ] {
        let paint = PaintUniform {
            dims: [16.0; 2],
            origin: [512.0; 2],
            source_dims: [16.0; 2],
            source_origin,
            ..PaintUniform::full_document(DOCUMENT)
        };
        let group = stamp_group(
            &device,
            &resources,
            paint,
            [520.0; 2],
            source_center,
            8.0,
            [&source_view, &target_view],
        );
        let (output, view) = create_paint_texture(&device, SIZE);
        draw(&mut encoder, &view, &resources.smudge_pipeline, &group, 6);
        outputs.push(output);
    }
    queue.submit([encoder.finish()]);
    let tracker = Arc::new(crate::renderer::diagnostics::ReadbackTracker::default());
    let images = persistence::begin_read_regions(
        &device,
        &queue,
        outputs
            .iter()
            .enumerate()
            .map(|(i, texture)| (LayerId(i as u64), texture, [0; 2])),
        SIZE,
        tracker.retain(persistence::readback_byte_len(SIZE) * outputs.len() as u64),
    )
    .finish()
    .unwrap();
    for (_, image) in &images[..2] {
        for (x, y, pixel) in image.enumerate_pixels() {
            assert_eq!(
                pixel.0,
                if x < 4 && y < 4 {
                    [255, 0, 0, 255]
                } else {
                    [0; 4]
                }
            );
        }
    }
    for (index, (_, image)) in images[2..].iter().enumerate() {
        for (x, y, pixel) in image.enumerate_pixels() {
            let (sx, sy) = match index {
                0 => (x, y),
                1 => (8, 8),
                _ => (7, 7),
            };
            let expected = [
                104,
                (sx as f32 * 8.0 * 0.35).round() as u8,
                (sy as f32 * 8.0 * 0.35).round() as u8,
                192,
            ];
            assert!(
                pixel
                    .0
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 1),
                "case {index}, ({x},{y}): {pixel:?}, expected {expected:?}"
            );
        }
    }
    assert_eq!(std::mem::size_of::<PaintUniform>(), 48);
    assert_eq!(tracker.live(), 0);
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn layer_pipeline_samples_only_the_tile_region_in_document_space() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let document = [64, 64];
    let resources = RenderResources::new(
        &device,
        &queue,
        document,
        document,
        DOCUMENT_FORMAT,
        &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
    )
    .unwrap();
    let (tile, tile_view) = create_paint_texture(&device, [7, 13]);
    let pixels = image::RgbaImage::from_fn(7, 13, |x, y| {
        image::Rgba([x as u8 * 30, y as u8 * 15, 0, 255])
    });
    queue.write_texture(
        tile.as_image_copy(),
        pixels.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(7 * 4),
            rows_per_image: Some(13),
        },
        tile.size(),
    );
    let region = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("immutable tile blit region"),
        contents: bytemuck::bytes_of(&LayerTileUniform {
            origin: [25.0, 30.0],
            extent: [7.0, 13.0],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("tile blit opacity"),
        contents: bytemuck::bytes_of(&LayerSettingsUniform {
            opacity: 1.0,
            padding: [0.0; 3],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tile blit"),
        layout: &resources.blit_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&resources.paint_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&tile_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: resources.view_uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: settings.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: region.as_entire_binding(),
            },
        ],
    });
    let (output, view) = create_paint_texture(&device, document);
    queue.write_buffer(
        &resources.view_uniform_buffer,
        0,
        bytemuck::bytes_of(&ViewUniform {
            document_from_window_x: [1.0, 0.0, 0.0, 0.0],
            document_from_window_y: [0.0, 1.0, 0.0, 0.0],
            paint_dims: [64.0; 2],
            padding: [0.0; 2],
            background_color: [0.0; 4],
        }),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    draw(&mut encoder, &view, &resources.layer_pipeline, &group, 3);
    queue.submit([encoder.finish()]);
    let tracker = Arc::new(crate::renderer::diagnostics::ReadbackTracker::default());
    let image = persistence::begin_read_regions(
        &device,
        &queue,
        std::iter::once((LayerId(1), &output, [0; 2])),
        document,
        tracker.retain(persistence::readback_byte_len(document)),
    )
    .finish()
    .unwrap()
    .remove(0)
    .1;
    for (x, y, pixel) in image.enumerate_pixels() {
        let expected = if (25..32).contains(&x) && (30..43).contains(&y) {
            *pixels.get_pixel(x - 25, y - 30)
        } else {
            image::Rgba([0; 4])
        };
        assert_eq!(*pixel, expected, "({x},{y})");
    }
    assert_eq!(tracker.live(), 0);
    assert_eq!(std::mem::size_of::<LayerTileUniform>(), 16);
}

fn stamp_group(
    device: &wgpu::Device,
    resources: &RenderResources,
    paint: PaintUniform,
    center: [f32; 2],
    source_center: [f32; 2],
    radius: f32,
    sources: [&wgpu::TextureView; 2],
) -> wgpu::BindGroup {
    let point = |[x, y]: [f32; 2]| StrokePoint {
        x,
        y,
        radius,
        opacity: 1.0,
    };
    let mut stamps = StampQueue::new(1.0);
    stamps.begin_stroke(point(source_center));
    assert!(stamps.queue_point(point(center), [1.0; 4], 16384, 16384));
    let raw = stamps.drain_raw(16384, 16384, 1);
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("immutable tile test stamp"),
        contents: bytemuck::cast_slice(&raw),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("immutable tile test uniform"),
        contents: bytemuck::bytes_of(&paint),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let [group, _] = create_stamp_bind_groups(
        device,
        &resources.stamp_bind_group_layout,
        &resources.brush_sampler,
        &resources.brush_texture_view,
        [&buffer; 2],
        &uniform,
        sources,
    );
    group
}

fn draw(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    vertices: u32,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("tile shader regression"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..vertices, 0..1);
}
