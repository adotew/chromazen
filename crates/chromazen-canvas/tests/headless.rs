use std::sync::mpsc;

use chromazen_canvas::{BrushCursor, Canvas, PaintTool, StrokePoint};

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

    run_brush_cursor_contrast(&device, &queue);
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

fn run_brush_cursor_contrast(device: &wgpu::Device, queue: &wgpu::Queue) {
    let brush = image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));

    // A narrow document leaves workspace visible on both sides. This catches the original
    // regression because the workspace is exactly 50% gray.
    let mut workspace_canvas = Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        RENDER_SIZE,
        [16, 32],
        &brush,
    )
    .expect("workspace canvas");
    let pixels = render_pixels(
        device,
        queue,
        &mut workspace_canvas,
        BrushCursor {
            center: [8.0, 32.0],
            diameter: 4.0,
        },
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
        )
        .expect("canvas");
        canvas.set_background_color(background);
        let pixels = render_pixels(
            device,
            queue,
            &mut canvas,
            BrushCursor {
                center: [32.0, 32.0],
                diameter: 8.0,
            },
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

fn render_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    canvas: &mut Canvas,
    cursor: BrushCursor,
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
    canvas.render_to_view(&mut encoder, &view, Some(cursor));
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
