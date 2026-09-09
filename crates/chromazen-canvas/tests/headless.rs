use chromazen_canvas::{Canvas, PaintTool, StrokePoint};

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
