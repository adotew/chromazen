const FROST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DOWNSAMPLE_FACTOR: u32 = 4;

pub(super) struct FrostRenderer {
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    downsample_pipeline: wgpu::RenderPipeline,
    horizontal_pipeline: wgpu::RenderPipeline,
    vertical_pipeline: wgpu::RenderPipeline,
    textures: FrostTextures,
    generation: u64,
}

struct FrostTextures {
    _first: wgpu::Texture,
    first_view: wgpu::TextureView,
    first_bind_group: wgpu::BindGroup,
    _second: wgpu::Texture,
    second_view: wgpu::TextureView,
    second_bind_group: wgpu::BindGroup,
}

impl FrostRenderer {
    pub(super) fn new(device: &wgpu::Device, surface_size: [u32; 2]) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frost texture bind group layout"),
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
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frost sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frost pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frost shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("frost.wgsl").into()),
        });
        let create_pipeline = |label, entry_point| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FROST_FORMAT,
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
        let downsample_pipeline = create_pipeline("frost downsample pipeline", "fs_downsample");
        let horizontal_pipeline = create_pipeline("frost horizontal pipeline", "fs_horizontal");
        let vertical_pipeline = create_pipeline("frost vertical pipeline", "fs_vertical");
        let textures = create_textures(device, surface_size, &bind_group_layout, &sampler);

        Self {
            bind_group_layout,
            sampler,
            downsample_pipeline,
            horizontal_pipeline,
            vertical_pipeline,
            textures,
            generation: 1,
        }
    }

    pub(super) fn view(&self) -> &wgpu::TextureView {
        &self.textures.first_view
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn resize(&mut self, device: &wgpu::Device, surface_size: [u32; 2]) {
        self.textures =
            create_textures(device, surface_size, &self.bind_group_layout, &self.sampler);
        self.generation = self.generation.wrapping_add(1);
    }

    pub(super) fn render(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
    ) {
        let source_group = self.create_bind_group(device, "frost surface bind group", source);
        render_pass(
            encoder,
            "frost downsample pass",
            &self.textures.first_view,
            &self.downsample_pipeline,
            &source_group,
        );

        render_pass(
            encoder,
            "frost horizontal pass",
            &self.textures.second_view,
            &self.horizontal_pipeline,
            &self.textures.first_bind_group,
        );

        render_pass(
            encoder,
            "frost vertical pass",
            &self.textures.first_view,
            &self.vertical_pipeline,
            &self.textures.second_bind_group,
        );
    }

    fn create_bind_group(
        &self,
        device: &wgpu::Device,
        label: &'static str,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
            ],
        })
    }
}

fn create_textures(
    device: &wgpu::Device,
    surface_size: [u32; 2],
    bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> FrostTextures {
    let size = wgpu::Extent3d {
        width: surface_size[0].div_ceil(DOWNSAMPLE_FACTOR).max(1),
        height: surface_size[1].div_ceil(DOWNSAMPLE_FACTOR).max(1),
        depth_or_array_layers: 1,
    };
    let create = |label| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FROST_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    };
    let (first, first_view) = create("frost first texture");
    let (second, second_view) = create("frost second texture");
    let create_bind_group = |label, view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
            ],
        })
    };
    let first_bind_group = create_bind_group("frost first texture bind group", &first_view);
    let second_bind_group = create_bind_group("frost second texture bind group", &second_view);
    FrostTextures {
        _first: first,
        first_view,
        first_bind_group,
        _second: second,
        second_view,
        second_bind_group,
    }
}

fn render_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &'static str,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
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
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}
