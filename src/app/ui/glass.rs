use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const BLUR_DOWNSAMPLE: u32 = 4;
const BLUR_SIGMA_POINTS: f32 = 12.0;
const MAX_GLASS_REGIONS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassSurface {
    Color,
    Layers,
    Toolbar,
}

#[derive(Clone, Copy)]
pub(super) struct GlassMarker(pub(super) GlassSurface);

#[derive(Clone, Copy)]
pub(super) struct GlassRegion {
    surface: GlassSurface,
    rect: egui::Rect,
    corner_radius: f32,
    tint: [f32; 4],
}

impl GlassRegion {
    pub(super) fn new(
        surface: GlassSurface,
        rect: egui::Rect,
        corner_radius: f32,
        dark_mode: bool,
    ) -> Self {
        let tint = if dark_mode {
            egui::Rgba::from_srgba_unmultiplied(17, 19, 24, 0)
        } else {
            egui::Rgba::from_srgba_unmultiplied(248, 250, 252, 0)
        };
        Self {
            surface,
            rect,
            corner_radius,
            tint: tint.to_array(),
        }
    }

    pub(super) fn surface(self) -> GlassSurface {
        self.surface
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        self.rect.intersects(other.rect)
    }

    fn to_raw(self, pixels_per_point: f32, surface_size: [u32; 2]) -> Option<GlassRegionRaw> {
        let scale = pixels_per_point.max(f32::EPSILON);
        let max_x = surface_size[0] as f32;
        let max_y = surface_size[1] as f32;
        let min_x = (self.rect.min.x * scale).clamp(0.0, max_x);
        let min_y = (self.rect.min.y * scale).clamp(0.0, max_y);
        let max_x = (self.rect.max.x * scale).clamp(0.0, max_x);
        let max_y = (self.rect.max.y * scale).clamp(0.0, max_y);
        if max_x <= min_x || max_y <= min_y {
            return None;
        }
        Some(GlassRegionRaw {
            rect: [min_x, min_y, max_x, max_y],
            radius: self.corner_radius * scale,
            padding: [0.0; 3],
            tint: self.tint,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlassRegionRaw {
    rect: [f32; 4],
    radius: f32,
    padding: [f32; 3],
    tint: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniform {
    direction: [f32; 2],
    scale: f32,
    padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceUniform {
    size: [f32; 2],
    padding: [f32; 2],
}

pub(super) struct GlassRenderer {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    _scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    _downsample_texture: wgpu::Texture,
    downsample_view: wgpu::TextureView,
    _horizontal_texture: wgpu::Texture,
    horizontal_view: wgpu::TextureView,
    _vertical_texture: wgpu::Texture,
    vertical_view: wgpu::TextureView,
    horizontal_uniform: wgpu::Buffer,
    vertical_uniform: wgpu::Buffer,
    _surface_uniform: wgpu::Buffer,
    region_buffer: wgpu::Buffer,
    horizontal_bind_group: wgpu::BindGroup,
    vertical_bind_group: wgpu::BindGroup,
    scene_bind_group: wgpu::BindGroup,
    glass_bind_group: wgpu::BindGroup,
    blur_pipeline: wgpu::RenderPipeline,
    scene_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
}

impl GlassRenderer {
    pub(super) fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass blur sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glass blur bind group layout"),
                entries: &[
                    sampler_layout_entry(0),
                    texture_layout_entry(1),
                    uniform_layout_entry(2),
                ],
            });
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glass composite bind group layout"),
                entries: &[
                    sampler_layout_entry(0),
                    texture_layout_entry(1),
                    uniform_layout_entry(2),
                ],
            });
        let horizontal_uniform = create_uniform_buffer(
            device,
            "horizontal glass blur uniform",
            &BlurUniform {
                direction: [1.0, 0.0],
                scale: BLUR_SIGMA_POINTS / BLUR_DOWNSAMPLE as f32,
                padding: 0.0,
            },
        );
        let vertical_uniform = create_uniform_buffer(
            device,
            "vertical glass blur uniform",
            &BlurUniform {
                direction: [0.0, 1.0],
                scale: BLUR_SIGMA_POINTS / BLUR_DOWNSAMPLE as f32,
                padding: 0.0,
            },
        );
        let surface_uniform = create_uniform_buffer(
            device,
            "glass surface uniform",
            &SurfaceUniform {
                size: [size[0] as f32, size[1] as f32],
                padding: [0.0; 2],
            },
        );
        let region_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass region vertex buffer"),
            size: (MAX_GLASS_REGIONS * std::mem::size_of::<GlassRegionRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass blur shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../renderer/shaders/glass_blur.wgsl").into(),
            ),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../renderer/shaders/glass.wgsl").into(),
            ),
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass blur pipeline layout"),
            bind_group_layouts: &[Some(&blur_bind_group_layout)],
            immediate_size: 0,
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glass composite pipeline layout"),
                bind_group_layouts: &[Some(&composite_bind_group_layout)],
                immediate_size: 0,
            });
        let blur_pipeline = create_pipeline(
            device,
            "glass blur pipeline",
            &blur_pipeline_layout,
            &blur_shader,
            GlassPipelineDescriptor {
                vertex_entry: "vs_fullscreen",
                fragment_entry: "fs_blur",
                format,
                blend: None,
                buffers: &[],
            },
        );
        let scene_pipeline = create_pipeline(
            device,
            "glass scene pipeline",
            &composite_pipeline_layout,
            &composite_shader,
            GlassPipelineDescriptor {
                vertex_entry: "vs_fullscreen",
                fragment_entry: "fs_scene",
                format,
                blend: None,
                buffers: &[],
            },
        );
        let glass_pipeline = create_pipeline(
            device,
            "glass region pipeline",
            &composite_pipeline_layout,
            &composite_shader,
            GlassPipelineDescriptor {
                vertex_entry: "vs_glass",
                fragment_entry: "fs_glass",
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlassRegionRaw>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 16,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 2,
                        },
                    ],
                }],
            },
        );
        let (
            scene_texture,
            scene_view,
            downsample_texture,
            downsample_view,
            horizontal_texture,
            horizontal_view,
            vertical_texture,
            vertical_view,
        ) = create_textures(device, size, format);
        let horizontal_bind_group = create_blur_bind_group(
            device,
            &blur_bind_group_layout,
            &sampler,
            &downsample_view,
            &horizontal_uniform,
            "horizontal glass blur bind group",
        );
        let vertical_bind_group = create_blur_bind_group(
            device,
            &blur_bind_group_layout,
            &sampler,
            &horizontal_view,
            &vertical_uniform,
            "vertical glass blur bind group",
        );
        let scene_bind_group = create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &sampler,
            &scene_view,
            &surface_uniform,
            "glass scene bind group",
        );
        let glass_bind_group = create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &sampler,
            &vertical_view,
            &surface_uniform,
            "glass region bind group",
        );
        Self {
            size,
            format,
            _scene_texture: scene_texture,
            scene_view,
            _downsample_texture: downsample_texture,
            downsample_view,
            _horizontal_texture: horizontal_texture,
            horizontal_view,
            _vertical_texture: vertical_texture,
            vertical_view,
            horizontal_uniform,
            vertical_uniform,
            _surface_uniform: surface_uniform,
            region_buffer,
            horizontal_bind_group,
            vertical_bind_group,
            scene_bind_group,
            glass_bind_group,
            blur_pipeline,
            scene_pipeline,
            glass_pipeline,
        }
    }

    pub(super) fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_view
    }

    pub(super) fn resize(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) {
        if self.size == size && self.format == format {
            return;
        }
        *self = Self::new(device, size, format);
    }

    pub(super) fn prepare_regions(
        &mut self,
        queue: &wgpu::Queue,
        regions: &[GlassRegion],
        pixels_per_point: f32,
    ) {
        let raw_regions: Vec<_> = regions
            .iter()
            .take(MAX_GLASS_REGIONS)
            .map(|region| {
                region
                    .to_raw(pixels_per_point, self.size)
                    .unwrap_or_else(GlassRegionRaw::zeroed)
            })
            .collect();
        if raw_regions.is_empty() {
            return;
        }
        queue.write_buffer(&self.region_buffer, 0, bytemuck::cast_slice(&raw_regions));
        queue.write_buffer(
            &self.horizontal_uniform,
            0,
            bytemuck::bytes_of(&BlurUniform {
                direction: [1.0, 0.0],
                scale: BLUR_SIGMA_POINTS * pixels_per_point / BLUR_DOWNSAMPLE as f32,
                padding: 0.0,
            }),
        );
        queue.write_buffer(
            &self.vertical_uniform,
            0,
            bytemuck::bytes_of(&BlurUniform {
                direction: [0.0, 1.0],
                scale: BLUR_SIGMA_POINTS * pixels_per_point / BLUR_DOWNSAMPLE as f32,
                padding: 0.0,
            }),
        );
    }

    pub(super) fn prepare_blur(&self, encoder: &mut wgpu::CommandEncoder) {
        self.render_scene_pass(
            encoder,
            "glass scene downsample pass",
            &self.downsample_view,
        );
        self.render_blur_pass(
            encoder,
            "horizontal glass blur pass",
            &self.horizontal_view,
            &self.horizontal_bind_group,
        );
        self.render_blur_pass(
            encoder,
            "vertical glass blur pass",
            &self.vertical_view,
            &self.vertical_bind_group,
        );
    }

    pub(super) fn apply_region(&self, encoder: &mut wgpu::CommandEncoder, region_index: usize) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glass region composite pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.scene_view,
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
        pass.set_bind_group(0, &self.glass_bind_group, &[]);
        pass.set_pipeline(&self.glass_pipeline);
        let stride = std::mem::size_of::<GlassRegionRaw>() as u64;
        let start = region_index as u64 * stride;
        pass.set_vertex_buffer(0, self.region_buffer.slice(start..start + stride));
        pass.draw(0..6, 0..1);
    }

    pub(super) fn blit_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        self.render_scene_pass(encoder, "glass scene blit pass", target);
    }

    fn render_scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &'static str,
        target: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.scene_bind_group, &[]);
        pass.set_pipeline(&self.scene_pipeline);
        pass.draw(0..3, 0..1);
    }

    fn render_blur_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &'static str,
        target: &wgpu::TextureView,
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
        pass.set_pipeline(&self.blur_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn create_textures(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> (
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Texture,
    wgpu::TextureView,
) {
    let (scene_texture, scene_view) = create_texture(device, "glass scene texture", size, format);
    let blur_size = [
        size[0].div_ceil(BLUR_DOWNSAMPLE).max(1),
        size[1].div_ceil(BLUR_DOWNSAMPLE).max(1),
    ];
    let (downsample_texture, downsample_view) =
        create_texture(device, "glass downsample texture", blur_size, format);
    let (horizontal_texture, horizontal_view) =
        create_texture(device, "horizontal glass blur texture", blur_size, format);
    let (vertical_texture, vertical_view) =
        create_texture(device, "vertical glass blur texture", blur_size, format);
    (
        scene_texture,
        scene_view,
        downsample_texture,
        downsample_view,
        horizontal_texture,
        horizontal_view,
        vertical_texture,
        vertical_view,
    )
}

fn create_texture(
    device: &wgpu::Device,
    label: &'static str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[format],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_blur_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(texture),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

fn create_composite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: &wgpu::TextureView,
    surface_uniform: &wgpu::Buffer,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(texture),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: surface_uniform.as_entire_binding(),
            },
        ],
    })
}

fn create_uniform_buffer<T: Pod>(
    device: &wgpu::Device,
    label: &'static str,
    value: &T,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

struct GlassPipelineDescriptor<'a> {
    vertex_entry: &'static str,
    fragment_entry: &'static str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
}

fn create_pipeline(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    descriptor: GlassPipelineDescriptor<'_>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(descriptor.vertex_entry),
            compilation_options: Default::default(),
            buffers: descriptor.buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(descriptor.fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: descriptor.format,
                blend: descriptor.blend,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_region_scales_and_clips_to_surface() {
        let raw = GlassRegion::new(
            GlassSurface::Toolbar,
            egui::Rect::from_min_max(egui::pos2(-5.0, 4.0), egui::pos2(60.0, 80.0)),
            16.0,
            true,
        )
        .to_raw(2.0, [100, 120])
        .expect("partially visible region should remain");

        assert_eq!(raw.rect, [0.0, 8.0, 100.0, 120.0]);
        assert_eq!(raw.radius, 32.0);
    }

    #[test]
    fn glass_region_outside_surface_is_discarded() {
        let region = GlassRegion::new(
            GlassSurface::Toolbar,
            egui::Rect::from_min_max(egui::pos2(60.0, 70.0), egui::pos2(80.0, 90.0)),
            16.0,
            false,
        );
        assert!(region.to_raw(2.0, [100, 120]).is_none());
    }

    #[test]
    fn overlapping_glass_regions_are_detected() {
        let region = |min, max| {
            GlassRegion::new(
                GlassSurface::Toolbar,
                egui::Rect::from_min_max(min, max),
                16.0,
                false,
            )
        };
        let first = region(egui::pos2(0.0, 0.0), egui::pos2(20.0, 20.0));

        assert!(first.overlaps(region(egui::pos2(10.0, 10.0), egui::pos2(30.0, 30.0))));
        assert!(!first.overlaps(region(egui::pos2(21.0, 21.0), egui::pos2(30.0, 30.0))));
    }
}
