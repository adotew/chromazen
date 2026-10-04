use std::sync::mpsc;

use chromazen_canvas::{
    BrushCursor, BrushSpacing, Canvas, CanvasDocument, LayerId, LayerInfo, LayerTransform,
    PaintTool, StrokePoint,
};

const RENDER_SIZE: [u32; 2] = [64, 64];

#[test]
#[ignore = "requires a wgpu adapter"]
fn paints_renders_and_restores_history_without_a_surface() {
    pollster::block_on(run());
}

async fn run() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .expect("headless wgpu adapter");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("canvas headless test device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("headless wgpu device");

    let size = [32, 32];
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    let mut canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        size,
        size,
        &brush,
        [0.16; 3],
    )
    .expect("canvas");
    let point = StrokePoint {
        x: 16.0,
        y: 16.0,
        radius: 4.0,
        opacity: 1.0,
    };
    assert!(canvas.begin_stroke(PaintTool::Brush, point, [0.0, 0.0, 0.0, 1.0], 1.0));
    assert!(canvas.queue_stamp(point));
    canvas.end_stroke();

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canvas headless render target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("canvas headless render encoder"),
    });
    canvas.render_to_view(&mut encoder, &view, None);
    queue.submit([encoder.finish()]);

    assert!(center_alpha(&canvas) > 0);
    assert!(canvas.undo());
    assert_eq!(center_alpha(&canvas), 0);
    assert!(canvas.redo());
    assert!(center_alpha(&canvas) > 0);

    strokes_across_tile_edges_undo_and_redo(&device, &queue);
    sparse_documents_round_trip_through_tiles(&device, &queue);
    unaligned_canvas_resize_shifts_tiled_content(&device, &queue);
    smudge_drags_color_into_an_empty_tile(&device, &queue);
    selection_limits_painting_across_tiles(&device, &queue);
    selection_transform_moves_only_selected_pixels(&device, &queue);
    preview_is_visible_but_not_committed(&device, &queue);
    run_brush_cursor_contrast(&device, &queue);
    run_adjustment_preview_over_reference(&device, &queue);
    run_workspace_background_colors(&device, &queue);
}

fn center_alpha(canvas: &Canvas) -> u8 {
    canvas
        .begin_document_layer_readback()
        .expect("readback")
        .finish()
        .expect("pixels")[0]
        .1
        .get_pixel(16, 16)[3]
}

// Wider and taller than one 512 px tile, with partial edge tiles.
const TILED_DOCUMENT_SIZE: [u32; 2] = [1100, 700];

fn tiled_canvas(device: &wgpu::Device, queue: &wgpu::Queue) -> Canvas {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        TILED_DOCUMENT_SIZE,
        &brush,
        [0.16; 3],
    )
    .expect("tiled canvas")
}

fn read_layers(canvas: &Canvas) -> Vec<image::RgbaImage> {
    canvas
        .begin_document_layer_readback()
        .expect("readback")
        .finish()
        .expect("pixels")
        .into_iter()
        .map(|(_, image)| image)
        .collect()
}

fn load_single_layer(canvas: &mut Canvas, image: image::RgbaImage) {
    canvas
        .load_document(
            &CanvasDocument {
                size: [image.width(), image.height()],
                background: [255; 3],
                selected_layer: LayerId(1),
                layers: vec![LayerInfo {
                    id: LayerId(1),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    opacity: 100,
                    clipped: false,
                }],
            },
            vec![image],
        )
        .expect("load document");
}

/// Sparse content with pixels on tile and document edges and fully transparent tiles.
fn sparse_pattern() -> image::RgbaImage {
    image::RgbaImage::from_fn(TILED_DOCUMENT_SIZE[0], TILED_DOCUMENT_SIZE[1], |x, y| {
        let on_edge = [0, 511, 512, 1023, 1024, 1099].contains(&x) && y < 600;
        if (x < 512 && y < 512 && (x + y) % 7 == 0) || on_edge || (x, y) == (1099, 699) {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        } else {
            image::Rgba([0; 4])
        }
    })
}

fn strokes_across_tile_edges_undo_and_redo(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    let from = StrokePoint {
        x: 490.0,
        y: 510.0,
        radius: 6.0,
        opacity: 1.0,
    };
    let to = StrokePoint { x: 540.0, ..from };
    assert!(canvas.begin_stroke(PaintTool::Brush, from, [0.0, 0.0, 0.0, 1.0], 1.0));
    assert!(canvas.queue_stamp(from));
    canvas.stamp_line(from, to, BrushSpacing::default());
    canvas.end_stroke();

    // One sample in each of the four tiles around the corner at (512, 512).
    let samples = [[500, 506], [530, 506], [500, 514], [530, 514]];
    let painted = |canvas: &Canvas| {
        let layer = &read_layers(canvas)[0];
        samples.map(|[x, y]| layer.get_pixel(x, y)[3] > 0)
    };
    assert_eq!(painted(&canvas), [true; 4]);
    assert!(canvas.undo());
    assert_eq!(painted(&canvas), [false; 4]);
    assert!(canvas.redo());
    assert_eq!(painted(&canvas), [true; 4]);

    // Erasing across the same edge only changes existing tiles and undoes cleanly.
    assert!(canvas.begin_stroke(PaintTool::Eraser, from, [0.0; 4], 1.0));
    assert!(canvas.queue_stamp(from));
    canvas.stamp_line(from, to, BrushSpacing::default());
    canvas.end_stroke();
    assert_eq!(painted(&canvas), [false; 4]);
    assert!(canvas.undo());
    assert_eq!(painted(&canvas), [true; 4]);
}

fn sparse_documents_round_trip_through_tiles(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    let image = sparse_pattern();
    load_single_layer(&mut canvas, image.clone());
    assert!(read_layers(&canvas)[0] == image);
}

fn unaligned_canvas_resize_shifts_tiled_content(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    let image = sparse_pattern();
    load_single_layer(&mut canvas, image.clone());

    let size = [900, 800];
    let origin = [-37_i32, 101];
    assert!(canvas.resize_canvas(size, origin).expect("resize"));
    let expected = image::RgbaImage::from_fn(size[0], size[1], |x, y| {
        let source = [x as i32 + origin[0], y as i32 + origin[1]];
        if source.iter().all(|&value| value >= 0)
            && (source[0] as u32) < image.width()
            && (source[1] as u32) < image.height()
        {
            *image.get_pixel(source[0] as u32, source[1] as u32)
        } else {
            image::Rgba([0; 4])
        }
    });
    assert!(read_layers(&canvas)[0] == expected);

    assert!(canvas.undo());
    assert_eq!(canvas.document_size(), TILED_DOCUMENT_SIZE);
    assert!(read_layers(&canvas)[0] == image);
}

fn smudge_drags_color_into_an_empty_tile(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    // Only the first tile column has paint.
    let image =
        image::RgbaImage::from_fn(TILED_DOCUMENT_SIZE[0], TILED_DOCUMENT_SIZE[1], |x, _| {
            if x < 512 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0; 4])
            }
        });
    load_single_layer(&mut canvas, image);
    let from = StrokePoint {
        x: 490.0,
        y: 300.0,
        radius: 12.0,
        opacity: 1.0,
    };
    let to = StrokePoint { x: 560.0, ..from };
    assert!(canvas.begin_stroke(PaintTool::Smudge, from, [0.0; 4], 1.0));
    assert!(canvas.queue_stamp(from));
    canvas.stamp_line(from, to, BrushSpacing::default());
    canvas.end_stroke();

    let smudged = read_layers(&canvas)[0].get_pixel(530, 300).0;
    assert!(smudged[3] > 0 && smudged[0] > 0 && smudged[1] == 0);
    assert!(canvas.undo());
    assert_eq!(read_layers(&canvas)[0].get_pixel(530, 300)[3], 0);
}

fn rectangle(min: [f32; 2], max: [f32; 2]) -> Vec<[f32; 2]> {
    vec![min, [max[0], min[1]], max, [min[0], max[1]]]
}

fn selection_limits_painting_across_tiles(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    let from = StrokePoint {
        x: 490.0,
        y: 300.0,
        radius: 6.0,
        opacity: 1.0,
    };
    let to = StrokePoint { x: 560.0, ..from };
    let stroke = |canvas: &mut Canvas, tool| {
        assert!(canvas.begin_stroke(tool, from, [0.0, 0.0, 0.0, 1.0], 1.0));
        assert!(canvas.queue_stamp(from));
        canvas.stamp_line(from, to, BrushSpacing::default());
        canvas.end_stroke();
    };
    let alpha_at = |canvas: &Canvas, x| read_layers(canvas)[0].get_pixel(x, 300)[3];

    assert!(canvas.set_selection(rectangle([0.0, 0.0], [520.0, 700.0])));
    stroke(&mut canvas, PaintTool::Brush);
    assert_eq!(alpha_at(&canvas, 500), 255);
    assert_eq!(alpha_at(&canvas, 515), 255);
    assert_eq!(alpha_at(&canvas, 530), 0);

    assert!(canvas.set_selection(rectangle([505.0, 0.0], [1100.0, 700.0])));
    stroke(&mut canvas, PaintTool::Eraser);
    assert_eq!(alpha_at(&canvas, 500), 255);
    assert_eq!(alpha_at(&canvas, 515), 0);

    let image =
        image::RgbaImage::from_fn(TILED_DOCUMENT_SIZE[0], TILED_DOCUMENT_SIZE[1], |x, _| {
            if x < 512 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0; 4])
            }
        });
    load_single_layer(&mut canvas, image);
    assert!(canvas.selection_polygon().is_none());
    assert!(canvas.set_selection(rectangle([0.0, 0.0], [520.0, 700.0])));
    stroke(&mut canvas, PaintTool::Smudge);
    assert!(alpha_at(&canvas, 515) > 0);
    assert_eq!(alpha_at(&canvas, 530), 0);
}

fn selection_transform_moves_only_selected_pixels(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = tiled_canvas(device, queue);
    let image =
        image::RgbaImage::from_fn(TILED_DOCUMENT_SIZE[0], TILED_DOCUMENT_SIZE[1], |x, y| {
            let in_square = |left| (left..left + 20).contains(&x) && (100..120).contains(&y);
            if in_square(100) || in_square(300) {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0; 4])
            }
        });
    load_single_layer(&mut canvas, image);
    assert!(canvas.set_selection(rectangle([90.0, 90.0], [130.0, 130.0])));
    assert_eq!(
        canvas.read_selected_layer_content_bounds().map(|b| b.max),
        Some([120.0, 120.0])
    );
    assert!(canvas.update_layer_transform(LayerTransform {
        translation: [600.0, 0.0],
        ..LayerTransform::default()
    }));
    assert!(canvas.commit_layer_transform());

    let alpha_at = |canvas: &Canvas, x| read_layers(canvas)[0].get_pixel(x, 110)[3];
    assert_eq!(alpha_at(&canvas, 110), 0);
    assert_eq!(alpha_at(&canvas, 710), 255);
    assert_eq!(alpha_at(&canvas, 310), 255);
    assert_eq!(
        canvas.selection_polygon().map(|polygon| polygon[0]),
        Some([690.0, 90.0])
    );

    assert!(canvas.undo());
    assert_eq!(alpha_at(&canvas, 110), 255);
    assert_eq!(alpha_at(&canvas, 710), 0);
}

fn preview_is_visible_but_not_committed(device: &wgpu::Device, queue: &wgpu::Queue) {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    let mut canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        RENDER_SIZE,
        &brush,
        [0.16; 3],
    )
    .expect("preview canvas");
    let start = StrokePoint {
        x: 8.0,
        y: 32.0,
        radius: 3.0,
        opacity: 1.0,
    };
    let preview_end = StrokePoint { x: 48.0, ..start };
    assert!(canvas.begin_stroke(PaintTool::Brush, start, [0.0, 0.0, 0.0, 1.0], 1.0));
    assert!(canvas.queue_stamp(start));
    assert!(canvas.update_stroke_preview(start, &[preview_end], BrushSpacing::default()));

    let pixels = render_pixels(device, queue, &mut canvas, None);
    let offset = ((32 * RENDER_SIZE[0] + 48) * 4) as usize;
    assert!(
        pixels[offset..offset + 3]
            .iter()
            .all(|&channel| channel < 64)
    );

    canvas.end_stroke();
    let layers = canvas
        .begin_document_layer_readback()
        .expect("preview readback")
        .finish()
        .expect("preview pixels");
    assert_eq!(layers[0].1.get_pixel(48, 32)[3], 0);
}

fn run_brush_cursor_contrast(device: &wgpu::Device, queue: &wgpu::Queue) {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));

    // A narrow document leaves workspace visible on both sides. Test the neutral gray option
    // because the cursor's contrast changes near 50% gray.
    let mut workspace_canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        [16, 32],
        &brush,
        [0.5; 3],
    )
    .expect("workspace canvas");
    let pixels = render_pixels(
        device,
        queue,
        &mut workspace_canvas,
        Some(BrushCursor {
            center: [8.0, 32.0],
            diameter: 4.0,
        }),
    );
    assert_visible_outline(&pixels, [0, 16, 16, 48], [128, 128, 128]);
    assert_single_dark_ring(&pixels, [0, 16, 16, 48], [128, 128, 128]);

    // Exercise nearby mid-grays and saturated colors to keep the softened response visible across
    // the supported color range. Neighboring mid-grays must not abruptly flip polarity.
    let mut midtone_differences = Vec::new();
    for background in [
        [124, 124, 124],
        [128, 128, 128],
        [132, 132, 132],
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
    ] {
        let mut canvas = Canvas::new(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8Unorm,
            RENDER_SIZE,
            RENDER_SIZE,
            &brush,
            [0.16; 3],
        )
        .expect("canvas");
        canvas.set_background_color(background);
        let pixels = render_pixels(
            device,
            queue,
            &mut canvas,
            Some(BrushCursor {
                center: [32.0, 32.0],
                diameter: 8.0,
            }),
        );
        assert_visible_outline(&pixels, [20, 20, 44, 44], background);
        if background[0] == background[1] && background[1] == background[2] {
            midtone_differences.push(strongest_luminance_difference(
                &pixels,
                [20, 20, 44, 44],
                background,
            ));
        }
    }

    for pair in midtone_differences.windows(2) {
        let change = (pair[1] - pair[0]).abs();
        assert!(
            change <= 0.04,
            "cursor contrast changes too abruptly between neighboring midtones: {pair:?}"
        );
    }
}

fn run_adjustment_preview_over_reference(device: &wgpu::Device, queue: &wgpu::Queue) {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    let mut canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        RENDER_SIZE,
        &brush,
        [0.16; 3],
    )
    .expect("preview canvas");
    let pixels = render_pixels_with_reference_overlay(
        device,
        queue,
        &mut canvas,
        BrushCursor {
            center: [32.0, 32.0],
            diameter: 16.0,
        },
    );
    assert_visible_outline(&pixels, [20, 20, 44, 44], [255, 0, 0]);
    // Only the reference's red may contribute to the outline, never the white canvas below it.
    for pixel in pixels.as_chunks::<4>().0 {
        assert_eq!(&pixel[1..3], &[0, 0]);
    }
    let center = ((32 * RENDER_SIZE[0] + 32) * 4) as usize;
    assert_eq!(&pixels[center..center + 3], &[255, 0, 0]);
}

fn run_workspace_background_colors(device: &wgpu::Device, queue: &wgpu::Queue) {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    let mut canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        [16, 32],
        &brush,
        [0.82; 3],
    )
    .expect("workspace canvas");

    let initial_pixels = render_pixels(device, queue, &mut canvas, None);
    assert!(initial_pixels[0].abs_diff(209) <= 1);

    for (gray, expected) in [(0.16, 41_u8), (0.5, 128), (0.82, 209)] {
        canvas.set_workspace_background_color([gray; 3]);
        let pixels = render_pixels(device, queue, &mut canvas, None);
        for &channel in &pixels[..3] {
            assert!(channel.abs_diff(expected) <= 1);
        }
        let document_offset = ((32 * RENDER_SIZE[0] + 32) * 4) as usize;
        assert_eq!(&pixels[document_offset..document_offset + 3], &[255; 3]);
    }
}

fn render_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas,
    cursor: Option<BrushCursor>,
) -> Vec<u8> {
    render_pixels_inner(device, queue, canvas, cursor, None)
}

fn render_pixels_with_reference_overlay(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas,
    cursor: BrushCursor,
) -> Vec<u8> {
    render_pixels_inner(device, queue, canvas, None, Some(cursor))
}

fn render_pixels_inner(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas,
    cursor: Option<BrushCursor>,
    overlay_cursor: Option<BrushCursor>,
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cursor contrast render target"),
        size: wgpu::Extent3d {
            width: RENDER_SIZE[0],
            height: RENDER_SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let bytes_per_row = RENDER_SIZE[0] * 4;
    assert_eq!(bytes_per_row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cursor contrast readback buffer"),
        size: u64::from(bytes_per_row) * u64::from(RENDER_SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cursor contrast render encoder"),
    });
    canvas.render_to_view(&mut encoder, &view, cursor);
    if let Some(cursor) = overlay_cursor {
        // Simulate an egui reference painted after the canvas and before the preview.
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("reference overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        drop(pass);
        canvas.render_brush_cursor_over_view(&mut encoder, &target, &view, cursor);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(RENDER_SIZE[1]),
            },
        },
        wgpu::Extent3d {
            width: RENDER_SIZE[0],
            height: RENDER_SIZE[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for cursor readback");
    receiver
        .recv()
        .expect("receive cursor readback")
        .expect("map cursor readback");
    let mapped = readback.slice(..).get_mapped_range();
    let pixels = mapped.to_vec();
    drop(mapped);
    readback.unmap();
    pixels
}

fn assert_visible_outline(pixels: &[u8], rect: [u32; 4], background: [u8; 3]) {
    let mut maximum_difference = 0.0_f32;
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let offset = ((y * RENDER_SIZE[0] + x) * 4) as usize;
            let pixel = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
            for channel in 0..3 {
                let difference = i16::from(pixel[channel]) - i16::from(background[channel]);
                maximum_difference = maximum_difference.max(f32::from(difference.abs()) / 255.0);
            }
        }
    }
    assert!(
        maximum_difference >= 0.05,
        "cursor channel difference {maximum_difference:.3} is too small over {background:?}"
    );
}

fn assert_single_dark_ring(pixels: &[u8], rect: [u32; 4], background: [u8; 3]) {
    let background_luminance = luminance(background);
    let mut maximum_lighter_difference = 0.0_f32;
    let mut maximum_darker_difference = 0.0_f32;
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let offset = ((y * RENDER_SIZE[0] + x) * 4) as usize;
            let pixel = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
            let difference = luminance(pixel) - background_luminance;
            maximum_lighter_difference = maximum_lighter_difference.max(difference);
            maximum_darker_difference = maximum_darker_difference.max(-difference);
        }
    }
    assert!(
        maximum_lighter_difference <= 0.01 && maximum_darker_difference >= 0.05,
        "50% gray cursor should be one dark ring, but its differences are \
         +{maximum_lighter_difference:.3} and -{maximum_darker_difference:.3}"
    );
}

fn strongest_luminance_difference(pixels: &[u8], rect: [u32; 4], background: [u8; 3]) -> f32 {
    let background_luminance = luminance(background);
    let mut strongest_difference = 0.0_f32;
    for y in rect[1]..rect[3] {
        for x in rect[0]..rect[2] {
            let offset = ((y * RENDER_SIZE[0] + x) * 4) as usize;
            let pixel = [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
            let difference = luminance(pixel) - background_luminance;
            if difference.abs() > strongest_difference.abs() {
                strongest_difference = difference;
            }
        }
    }
    strongest_difference
}

fn luminance(rgb: [u8; 3]) -> f32 {
    (0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2])) / 255.0
}
