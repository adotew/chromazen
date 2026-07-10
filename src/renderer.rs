use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

mod stamps;
mod view;

use self::{
    stamps::{MAX_STAMPS_PER_FRAME, StampQueue, StampRaw},
    view::PaintView,
};
pub use crate::brush::StrokePoint;
use crate::constants::{DEFAULT_CANVAS_HEIGHT, DEFAULT_CANVAS_WIDTH};

const DOCUMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
    view: PaintView,
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

        let brush_image =
            image::load_from_memory(include_bytes!("../assets/charcoal-removebg-preview.png"))
                .map_err(|err| format!("failed to load brush stamp: {err}"))?
                .to_rgba8();
        let brush_size = brush_image.dimensions();
        let brush_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("brush stamp texture"),
            size: wgpu::Extent3d {
                width: brush_size.0,
                height: brush_size.1,
                depth_or_array_layers: 1,
            },
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
            wgpu::Extent3d {
                width: brush_size.0,
                height: brush_size.1,
                depth_or_array_layers: 1,
            },
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

        let stamp_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("stamp bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let stamp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stamp bind group"),
            layout: &stamp_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&brush_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&brush_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: stamp_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: stamp_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blit bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bind group"),
            layout: &blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&paint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&paint_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: view_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let stamp_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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
            vertex: wgpu::VertexState {
                module: &stamp_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &stamp_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DOCUMENT_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                        alpha: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
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
            view: PaintView::default(),
            stats: PaintStats::default(),
        };
        renderer.fit_to_screen();
        renderer.clear_canvas();
        Ok(renderer)
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
    pub fn surface_size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }
    pub fn document_size(&self) -> [u32; 2] {
        self.document_size
    }
    pub fn zoom(&self) -> f32 {
        self.view.zoom()
    }
    pub fn offset(&self) -> [f32; 2] {
        self.view.offset()
    }
    pub fn stats(&self) -> PaintStats {
        self.stats
    }

    pub fn has_pending_stamps(&self) -> bool {
        self.stamp_queue.has_pending()
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn fit_to_screen(&mut self) {
        self.view
            .fit_to_screen(self.surface_size(), self.document_size);
    }

    pub fn zoom_to_100(&mut self) {
        self.view.zoom_to_100();
    }

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        self.view.apply_zoom_at(factor, cursor);
    }

    pub fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.view.pan_by_window_delta(delta);
    }

    pub fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.window_to_document(point)
    }

    pub fn begin_stroke(&mut self) {
        self.stamp_queue.reset_spacing();
    }

    pub fn end_stroke(&mut self) {
        self.stamp_queue.reset_spacing();
    }

    pub fn queue_stamp(&mut self, point: StrokePoint, color: [f32; 4]) -> bool {
        self.stamp_queue
            .queue_point(point, color, self.document_size[0], self.document_size[1])
    }

    pub fn stamp_line(&mut self, from: StrokePoint, to: StrokePoint, color: [f32; 4]) -> usize {
        self.stamp_queue.stamp_line(
            from,
            to,
            color,
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn clear_canvas(&mut self) {
        self.stamp_queue.clear();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear canvas encoder"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.paint_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
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
        self.stats.pending_stamps = self.stamp_queue.pending_len();
        self.stats.total_stamps += count as u64;
        self.write_view_uniform();

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.5,
                        b: 0.5,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
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
        let raw = self.stamp_queue.drain_raw(
            self.document_size[0],
            self.document_size[1],
            MAX_STAMPS_PER_FRAME,
        );
        let count = raw.len();
        if count == 0 {
            return 0;
        }

        self.queue
            .write_buffer(&self.stamp_buffer, 0, bytemuck::cast_slice(&raw));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.paint_texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
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
                scale: [1.0 / self.view.zoom(), 1.0 / self.view.zoom()],
                offset: self.view.offset(),
                paint_dims: [self.document_size[0] as f32, self.document_size[1] as f32],
                padding: [0.0, 0.0],
            }),
        );
    }
}

fn create_paint_texture(
    device: &wgpu::Device,
    size: [u32; 2],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("paint texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DOCUMENT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
