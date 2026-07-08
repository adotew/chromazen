use std::{collections::VecDeque, sync::Arc};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

pub const DEFAULT_CANVAS_WIDTH: u32 = 4000;
pub const DEFAULT_CANVAS_HEIGHT: u32 = 4000;
const MIN_ZOOM: f32 = 0.01;
const MAX_ZOOM: f32 = 32.0;
const MAX_STAMPS_PER_FRAME: usize = 1024;
const MIN_STAMP_SPACING: f32 = 1.0;
const STAMP_SPACING_RATIO: f32 = 0.25;
const BRUSH_STAMP_ASPECT: f32 = 1.0;
const DOCUMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StampRaw {
    center: [f32; 2],
    half_size: [f32; 2],
    color: [f32; 4],
    bounds: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PaintUniform {
    dims: [f32; 2],
    padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    scale: [f32; 2],
    offset: [f32; 2],
    paint_dims: [f32; 2],
    padding: [f32; 2],
}

#[derive(Clone, Copy)]
struct Stamp {
    x: f32,
    y: f32,
    radius: f32,
    rgba: [f32; 4],
}

#[derive(Default)]
struct StampQueue {
    pending: VecDeque<Stamp>,
    distance_since_last_stamp: f32,
}

impl StampQueue {
    fn clear(&mut self) {
        self.pending.clear();
        self.distance_since_last_stamp = 0.0;
    }

    fn queue_stamp(&mut self, stamp: Stamp, width: u32, height: u32) -> bool {
        let bounds = get_stamp_bounds(stamp.x, stamp.y, stamp.radius, width, height);
        if stamp.x + bounds.half_width < 0.0
            || stamp.y + bounds.half_height < 0.0
            || stamp.x - bounds.half_width >= width as f32
            || stamp.y - bounds.half_height >= height as f32
            || bounds.max_x < bounds.min_x
            || bounds.max_y < bounds.min_y
        {
            return false;
        }

        self.pending.push_back(stamp);
        true
    }

    fn stamp_line(&mut self, from: StrokePoint, to: StrokePoint, rgba: [f32; 4], width: u32, height: u32) -> usize {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dist = dx.hypot(dy);
        if dist == 0.0 {
            return 0;
        }

        let mut queued = 0;
        let mut travelled = 0.0;
        while travelled < dist {
            let spacing_t = travelled / dist;
            let spacing_radius = lerp(from.radius, to.radius, spacing_t);
            let spacing = get_stamp_spacing(spacing_radius);
            let distance_to_next_stamp = (spacing - self.distance_since_last_stamp).max(0.0);
            let remaining_distance = dist - travelled;

            if distance_to_next_stamp > remaining_distance {
                self.distance_since_last_stamp += remaining_distance;
                return queued;
            }

            travelled += distance_to_next_stamp;
            let t = travelled / dist;
            let radius = lerp(from.radius, to.radius, t);
            let opacity = lerp(from.opacity, to.opacity, t);
            let x = from.x + dx * t;
            let y = from.y + dy * t;
            let mut color = rgba;
            color[3] = opacity;
            if self.queue_stamp(Stamp { x, y, radius, rgba: color }, width, height) {
                queued += 1;
            }
            self.distance_since_last_stamp = 0.0;
        }

        queued
    }
}

#[derive(Clone, Copy)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub opacity: f32,
}

#[derive(Default, Clone, Copy)]
pub struct PaintStats {
    pub stamps_last_frame: usize,
    pub pending_stamps: usize,
    pub total_stamps: u64,
}

pub struct PaintRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    document_size: [u32; 2],
    _paint_texture: wgpu::Texture,
    paint_texture_view: wgpu::TextureView,
    stamp_buffer: wgpu::Buffer,
    _stamp_uniform_buffer: wgpu::Buffer,
    view_uniform_buffer: wgpu::Buffer,
    stamp_bind_group: wgpu::BindGroup,
    blit_bind_group: wgpu::BindGroup,
    stamp_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    stamp_queue: StampQueue,
    zoom: f32,
    offset: [f32; 2],
    stats: PaintStats,
}

impl PaintRenderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|err| format!("failed to create surface: {err}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|err| format!("failed to find a suitable GPU adapter: {err}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("minipaint-rs device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: Default::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| format!("failed to create device: {err}"))?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .unwrap_or_else(|_| caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![surface_format],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let document_size = [DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT];
        let (paint_texture, paint_texture_view) = create_paint_texture(&device, document_size);

        let stamp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stamp storage buffer"),
            size: (MAX_STAMPS_PER_FRAME * std::mem::size_of::<StampRaw>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stamp_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stamp uniform buffer"),
            contents: bytemuck::bytes_of(&PaintUniform {
                dims: [document_size[0] as f32, document_size[1] as f32],
                padding: [0.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view uniform buffer"),
            contents: bytemuck::bytes_of(&ViewUniform {
                scale: [1.0, 1.0],
                offset: [0.0, 0.0],
                paint_dims: [document_size[0] as f32, document_size[1] as f32],
                padding: [0.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let brush_image = image::load_from_memory(include_bytes!("../assets/charcoal-removebg-preview.png"))
            .map_err(|err| format!("failed to load brush stamp: {err}"))?
            .to_rgba8();
        let brush_size = brush_image.dimensions();
        let brush_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("brush stamp texture"),
            size: wgpu::Extent3d { width: brush_size.0, height: brush_size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DOCUMENT_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &brush_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            brush_image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * brush_size.0),
                rows_per_image: Some(brush_size.1),
            },
            wgpu::Extent3d { width: brush_size.0, height: brush_size.1, depth_or_array_layers: 1 },
        );
        let brush_texture_view = brush_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let brush_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("brush sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let paint_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("paint sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let stamp_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stamp bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let stamp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stamp bind group"),
            layout: &stamp_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&brush_sampler) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&brush_texture_view) },
                wgpu::BindGroupEntry { binding: 2, resource: stamp_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: stamp_uniform_buffer.as_entire_binding() },
            ],
        });

        let blit_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bind group"),
            layout: &blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&paint_sampler) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&paint_texture_view) },
                wgpu::BindGroupEntry { binding: 2, resource: view_uniform_buffer.as_entire_binding() },
            ],
        });

        let stamp_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stamp pipeline layout"),
            bind_group_layouts: &[Some(&stamp_bind_group_layout)],
            immediate_size: 0,
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit pipeline layout"),
            bind_group_layouts: &[Some(&blit_bind_group_layout)],
            immediate_size: 0,
        });
        let stamp_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stamp shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/stamp.wgsl").into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blit.wgsl").into()),
        });

        let stamp_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stamp pipeline"),
            layout: Some(&stamp_pipeline_layout),
            vertex: wgpu::VertexState { module: &stamp_shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &stamp_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DOCUMENT_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { operation: wgpu::BlendOperation::Add, src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha },
                        alpha: wgpu::BlendComponent { operation: wgpu::BlendOperation::Add, src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState { module: &blit_shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: surface_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            document_size,
            _paint_texture: paint_texture,
            paint_texture_view,
            stamp_buffer,
            _stamp_uniform_buffer: stamp_uniform_buffer,
            view_uniform_buffer,
            stamp_bind_group,
            blit_bind_group,
            stamp_pipeline,
            blit_pipeline,
            stamp_queue: StampQueue::default(),
            zoom: 1.0,
            offset: [0.0, 0.0],
            stats: PaintStats::default(),
        };
        renderer.fit_to_screen();
        renderer.clear_canvas();
        Ok(renderer)
    }

    pub fn device(&self) -> &wgpu::Device { &self.device }
    pub fn queue(&self) -> &wgpu::Queue { &self.queue }
    pub fn surface_format(&self) -> wgpu::TextureFormat { self.config.format }
    pub fn surface_size(&self) -> [u32; 2] { [self.config.width, self.config.height] }
    pub fn document_size(&self) -> [u32; 2] { self.document_size }
    pub fn zoom(&self) -> f32 { self.zoom }
    pub fn offset(&self) -> [f32; 2] { self.offset }
    pub fn stats(&self) -> PaintStats { self.stats }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn fit_to_screen(&mut self) {
        let zoom = (self.config.width as f32 / self.document_size[0] as f32)
            .min(self.config.height as f32 / self.document_size[1] as f32)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom = zoom;
        let visible_width = self.config.width as f32 / zoom;
        let visible_height = self.config.height as f32 / zoom;
        self.offset = [
            (self.document_size[0] as f32 - visible_width) * 0.5,
            (self.document_size[1] as f32 - visible_height) * 0.5,
        ];
    }

    pub fn zoom_to_100(&mut self) {
        self.zoom = 1.0;
        self.offset = [0.0, 0.0];
    }

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        let old = self.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new - old).abs() <= f32::EPSILON {
            return;
        }
        self.zoom = new;
        self.offset[0] += cursor[0] * (1.0 / old - 1.0 / new);
        self.offset[1] += cursor[1] * (1.0 / old - 1.0 / new);
    }

    pub fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.offset[0] -= delta[0] / self.zoom;
        self.offset[1] -= delta[1] / self.zoom;
    }

    pub fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        [point[0] / self.zoom + self.offset[0], point[1] / self.zoom + self.offset[1]]
    }

    pub fn begin_stroke(&mut self) {
        self.stamp_queue.distance_since_last_stamp = 0.0;
    }

    pub fn end_stroke(&mut self) {
        self.stamp_queue.distance_since_last_stamp = 0.0;
    }

    pub fn queue_stamp(&mut self, point: StrokePoint, color: [f32; 4]) -> bool {
        let mut rgba = color;
        rgba[3] = point.opacity;
        self.stamp_queue.queue_stamp(
            Stamp { x: point.x, y: point.y, radius: point.radius, rgba },
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn stamp_line(&mut self, from: StrokePoint, to: StrokePoint, color: [f32; 4]) -> usize {
        self.stamp_queue.stamp_line(from, to, color, self.document_size[0], self.document_size[1])
    }

    pub fn clear_canvas(&mut self) {
        self.stamp_queue.clear();
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clear canvas encoder") });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.paint_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.stats = PaintStats::default();
    }

    pub fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }

    pub fn reconfigure_surface(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render_to_view(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        self.stats.stamps_last_frame = 0;
        let count = self.flush_stamps(encoder);
        self.stats.stamps_last_frame = count;
        self.stats.pending_stamps = self.stamp_queue.pending.len();
        self.stats.total_stamps += count as u64;
        self.write_view_uniform();

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blit_pipeline);
        pass.set_bind_group(0, &self.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn flush_stamps(&mut self, encoder: &mut wgpu::CommandEncoder) -> usize {
        let count = self.stamp_queue.pending.len().min(MAX_STAMPS_PER_FRAME);
        if count == 0 {
            return 0;
        }

        let mut raw = Vec::with_capacity(count);
        for _ in 0..count {
            let stamp = self.stamp_queue.pending.pop_front().expect("count checked");
            raw.push(stamp_to_raw(stamp, self.document_size[0], self.document_size[1]));
        }
        self.queue.write_buffer(&self.stamp_buffer, 0, bytemuck::cast_slice(&raw));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.paint_texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.stamp_pipeline);
        pass.set_bind_group(0, &self.stamp_bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
        count
    }

    fn write_view_uniform(&self) {
        self.queue.write_buffer(
            &self.view_uniform_buffer,
            0,
            bytemuck::bytes_of(&ViewUniform {
                scale: [1.0 / self.zoom, 1.0 / self.zoom],
                offset: self.offset,
                paint_dims: [self.document_size[0] as f32, self.document_size[1] as f32],
                padding: [0.0, 0.0],
            }),
        );
    }
}

fn create_paint_texture(device: &wgpu::Device, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("paint texture"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DOCUMENT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn stamp_to_raw(stamp: Stamp, width: u32, height: u32) -> StampRaw {
    let bounds = get_stamp_bounds(stamp.x, stamp.y, stamp.radius, width, height);
    StampRaw {
        center: [stamp.x, stamp.y],
        half_size: [bounds.half_width, bounds.half_height],
        color: stamp.rgba,
        bounds: [bounds.min_x as f32, bounds.min_y as f32, bounds.max_x as f32, bounds.max_y as f32],
    }
}

struct StampBounds {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    half_width: f32,
    half_height: f32,
}

fn get_stamp_half_size(radius: f32) -> (f32, f32) {
    if BRUSH_STAMP_ASPECT >= 1.0 {
        (radius, radius / BRUSH_STAMP_ASPECT)
    } else {
        (radius * BRUSH_STAMP_ASPECT, radius)
    }
}

fn get_stamp_bounds(x: f32, y: f32, radius: f32, width: u32, height: u32) -> StampBounds {
    let (half_width, half_height) = get_stamp_half_size(radius);
    let min_x = 0.max((x - half_width).floor() as i32);
    let max_x = (width as i32 - 1).min((x + half_width).ceil() as i32);
    let min_y = 0.max((y - half_height).floor() as i32);
    let max_y = (height as i32 - 1).min((y + half_height).ceil() as i32);
    StampBounds { min_x, max_x, min_y, max_y, half_width, half_height }
}

fn get_stamp_spacing(radius: f32) -> f32 {
    MIN_STAMP_SPACING.max(radius * STAMP_SPACING_RATIO)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
