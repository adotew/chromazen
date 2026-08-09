use wgpu::util::DeviceExt;

use super::layers::{LayerId, LayerProperties, LayerResourceId, PaintLayer};
use super::stamps::{StampRaw, MAX_STAMPS_PER_FRAME};
use super::{
    CursorRaw, LayerPreviewUniform, LayerSettingsUniform, LayerTransform, PaintUniform,
    StrokeUniform, ViewUniform, DOCUMENT_FORMAT, LAYER_PREVIEW_SIZE, STROKE_MASK_FORMAT,
};

pub(crate) struct RenderResources {
    pub(crate) stamp_buffer: wgpu::Buffer,
    pub(crate) cursor_buffer: wgpu::Buffer,
    pub(crate) stamp_uniform_buffer: wgpu::Buffer,
    pub(crate) view_uniform_buffer: wgpu::Buffer,
    stroke_uniform_buffer: wgpu::Buffer,
    layer_preview_uniform_buffer: wgpu::Buffer,
    transform_uniform_buffer: wgpu::Buffer,
    pub(crate) stamp_bind_group: wgpu::BindGroup,
    pub(crate) cursor_bind_group: wgpu::BindGroup,
    pub(crate) smudge_texture: wgpu::Texture,
    smudge_texture_view: wgpu::TextureView,
    _clipping_group_texture: wgpu::Texture,
    pub(crate) clipping_group_view: wgpu::TextureView,
    clipping_group_settings_buffer: wgpu::Buffer,
    clipping_group_bind_group: wgpu::BindGroup,
    _stroke_mask_texture: wgpu::Texture,
    pub(crate) stroke_mask_view: wgpu::TextureView,
    brush_texture: wgpu::Texture,
    brush_texture_view: wgpu::TextureView,
    brush_sampler: wgpu::Sampler,
    paint_sampler: wgpu::Sampler,
    preview_sampler: wgpu::Sampler,
    stamp_bind_group_layout: wgpu::BindGroupLayout,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    clipped_layer_bind_group_layout: wgpu::BindGroupLayout,
    stroke_preview_bind_group_layout: wgpu::BindGroupLayout,
    stroke_preview_bind_group: Option<wgpu::BindGroup>,
    layer_preview_bind_group_layout: wgpu::BindGroupLayout,
    stroke_commit_bind_group_layout: wgpu::BindGroupLayout,
    transform_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) stroke_commit_bind_group: wgpu::BindGroup,
    pub(crate) mask_pipeline: wgpu::RenderPipeline,
    pub(crate) mask_clear_pipeline: wgpu::RenderPipeline,
    pub(crate) smudge_pipeline: wgpu::RenderPipeline,
    pub(crate) cursor_pipeline: wgpu::RenderPipeline,
    pub(crate) background_pipeline: wgpu::RenderPipeline,
    pub(crate) layer_pipeline: wgpu::RenderPipeline,
    pub(crate) clipped_layer_merge_pipeline: wgpu::RenderPipeline,
    pub(crate) merge_pipeline: wgpu::RenderPipeline,
    pub(crate) brush_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) eraser_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) group_brush_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) group_eraser_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) group_clipped_brush_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) group_clipped_eraser_preview_pipeline: wgpu::RenderPipeline,
    pub(crate) layer_thumbnail_pipeline: wgpu::RenderPipeline,
    pub(crate) brush_commit_pipeline: wgpu::RenderPipeline,
    pub(crate) eraser_commit_pipeline: wgpu::RenderPipeline,
    pub(crate) transform_pipeline: wgpu::RenderPipeline,
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
        let cursor_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brush cursor storage buffer"),
            size: std::mem::size_of::<CursorRaw>() as u64,
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
                document_from_window_x: [1.0, 0.0, 0.0, 0.0],
                document_from_window_y: [0.0, 1.0, 0.0, 0.0],
                paint_dims: [document_size[0] as f32, document_size[1] as f32],
                padding: [0.0, 0.0],
                background_color: [1.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let stroke_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stroke uniform buffer"),
            contents: bytemuck::bytes_of(&StrokeUniform { color: [0.0; 4] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layer_preview_uniform_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("layer preview uniform buffer"),
                contents: bytemuck::bytes_of(&LayerPreviewUniform {
                    preview_dims: [LAYER_PREVIEW_SIZE as f32; 2],
                    document_dims: [document_size[0] as f32, document_size[1] as f32],
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let transform_uniform_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("layer transform uniform buffer"),
                contents: bytemuck::bytes_of(
                    &LayerTransform::default()
                        .uniform([document_size[0] as f32 * 0.5, document_size[1] as f32 * 0.5]),
                ),
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
        let preview_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer preview sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (smudge_texture, smudge_texture_view) = create_paint_texture(device, document_size);
        let (clipping_group_texture, clipping_group_view) =
            create_paint_texture(device, document_size);
        let clipping_group_settings_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("clipping group settings uniform buffer"),
                contents: bytemuck::bytes_of(&LayerSettingsUniform {
                    opacity: 1.0,
                    padding: [0.0; 3],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let (stroke_mask_texture, stroke_mask_view) =
            create_stroke_mask_texture(device, document_size);
        clear_stroke_mask(device, queue, &stroke_mask_view);

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
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&smudge_texture_view),
                },
            ],
        });
        let cursor_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush cursor bind group"),
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
                    resource: cursor_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: stamp_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&smudge_texture_view),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
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
        let clipped_layer_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("clipped layer bind group layout"),
                entries: &[
                    sampler_layout_entry(0),
                    texture_layout_entry(1),
                    texture_layout_entry(2),
                    uniform_layout_entry(3),
                    uniform_layout_entry(4),
                    uniform_layout_entry(5),
                ],
            });
        let stroke_preview_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("stroke preview bind group layout"),
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
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    texture_layout_entry(6),
                    uniform_layout_entry(7),
                ],
            });
        let layer_preview_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("layer preview bind group layout"),
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
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
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
        let stroke_commit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("stroke commit bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
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
        let transform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("layer transform bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
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
        let stroke_commit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stroke commit bind group"),
            layout: &stroke_commit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&stroke_mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: stroke_uniform_buffer.as_entire_binding(),
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
        let clipping_group_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clipping group blit bind group"),
            layout: &blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&paint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&clipping_group_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: view_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: clipping_group_settings_buffer.as_entire_binding(),
                },
            ],
        });
        let clipped_layer_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("clipped layer pipeline layout"),
                bind_group_layouts: &[Some(&clipped_layer_bind_group_layout)],
                immediate_size: 0,
            });
        let stroke_preview_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("stroke preview pipeline layout"),
                bind_group_layouts: &[Some(&stroke_preview_bind_group_layout)],
                immediate_size: 0,
            });
        let layer_preview_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("layer preview pipeline layout"),
                bind_group_layouts: &[Some(&layer_preview_bind_group_layout)],
                immediate_size: 0,
            });
        let stroke_commit_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("stroke commit pipeline layout"),
                bind_group_layouts: &[Some(&stroke_commit_bind_group_layout)],
                immediate_size: 0,
            });
        let transform_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("layer transform pipeline layout"),
                bind_group_layouts: &[Some(&transform_bind_group_layout)],
                immediate_size: 0,
            });
        let stamp_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stamp shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/stamp.wgsl").into()),
        });
        let smudge_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smudge shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/smudge.wgsl").into()),
        });
        let cursor_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brush cursor shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/cursor.wgsl").into()),
        });
        let mask_clear_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stroke mask clear shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/mask_clear.wgsl").into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blit.wgsl").into()),
        });
        let clipped_layer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clipped layer shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/clipped_layer.wgsl").into()),
        });
        let merge_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer merge shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/merge.wgsl").into()),
        });
        let stroke_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stroke composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/stroke_composite.wgsl").into()),
        });
        let layer_preview_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer preview shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/layer_preview.wgsl").into()),
        });
        let transform_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer transform shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/transform.wgsl").into()),
        });

        let create_stamp_pipeline =
            |label, shader: &wgpu::ShaderModule, source_factor: Option<wgpu::BlendFactor>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&stamp_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: DOCUMENT_FORMAT,
                            blend: source_factor.map(|source_factor| wgpu::BlendState {
                                color: wgpu::BlendComponent {
                                    operation: wgpu::BlendOperation::Add,
                                    src_factor: source_factor,
                                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                },
                                alpha: wgpu::BlendComponent {
                                    operation: wgpu::BlendOperation::Add,
                                    src_factor: source_factor,
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
                })
            };
        let smudge_pipeline = create_stamp_pipeline("smudge pipeline", &smudge_shader, None);
        let mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke mask pipeline"),
            layout: Some(&stamp_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &stamp_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &stamp_shader,
                entry_point: Some("fs_mask"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: STROKE_MASK_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Max,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                        },
                        alpha: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Max,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::RED,
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
        let mask_clear_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke mask clear pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &mask_clear_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_clear_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: STROKE_MASK_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::RED,
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
        let cursor_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brush cursor pipeline"),
            layout: Some(&stamp_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cursor_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &cursor_shader,
                entry_point: Some("fs"),
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
        let background_pipeline = create_blit_pipeline("background pipeline", "fs_background");
        let layer_pipeline = create_blit_pipeline("layer pipeline", "fs_layer");
        let clipping_group_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                operation: wgpu::BlendOperation::Add,
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            },
            // A clipping group inherits its alpha exclusively from the base layer.
            alpha: wgpu::BlendComponent {
                operation: wgpu::BlendOperation::Add,
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
            },
        };
        let clipped_layer_merge_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("clipped layer merge pipeline"),
                layout: Some(&clipped_layer_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &clipped_layer_shader,
                    entry_point: Some("vs_merge"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &clipped_layer_shader,
                    entry_point: Some("fs_merge"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DOCUMENT_FORMAT,
                        blend: Some(clipping_group_blend),
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
        let merge_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("layer merge pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &merge_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &merge_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DOCUMENT_FORMAT,
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
        });
        let create_preview_pipeline = |label, entry_point| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&stroke_preview_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &stroke_composite_shader,
                    entry_point: Some("vs_preview"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stroke_composite_shader,
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
        let brush_preview_pipeline =
            create_preview_pipeline("brush stroke preview pipeline", "fs_preview_brush");
        let eraser_preview_pipeline =
            create_preview_pipeline("eraser stroke preview pipeline", "fs_preview_eraser");
        let create_group_preview_pipeline = |label, entry_point, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&stroke_preview_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &stroke_composite_shader,
                    entry_point: Some("vs_group"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stroke_composite_shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DOCUMENT_FORMAT,
                        blend: Some(blend),
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
        let group_brush_preview_pipeline = create_group_preview_pipeline(
            "clipping group brush stroke preview pipeline",
            "fs_group_preview_brush",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let group_eraser_preview_pipeline = create_group_preview_pipeline(
            "clipping group eraser stroke preview pipeline",
            "fs_group_preview_eraser",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let group_clipped_brush_preview_pipeline = create_group_preview_pipeline(
            "clipping group clipped brush stroke preview pipeline",
            "fs_group_preview_clipped_brush",
            clipping_group_blend,
        );
        let group_clipped_eraser_preview_pipeline = create_group_preview_pipeline(
            "clipping group clipped eraser stroke preview pipeline",
            "fs_group_preview_clipped_eraser",
            clipping_group_blend,
        );
        let create_thumbnail_pipeline = |label, entry_point| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layer_preview_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &layer_preview_shader,
                    entry_point: Some("vs_preview"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &layer_preview_shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DOCUMENT_FORMAT,
                        blend: None,
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
        let layer_thumbnail_pipeline =
            create_thumbnail_pipeline("layer thumbnail pipeline", "fs_layer");
        let create_commit_pipeline = |label, entry_point, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&stroke_commit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &stroke_composite_shader,
                    entry_point: Some("vs_commit"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stroke_composite_shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DOCUMENT_FORMAT,
                        blend: Some(blend),
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
        let brush_commit_pipeline = create_commit_pipeline(
            "brush stroke commit pipeline",
            "fs_commit_brush",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let erase_blend = wgpu::BlendComponent {
            operation: wgpu::BlendOperation::Add,
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        };
        let eraser_commit_pipeline = create_commit_pipeline(
            "eraser stroke commit pipeline",
            "fs_commit_eraser",
            wgpu::BlendState {
                color: erase_blend,
                alpha: erase_blend,
            },
        );
        let transform_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("layer transform pipeline"),
            layout: Some(&transform_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &transform_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &transform_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DOCUMENT_FORMAT,
                    blend: None,
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

        Ok(Self {
            stamp_buffer,
            cursor_buffer,
            stamp_uniform_buffer,
            view_uniform_buffer,
            stroke_uniform_buffer,
            layer_preview_uniform_buffer,
            transform_uniform_buffer,
            stamp_bind_group,
            cursor_bind_group,
            smudge_texture,
            smudge_texture_view,
            _clipping_group_texture: clipping_group_texture,
            clipping_group_view,
            clipping_group_settings_buffer,
            clipping_group_bind_group,
            _stroke_mask_texture: stroke_mask_texture,
            stroke_mask_view,
            brush_texture,
            brush_texture_view,
            brush_sampler,
            paint_sampler,
            preview_sampler,
            stamp_bind_group_layout,
            blit_bind_group_layout,
            clipped_layer_bind_group_layout,
            stroke_preview_bind_group_layout,
            stroke_preview_bind_group: None,
            layer_preview_bind_group_layout,
            stroke_commit_bind_group_layout,
            transform_bind_group_layout,
            stroke_commit_bind_group,
            mask_pipeline,
            mask_clear_pipeline,
            smudge_pipeline,
            cursor_pipeline,
            background_pipeline,
            layer_pipeline,
            clipped_layer_merge_pipeline,
            merge_pipeline,
            brush_preview_pipeline,
            eraser_preview_pipeline,
            group_brush_preview_pipeline,
            group_eraser_preview_pipeline,
            group_clipped_brush_preview_pipeline,
            group_clipped_eraser_preview_pipeline,
            layer_thumbnail_pipeline,
            brush_commit_pipeline,
            eraser_commit_pipeline,
            transform_pipeline,
        })
    }

    pub(crate) fn create_transform_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer transform bind group"),
            layout: &self.transform_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.transform_uniform_buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub(crate) fn write_transform(
        &self,
        queue: &wgpu::Queue,
        transform: LayerTransform,
        pivot: [f32; 2],
    ) {
        queue.write_buffer(
            &self.transform_uniform_buffer,
            0,
            bytemuck::bytes_of(&transform.uniform(pivot)),
        );
    }

    pub(crate) fn prepare_stroke_preview(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer_view: &wgpu::TextureView,
        layer_settings_buffer: &wgpu::Buffer,
        clipping_base: Option<(&wgpu::TextureView, &wgpu::Buffer)>,
        color: [f32; 4],
    ) {
        queue.write_buffer(
            &self.stroke_uniform_buffer,
            0,
            bytemuck::bytes_of(&StrokeUniform { color }),
        );
        let (base_view, base_settings_buffer) =
            clipping_base.unwrap_or((layer_view, layer_settings_buffer));
        self.stroke_preview_bind_group =
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("stroke preview bind group"),
                layout: &self.stroke_preview_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.paint_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(layer_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.stroke_mask_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.view_uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.stroke_uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: layer_settings_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(base_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: base_settings_buffer.as_entire_binding(),
                    },
                ],
            }));
    }

    pub(crate) fn stroke_preview_bind_group(&self) -> &wgpu::BindGroup {
        self.stroke_preview_bind_group
            .as_ref()
            .expect("brush and eraser strokes require a preview bind group")
    }

    pub(crate) fn clipping_group_bind_group(&self) -> &wgpu::BindGroup {
        &self.clipping_group_bind_group
    }

    pub(crate) fn clear_stroke_preview(&mut self) {
        self.stroke_preview_bind_group = None;
    }

    pub(crate) fn resize_document(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        document_size: [u32; 2],
    ) {
        queue.write_buffer(
            &self.stamp_uniform_buffer,
            0,
            bytemuck::bytes_of(&PaintUniform {
                dims: [document_size[0] as f32, document_size[1] as f32],
                padding: [0.0; 2],
            }),
        );
        queue.write_buffer(
            &self.layer_preview_uniform_buffer,
            0,
            bytemuck::bytes_of(&LayerPreviewUniform {
                preview_dims: [LAYER_PREVIEW_SIZE as f32; 2],
                document_dims: [document_size[0] as f32, document_size[1] as f32],
            }),
        );

        let (smudge_texture, smudge_texture_view) = create_paint_texture(device, document_size);
        let (clipping_group_texture, clipping_group_view) =
            create_paint_texture(device, document_size);
        let clipping_group_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clipping group blit bind group"),
            layout: &self.blit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.paint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&clipping_group_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.view_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.clipping_group_settings_buffer.as_entire_binding(),
                },
            ],
        });
        let (stroke_mask_texture, stroke_mask_view) =
            create_stroke_mask_texture(device, document_size);
        clear_stroke_mask(device, queue, &stroke_mask_view);
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
                    resource: wgpu::BindingResource::TextureView(&self.brush_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.stamp_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.stamp_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&smudge_texture_view),
                },
            ],
        });
        let cursor_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush cursor bind group"),
            layout: &self.stamp_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.brush_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.brush_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.cursor_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.stamp_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&smudge_texture_view),
                },
            ],
        });
        let stroke_commit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stroke commit bind group"),
            layout: &self.stroke_commit_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&stroke_mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.stroke_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        self.smudge_texture = smudge_texture;
        self.smudge_texture_view = smudge_texture_view;
        self._clipping_group_texture = clipping_group_texture;
        self.clipping_group_view = clipping_group_view;
        self.clipping_group_bind_group = clipping_group_bind_group;
        self._stroke_mask_texture = stroke_mask_texture;
        self.stroke_mask_view = stroke_mask_view;
        self.stamp_bind_group = stamp_bind_group;
        self.cursor_bind_group = cursor_bind_group;
        self.stroke_commit_bind_group = stroke_commit_bind_group;
        self.stroke_preview_bind_group = None;
    }

    pub(crate) fn create_paint_layer(
        &self,
        device: &wgpu::Device,
        size: [u32; 2],
        id: LayerId,
        resource_id: LayerResourceId,
        properties: LayerProperties,
    ) -> PaintLayer {
        let (texture, view) = create_paint_texture(device, size);
        let settings_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("layer settings uniform buffer"),
            contents: bytemuck::bytes_of(&LayerSettingsUniform {
                opacity: f32::from(properties.opacity) / 100.0,
                padding: [0.0; 3],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: settings_buffer.as_entire_binding(),
                },
            ],
        });
        let (preview_texture, preview_view) = create_layer_preview_texture(device);
        let preview_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer preview bind group"),
            layout: &self.layer_preview_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.preview_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.stroke_mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.stroke_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.layer_preview_uniform_buffer.as_entire_binding(),
                },
            ],
        });
        PaintLayer {
            id,
            resource_id,
            name: properties.name,
            visible: properties.visible,
            opacity: properties.opacity,
            clipped: properties.clipped,
            settings_buffer,
            texture,
            view,
            blit_bind_group,
            _preview_texture: preview_texture,
            preview_view,
            preview_bind_group,
            preview_dirty: true,
        }
    }

    pub(crate) fn create_clipped_layer_bind_group(
        &self,
        device: &wgpu::Device,
        layer: &PaintLayer,
        base: &PaintLayer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clipped layer bind group"),
            layout: &self.clipped_layer_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.paint_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&layer.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&base.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.view_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: layer.settings_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: base.settings_buffer.as_entire_binding(),
                },
            ],
        })
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
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.smudge_texture_view),
                },
            ],
        });
        let cursor_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush cursor bind group"),
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
                    resource: self.cursor_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.stamp_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.smudge_texture_view),
                },
            ],
        });
        self.brush_texture = brush_texture;
        self.brush_texture_view = brush_texture_view;
        self.stamp_bind_group = stamp_bind_group;
        self.cursor_bind_group = cursor_bind_group;
        Ok(())
    }
}

fn sampler_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}

fn uniform_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
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

fn clear_stroke_mask(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("stroke mask initialization encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stroke mask initialization pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
    }
    queue.submit(std::iter::once(encoder.finish()));
}

fn create_stroke_mask_texture(
    device: &wgpu::Device,
    size: [u32; 2],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("stroke mask texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: STROKE_MASK_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_layer_preview_texture(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("layer preview texture"),
        size: wgpu::Extent3d {
            width: LAYER_PREVIEW_SIZE,
            height: LAYER_PREVIEW_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DOCUMENT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
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
