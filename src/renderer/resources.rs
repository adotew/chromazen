use wgpu::util::DeviceExt;

use super::layers::{LayerId, PaintLayer};
use super::stamps::{MAX_STAMPS_PER_FRAME, StampRaw};
use super::{DOCUMENT_FORMAT, PaintUniform, ViewUniform};

pub(crate) struct RenderResources {
    pub(crate) stamp_buffer: wgpu::Buffer,
    pub(crate) stamp_uniform_buffer: wgpu::Buffer,
    pub(crate) view_uniform_buffer: wgpu::Buffer,
    pub(crate) stamp_bind_group: wgpu::BindGroup,
    brush_texture: wgpu::Texture,
    brush_sampler: wgpu::Sampler,
    paint_sampler: wgpu::Sampler,
    stamp_bind_group_layout: wgpu::BindGroupLayout,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) stamp_pipeline: wgpu::RenderPipeline,
    pub(crate) background_pipeline: wgpu::RenderPipeline,
    pub(crate) layer_pipeline: wgpu::RenderPipeline,
}

impl RenderResources {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        document_size: [u32; 2],
        surface_format: wgpu::TextureFormat,
        preset_stamp: Option<&image::RgbaImage>,
    ) -> Result<Self, String> {
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
                background_color: [1.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bundled_brush;
        let brush_image = if let Some(preset_stamp) = preset_stamp {
            preset_stamp
        } else {
            bundled_brush = image::load_from_memory(include_bytes!("../../assets/charcoal.png"))
                .map_err(|err| format!("failed to load bundled brush stamp: {err}"))?
                .to_rgba8();
            &bundled_brush
        };
        let (brush_texture, brush_texture_view) = create_brush_texture(device, queue, brush_image);
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
                            src_factor: wgpu::BlendFactor::One,
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
        let create_blit_pipeline = |label, entry_point| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blit_shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blit_shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            })
        };
        let background_pipeline =
            create_blit_pipeline("background pipeline", "fs_background");
        let layer_pipeline = create_blit_pipeline("layer pipeline", "fs_layer");

        Ok(Self {
            stamp_buffer,
            stamp_uniform_buffer,
            view_uniform_buffer,
            stamp_bind_group,
            brush_texture,
            brush_sampler,
            paint_sampler,
            stamp_bind_group_layout,
            blit_bind_group_layout,
            stamp_pipeline,
            background_pipeline,
            layer_pipeline,
        })
    }

    pub(crate) fn create_paint_layer(
        &self,
        device: &wgpu::Device,
        size: [u32; 2],
        id: LayerId,
        name: String,
    ) -> PaintLayer {
        let (texture, view) = create_paint_texture(device, size);
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer blit bind group"),
            layout: &self.blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.paint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.view_uniform_buffer.as_entire_binding(),
                },
            ],
        });
        PaintLayer {
            id,
            name,
            texture,
            view,
            blit_bind_group,
        }
    }

    pub(crate) fn replace_brush_stamp(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        preset_stamp: Option<&image::RgbaImage>,
    ) -> Result<(), String> {
        let bundled_brush;
        let brush_image = if let Some(preset_stamp) = preset_stamp {
            preset_stamp
        } else {
            bundled_brush = image::load_from_memory(include_bytes!("../../assets/charcoal.png"))
                .map_err(|error| format!("failed to load bundled brush stamp: {error}"))?
                .to_rgba8();
            &bundled_brush
        };
        let (brush_texture, brush_texture_view) = create_brush_texture(device, queue, brush_image);
        let stamp_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stamp bind group"),
            layout: &self.stamp_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.brush_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&brush_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.stamp_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.stamp_uniform_buffer.as_entire_binding(),
                },
            ],
        });
        self.brush_texture = brush_texture;
        self.stamp_bind_group = stamp_bind_group;
        Ok(())
    }
}

fn create_brush_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    brush_image: &image::RgbaImage,
) -> (wgpu::Texture, wgpu::TextureView) {
    let brush_size = brush_image.dimensions();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
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
            texture: &texture,
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
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
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
