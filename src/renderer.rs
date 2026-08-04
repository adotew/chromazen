use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use winit::{dpi::PhysicalSize, window::Window};

mod history;
mod layers;
mod persistence;
mod resources;
mod sampling;
mod stamps;
mod view;

use self::{
    history::{HistoryTarget, PaintHistory, StructureEffect, TextureRect},
    layers::{
        LayerProperties, PaintLayer, insertion_index, layer_name, normalized_layer_name,
        relative_insertion_index, replacement_index_after_delete,
    },
    resources::RenderResources,
    sampling::{document_pixel, read_composited_color},
    stamps::{MAX_STAMPS_PER_FRAME, StampQueue, StampRaw},
    view::PaintView,
};
pub(crate) use self::{
    layers::{
        DropEdge, LayerId, LayerInfo, LayerResourceId, LayerSnapshot, merge_down_target_index,
    },
    persistence::LayerReadback,
    view::PaintViewSnapshot,
};
use crate::{
    artwork::{DOCUMENT_SCHEMA_VERSION, DocumentManifest, LayerManifest, clipping_base_index},
    config::LoadedBrushPreset,
    gpu::GpuContext,
    paint::{BrushSpacing, PaintTool, StrokePoint},
};

pub(crate) const DEFAULT_CANVAS_SIZE: [u32; 2] = [4000, 4000];
const MAX_CANVAS_DIMENSION: u32 = 8192;
// Caps the baseline layer, history, smudge, and mask allocations to a safe working set.
const MAX_CANVAS_PIXELS: u64 = 32 * 1024 * 1024;
const DOCUMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const STROKE_MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const LAYER_PREVIEW_SIZE: u32 = 128;
// Eight simulation steps per brush radius retain tip detail without dense-pass overhead.
const SMUDGE_MIN_STEP_RATIO: f32 = 0.125;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanvasSizeConstraints {
    pub(crate) max_dimension: u32,
    pub(crate) max_pixels: u64,
}

impl CanvasSizeConstraints {
    pub(crate) fn validate(self, size: [u32; 2]) -> Result<(), String> {
        let [width, height] = size;
        if width == 0 || height == 0 {
            return Err("canvas width and height must be at least 1 pixel".to_owned());
        }
        if width > self.max_dimension || height > self.max_dimension {
            return Err(format!(
                "canvas width and height cannot exceed {} pixels",
                self.max_dimension
            ));
        }
        if u64::from(width) * u64::from(height) > self.max_pixels {
            return Err(format!(
                "canvas area cannot exceed {} megapixels",
                self.max_pixels / 1_000_000
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PaintUniform {
    dims: [f32; 2],
    padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerPreviewUniform {
    preview_dims: [f32; 2],
    document_dims: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct ViewUniform {
    document_from_window_x: [f32; 4],
    document_from_window_y: [f32; 4],
    paint_dims: [f32; 2],
    padding: [f32; 2],
    background_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StrokeUniform {
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerSettingsUniform {
    opacity: f32,
    padding: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CursorRaw {
    center: [f32; 2],
    half_size: [f32; 2],
    axis_x: [f32; 2],
    axis_y: [f32; 2],
    surface_size: [f32; 2],
    padding: [f32; 2],
}

#[derive(Clone, Copy)]
pub(crate) struct BrushCursor {
    pub(crate) center: [f32; 2],
    pub(crate) diameter: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentVersions {
    pub(crate) generation: u64,
    pub(crate) metadata: u64,
    pub(crate) layers: Vec<(LayerId, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrokeRenderPath {
    Mask,
    DirectSmudge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ActiveStroke {
    layer_id: LayerId,
    tool: PaintTool,
    color: [f32; 4],
}

impl ActiveStroke {
    fn new(layer_id: LayerId, tool: PaintTool, color: [f32; 4]) -> Self {
        Self {
            layer_id,
            tool,
            color: premultiply(color),
        }
    }

    fn render_path(self) -> StrokeRenderPath {
        match self.tool {
            PaintTool::Brush | PaintTool::Eraser => StrokeRenderPath::Mask,
            PaintTool::Smudge => StrokeRenderPath::DirectSmudge,
        }
    }
}

pub struct PaintRenderer {
    gpu: GpuContext,
    document_size: [u32; 2],
    resources: RenderResources,
    layers: Vec<PaintLayer>,
    selection: LayerId,
    background_color: [f32; 4],
    next_layer_id: u64,
    next_layer_number: u64,
    next_layer_resource_id: u64,
    stamp_queue: StampQueue,
    active_stroke: Option<ActiveStroke>,
    history: PaintHistory,
    view: PaintView,
    document_generation: u64,
    metadata_version: u64,
    layer_versions: HashMap<LayerId, u64>,
    clipped_layer_bind_groups: HashMap<LayerId, wgpu::BindGroup>,
    clipping_bind_groups_dirty: bool,
    last_view_uniform: Option<ViewUniform>,
}

impl PaintRenderer {
    pub async fn new(
        window: Arc<Window>,
        brush_preset: &LoadedBrushPreset,
    ) -> Result<Self, String> {
        let gpu = GpuContext::new(window).await?;
        let device = gpu.device();
        let queue = gpu.queue();
        let surface_format = gpu.surface_format();

        let document_size = DEFAULT_CANVAS_SIZE;
        let resources = RenderResources::new(
            device,
            queue,
            document_size,
            surface_format,
            brush_preset.stamp_image.as_ref(),
        )?;

        let first_layer = resources.create_paint_layer(
            device,
            document_size,
            LayerId(1),
            LayerResourceId(1),
            LayerProperties::new("Layer 1".to_owned()),
        );
        let stamp_aspect = brush_preset
            .stamp_image
            .as_ref()
            .map_or(1.0, |image| image.width() as f32 / image.height() as f32);
        let history = PaintHistory::new(device, document_size);
        let mut renderer = Self {
            gpu,
            document_size,
            resources,
            layers: vec![first_layer],
            selection: LayerId(1),
            background_color: [1.0; 4],
            next_layer_id: 2,
            next_layer_number: 2,
            next_layer_resource_id: 2,
            stamp_queue: StampQueue::new(stamp_aspect),
            active_stroke: None,
            history,
            view: PaintView::default(),
            document_generation: 0,
            metadata_version: 0,
            layer_versions: HashMap::from([(LayerId(1), 0)]),
            clipped_layer_bind_groups: HashMap::new(),
            clipping_bind_groups_dirty: true,
            last_view_uniform: None,
        };
        renderer.fit_to_screen();
        renderer.clear_canvas();
        renderer.reset_change_tracking(false);
        Ok(renderer)
    }

    pub fn device(&self) -> &wgpu::Device {
        self.gpu.device()
    }
    pub fn queue(&self) -> &wgpu::Queue {
        self.gpu.queue()
    }
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.gpu.surface_format()
    }
    pub fn surface_size(&self) -> [u32; 2] {
        self.gpu.surface_size()
    }
    pub(crate) fn document_size(&self) -> [u32; 2] {
        self.document_size
    }
    pub fn zoom(&self) -> f32 {
        self.view.zoom()
    }
    pub(crate) fn view_snapshot(&self) -> PaintViewSnapshot {
        self.view.snapshot()
    }
    pub fn brush_outline_half_size(&self, diameter: f32) -> [f32; 2] {
        let half_size = self.stamp_queue.half_size(diameter * 0.5);
        let zoom = self.view.zoom();
        [
            (half_size[0] * zoom).max(0.5),
            (half_size[1] * zoom).max(0.5),
        ]
    }
    pub fn has_pending_stamps(&self) -> bool {
        self.stamp_queue.has_pending()
    }

    pub(crate) fn canvas_size_constraints(&self) -> CanvasSizeConstraints {
        CanvasSizeConstraints {
            max_dimension: self
                .gpu
                .device()
                .limits()
                .max_texture_dimension_2d
                .min(MAX_CANVAS_DIMENSION),
            max_pixels: MAX_CANVAS_PIXELS,
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.view.set_surface_size(self.gpu.surface_size());
    }

    pub fn try_set_brush_preset(&mut self, preset: &LoadedBrushPreset) -> Result<bool, String> {
        if self.active_stroke.is_some() || self.stamp_queue.has_pending() {
            return Ok(false);
        }
        self.resources.replace_brush_stamp(
            self.gpu.device(),
            self.gpu.queue(),
            preset.stamp_image.as_ref(),
        )?;
        let stamp_aspect = preset
            .stamp_image
            .as_ref()
            .map_or(1.0, |image| image.width() as f32 / image.height() as f32);
        self.stamp_queue.set_stamp_aspect(stamp_aspect);
        Ok(true)
    }

    pub fn fit_to_screen(&mut self) {
        self.view
            .fit_to_screen(self.surface_size(), self.document_size);
    }

    pub(crate) fn prepare_canvas_crop_view(&mut self) {
        self.fit_to_screen();
        let [width, height] = self.surface_size();
        self.view
            .apply_zoom_at(0.8, [width as f32 * 0.5, height as f32 * 0.5]);
    }

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        self.view.apply_zoom_at(factor, cursor);
    }

    pub(crate) fn canvas_rotation(&self) -> f32 {
        self.view.snapshot().rotation()
    }

    pub(crate) fn canvas_center_in_window(&self) -> [f32; 2] {
        self.view
            .snapshot()
            .document_to_window(self.document_center())
    }

    pub(crate) fn set_canvas_rotation(&mut self, radians: f32) -> bool {
        let center = self.document_center();
        self.view.set_rotation_around(radians, center)
    }

    pub(crate) fn rotate_canvas_view(&mut self, radians: f32) -> bool {
        let center = self.document_center();
        self.view.rotate_by_around(radians, center)
    }

    pub(crate) fn reset_canvas_rotation(&mut self) -> bool {
        let center = self.document_center();
        self.view.reset_rotation_around(center)
    }

    pub(crate) fn toggle_canvas_flip_horizontal(&mut self) {
        let center = self.document_center();
        self.view.toggle_flip_horizontal_around(center);
    }

    pub(crate) fn toggle_canvas_flip_vertical(&mut self) {
        let center = self.document_center();
        self.view.toggle_flip_vertical_around(center);
    }

    fn document_center(&self) -> [f32; 2] {
        [
            self.document_size[0] as f32 * 0.5,
            self.document_size[1] as f32 * 0.5,
        ]
    }

    pub fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.view.pan_by_window_delta(delta);
    }

    pub fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.window_to_document(point)
    }

    pub(crate) fn window_to_workspace(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.snapshot().window_to_workspace(point)
    }

    pub fn sample_composited_color(&self, window_point: [f32; 2]) -> Option<[u8; 3]> {
        if self.active_stroke.is_some() || self.stamp_queue.has_pending() {
            return None;
        }
        let pixel = document_pixel(
            self.view.window_to_document(window_point),
            self.document_size,
        )?;
        read_composited_color(
            self.gpu.device(),
            self.gpu.queue(),
            &self.layers,
            pixel,
            self.background_color,
        )
    }

    pub fn can_paint(&self) -> bool {
        self.active_stroke.is_none()
            && self
                .selected_layer_index()
                .is_some_and(|index| self.layers[index].visible)
    }

    pub(crate) fn document_versions(&self) -> DocumentVersions {
        let mut layers: Vec<_> = self
            .layers
            .iter()
            .map(|layer| {
                (
                    layer.id,
                    self.layer_versions.get(&layer.id).copied().unwrap_or(0),
                )
            })
            .collect();
        layers.sort_by_key(|(id, _)| id.0);
        DocumentVersions {
            generation: self.document_generation,
            metadata: self.metadata_version,
            layers,
        }
    }

    pub(crate) fn can_replace_document(&self) -> bool {
        self.active_stroke.is_none()
            && !self.history.stroke_active()
            && !self.stamp_queue.has_pending()
    }

    pub(crate) fn document_manifest(&self) -> DocumentManifest {
        DocumentManifest {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            width: self.document_size[0],
            height: self.document_size[1],
            background: rgb8(self.background_color),
            brush_color: [170, 187, 204, 255],
            selected_layer: self.selection.0,
            layers: self
                .layers
                .iter()
                .map(|layer| LayerManifest {
                    id: layer.id.0,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    clipped: layer.clipped,
                    file: format!("layers/{}.png", layer.id.0),
                })
                .collect(),
            references: Vec::new(),
        }
    }

    pub(crate) fn begin_document_layer_readback(&self) -> Result<LayerReadback, String> {
        if !self.can_replace_document() {
            return Err("the current document is busy".to_owned());
        }
        Ok(persistence::begin_read_layers(
            self.gpu.device(),
            self.gpu.queue(),
            &self.layers,
            self.document_size,
        ))
    }

    pub(crate) fn reset_document(&mut self, size: [u32; 2]) -> Result<(), String> {
        if !self.can_replace_document() {
            return Err("the current document is busy".to_owned());
        }
        self.canvas_size_constraints().validate(size)?;
        self.resize_document_resources(size);

        let id = LayerId(1);
        let resource_id = self.allocate_layer_resource_id();
        let layer = self.resources.create_paint_layer(
            self.gpu.device(),
            self.document_size,
            id,
            resource_id,
            LayerProperties::new("Layer 1".to_owned()),
        );
        clear_layer(self.gpu.device(), self.gpu.queue(), &layer.view);
        self.layers = vec![layer];
        self.selection = id;
        self.background_color = [1.0; 4];
        self.next_layer_id = 2;
        self.next_layer_number = 2;
        self.stamp_queue.clear();
        self.history.clear();
        self.clipping_bind_groups_dirty = true;
        self.view.reset_orientation();
        self.fit_to_screen();
        self.reset_change_tracking(true);
        Ok(())
    }

    pub(crate) fn resize_canvas(
        &mut self,
        size: [u32; 2],
        origin: [i32; 2],
    ) -> Result<bool, String> {
        if !self.can_replace_document() {
            return Err("the current document is busy".to_owned());
        }
        self.canvas_size_constraints().validate(size)?;
        if size == self.document_size && origin == [0, 0] {
            return Ok(false);
        }

        let before_size = self.document_size;
        let copy = canvas_copy_for_resize(before_size, size, origin);
        let mut replacement_layers = Vec::with_capacity(self.layers.len());
        for index in 0..self.layers.len() {
            let resource_id = self.allocate_layer_resource_id();
            let layer = &self.layers[index];
            replacement_layers.push(self.resources.create_paint_layer(
                self.gpu.device(),
                size,
                layer.id,
                resource_id,
                LayerProperties {
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    clipped: layer.clipped,
                },
            ));
        }

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("canvas resize encoder"),
                });
        for (source, destination) in self.layers.iter().zip(&replacement_layers) {
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("canvas resize clear pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &destination.view,
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
            if let Some(copy) = copy {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &source.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: copy.source.x,
                            y: copy.source.y,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &destination.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: copy.destination[0],
                            y: copy.destination[1],
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: copy.source.width,
                        height: copy.source.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        self.gpu.queue().submit(std::iter::once(encoder.finish()));

        let previous_layers = std::mem::replace(&mut self.layers, replacement_layers);
        let retained_bytes = layer_set_byte_len(before_size, previous_layers.len())
            .max(layer_set_byte_len(size, self.layers.len()));
        self.resources
            .resize_document(self.gpu.device(), self.gpu.queue(), size);
        self.history.resize_mirror(self.gpu.device(), size);
        self.document_size = size;
        self.history
            .record_canvas_resize(before_size, size, previous_layers, retained_bytes);
        self.clipping_bind_groups_dirty = true;
        self.fit_to_screen();
        self.mark_entire_document_changed();
        Ok(true)
    }

    pub(crate) fn load_document(
        &mut self,
        document: &DocumentManifest,
        pixels: Vec<image::RgbaImage>,
    ) -> Result<(), String> {
        if !self.can_replace_document() {
            return Err("the current document is busy".to_owned());
        }
        document.validate()?;
        let document_size = [document.width, document.height];
        self.canvas_size_constraints().validate(document_size)?;
        if pixels.len() != document.layers.len() {
            return Err("loaded layer count does not match document metadata".to_owned());
        }

        for (metadata, image) in document.layers.iter().zip(&pixels) {
            if image.dimensions() != (document.width, document.height) {
                return Err(format!(
                    "layer {} has dimensions {}x{}; expected {}x{}",
                    metadata.id,
                    image.width(),
                    image.height(),
                    document.width,
                    document.height
                ));
            }
        }

        self.resize_document_resources(document_size);
        let mut layers = Vec::with_capacity(document.layers.len());
        for (metadata, image) in document.layers.iter().zip(pixels) {
            let resource_id = self.allocate_layer_resource_id();
            let layer = self.resources.create_paint_layer(
                self.gpu.device(),
                self.document_size,
                LayerId(metadata.id),
                resource_id,
                LayerProperties {
                    name: metadata.name.clone(),
                    visible: metadata.visible,
                    opacity: metadata.opacity,
                    clipped: metadata.clipped,
                },
            );
            self.gpu.queue().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &layer.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                image.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(document.width * 4),
                    rows_per_image: Some(document.height),
                },
                wgpu::Extent3d {
                    width: document.width,
                    height: document.height,
                    depth_or_array_layers: 1,
                },
            );
            layers.push(layer);
        }

        self.layers = layers;
        self.selection = LayerId(document.selected_layer);
        self.background_color = opaque_color(document.background);
        self.next_layer_id = document
            .layers
            .iter()
            .map(|layer| layer.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.next_layer_number = next_layer_number(&document.layers);
        self.stamp_queue.clear();
        self.history.clear();
        self.clipping_bind_groups_dirty = true;
        self.view.reset_orientation();
        self.fit_to_screen();
        self.reset_change_tracking(false);
        Ok(())
    }

    pub(crate) fn layer_snapshot(&self) -> LayerSnapshot {
        LayerSnapshot {
            layers: self
                .layers
                .iter()
                .map(|layer| LayerInfo {
                    id: layer.id,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    clipped: layer.clipped,
                })
                .collect(),
            selection: self.selection,
            background_color: self.background_color,
        }
    }

    pub(crate) fn layer_preview_views(
        &self,
    ) -> impl Iterator<Item = (LayerId, LayerResourceId, &wgpu::TextureView)> {
        self.layers
            .iter()
            .map(|layer| (layer.id, layer.resource_id, &layer.preview_view))
    }

    pub(crate) fn select_layer(&mut self, id: LayerId) -> bool {
        if self.active_stroke.is_some() || self.selection == id {
            return false;
        }
        if self.layers.iter().any(|layer| layer.id == id) {
            self.selection = id;
            self.mark_metadata_changed();
            true
        } else {
            false
        }
    }

    pub(crate) fn rename_layer(&mut self, id: LayerId, name: &str) -> bool {
        if !self.can_replace_document() {
            return false;
        }
        let Some(name) = normalized_layer_name(name) else {
            return false;
        };
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.name == name {
            return false;
        }
        let before = std::mem::replace(&mut layer.name, name.clone());
        self.history.record_rename_layer(id, before, name);
        self.mark_metadata_changed();
        true
    }

    pub(crate) fn set_layer_clipped(&mut self, id: LayerId, clipped: bool) -> bool {
        if !self.can_replace_document() {
            return false;
        }
        let Some(index) = self.layers.iter().position(|layer| layer.id == id) else {
            return false;
        };
        if clipped && index == 0 || self.layers[index].clipped == clipped {
            return false;
        }
        let before = std::mem::replace(&mut self.layers[index].clipped, clipped);
        self.history.record_layer_clipping(id, before, clipped);
        self.mark_metadata_changed();
        true
    }

    pub(crate) fn set_layer_visibility(&mut self, id: LayerId, visible: bool) -> bool {
        if !self.can_replace_document() {
            return false;
        }
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.visible == visible {
            return false;
        }
        let before = std::mem::replace(&mut layer.visible, visible);
        self.history.record_layer_visibility(id, before, visible);
        self.mark_metadata_changed();
        true
    }

    pub(crate) fn set_layer_opacity(&mut self, id: LayerId, opacity: u8) -> bool {
        if !self.can_replace_document() || opacity > 100 {
            return false;
        }
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.opacity == opacity {
            return false;
        }
        layer.opacity = opacity;
        self.gpu.queue().write_buffer(
            &layer.settings_buffer,
            0,
            bytemuck::bytes_of(&LayerSettingsUniform {
                opacity: f32::from(opacity) / 100.0,
                padding: [0.0; 3],
            }),
        );
        self.mark_metadata_changed();
        true
    }

    pub(crate) fn commit_layer_opacity(&mut self, id: LayerId, before: u8, after: u8) -> bool {
        if !self.can_replace_document() || before == after || after > 100 {
            return false;
        }
        let Some(layer) = self.layers.iter().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.opacity != after {
            return false;
        }
        self.history.record_layer_opacity(id, before, after);
        true
    }

    pub(crate) fn move_layer_relative(
        &mut self,
        dragged: LayerId,
        target: LayerId,
        edge: DropEdge,
    ) -> bool {
        if !self.can_replace_document() {
            return false;
        }
        let Some(dragged_index) = self.layers.iter().position(|layer| layer.id == dragged) else {
            return false;
        };
        let Some(target_index) = self.layers.iter().position(|layer| layer.id == target) else {
            return false;
        };
        let Some(insertion) = relative_insertion_index(dragged_index, target_index, edge) else {
            return false;
        };
        let mut reordered_clipping: Vec<_> =
            self.layers.iter().map(|layer| layer.clipped).collect();
        let dragged_clipping = reordered_clipping.remove(dragged_index);
        reordered_clipping.insert(insertion, dragged_clipping);
        if reordered_clipping[0] {
            return false;
        }
        let layer = self.layers.remove(dragged_index);
        self.layers.insert(insertion, layer);
        self.history
            .record_move_layer(dragged, dragged_index, insertion);
        self.mark_metadata_changed();
        true
    }

    pub(crate) fn set_background_color(&mut self, color: [u8; 3]) {
        let color = opaque_color(color);
        if self.active_stroke.is_none() && self.background_color != color {
            self.background_color = color;
            self.mark_metadata_changed();
        }
    }

    pub(crate) fn commit_background_color(&mut self, before: [u8; 3], after: [u8; 3]) {
        if self.active_stroke.is_some() {
            return;
        }
        let before = opaque_color(before);
        let after = opaque_color(after);
        if self.background_color != after {
            self.background_color = after;
            self.mark_metadata_changed();
        }
        self.history.record_background_color(before, after);
    }

    pub(crate) fn add_layer(&mut self) -> bool {
        if self.active_stroke.is_some()
            || self.history.stroke_active()
            || self.stamp_queue.has_pending()
        {
            return false;
        }
        let selection_before = self.selection;
        let index = insertion_index(self.selected_layer_index(), self.layers.len());
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;
        let name = layer_name(self.next_layer_number);
        self.next_layer_number += 1;
        let resource_id = self.allocate_layer_resource_id();
        let layer = self.resources.create_paint_layer(
            self.gpu.device(),
            self.document_size,
            id,
            resource_id,
            LayerProperties::new(name),
        );
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("new layer clear encoder"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("new layer clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.view,
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
        self.layers.insert(index, layer);
        self.selection = id;
        self.history
            .record_add(id, index, selection_before, self.layer_resource_byte_len());
        self.mark_metadata_changed();
        self.layer_versions.insert(id, self.document_generation);
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
        true
    }

    pub(crate) fn can_merge_layer_down(&self, id: LayerId) -> bool {
        self.can_replace_document()
            && self
                .layers
                .iter()
                .position(|layer| layer.id == id)
                .and_then(merge_down_target_index)
                .is_some_and(|lower_index| {
                    let upper_index = lower_index + 1;
                    self.layers[upper_index].visible
                        && self.layers[lower_index].visible
                        && !self.layers[lower_index].clipped
                        && !self
                            .layers
                            .get(upper_index + 1)
                            .is_some_and(|layer| layer.clipped)
                })
    }

    pub(crate) fn merge_layer_down(&mut self, id: LayerId) -> bool {
        if !self.can_merge_layer_down(id) {
            return false;
        }
        let upper_index = self
            .layers
            .iter()
            .position(|layer| layer.id == id)
            .expect("mergeable layer must exist");
        let lower_index =
            merge_down_target_index(upper_index).expect("mergeable layer must have a layer below");
        let lower_id = self.layers[lower_index].id;
        let lower_name = self.layers[lower_index].name.clone();
        let selection_before = self.selection;
        let resource_id = self.allocate_layer_resource_id();
        let merged = self.resources.create_paint_layer(
            self.gpu.device(),
            self.document_size,
            lower_id,
            resource_id,
            LayerProperties::new(lower_name),
        );

        let upper = self.layers.remove(upper_index);
        let lower = self.layers.remove(lower_index);
        let clipped_bind_group = upper.clipped.then(|| {
            self.resources
                .create_clipped_layer_bind_group(self.gpu.device(), &upper, &lower)
        });
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("layer merge encoder"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer merge pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &merged.view,
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
            pass.set_pipeline(&self.resources.merge_pipeline);
            pass.set_bind_group(0, &lower.blit_bind_group, &[]);
            pass.draw(0..3, 0..1);
            if let Some(bind_group) = &clipped_bind_group {
                pass.set_pipeline(&self.resources.clipped_layer_merge_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
            } else {
                pass.set_bind_group(0, &upper.blit_bind_group, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue().submit(std::iter::once(encoder.finish()));

        self.layers.insert(lower_index, merged);
        self.selection = lower_id;
        self.history.record_merge_down(
            upper,
            lower,
            lower_index,
            selection_before,
            self.layer_resource_byte_len(),
        );
        self.layer_versions.remove(&id);
        let version = self.next_document_generation();
        self.layer_versions.insert(lower_id, version);
        self.metadata_version = version;
        true
    }

    pub(crate) fn can_delete_selected_layer(&self) -> bool {
        self.layers.len() > 1
            && self
                .selected_layer_index()
                .is_some_and(|index| index != 0 || !self.layers[1].clipped)
            && self.active_stroke.is_none()
            && !self.history.stroke_active()
            && !self.stamp_queue.has_pending()
    }

    pub(crate) fn delete_selected_layer(&mut self) -> bool {
        if !self.can_delete_selected_layer() {
            return false;
        }
        let selection_before = self.selection;
        let index = self
            .selected_layer_index()
            .expect("selected layer must exist");
        let replacement_index = replacement_index_after_delete(self.layers.len(), index)
            .expect("deletion requires another paint layer");
        let next_id = self.layers[replacement_index].id;
        let layer = self.layers.remove(index);
        self.selection = next_id;
        self.history.record_delete(
            layer,
            index,
            selection_before,
            self.selection,
            self.layer_resource_byte_len(),
        );
        self.layer_versions.remove(&selection_before);
        self.mark_metadata_changed();
        true
    }

    pub fn begin_stroke(&mut self, tool: PaintTool, origin: StrokePoint, color: [f32; 4]) -> bool {
        if self.active_stroke.is_some() {
            return false;
        }
        let Some(layer_index) = self.selected_layer_index() else {
            return false;
        };
        if !self.layers[layer_index].visible {
            return false;
        }
        let layer_id = self.layers[layer_index].id;
        if !self.history.begin_stroke(layer_id) {
            return false;
        }
        let active_stroke = ActiveStroke::new(layer_id, tool, color);
        self.active_stroke = Some(active_stroke);
        if active_stroke.render_path() == StrokeRenderPath::Mask {
            let clipped: Vec<_> = self.layers.iter().map(|layer| layer.clipped).collect();
            let clipping_base = clipping_base_index(&clipped, layer_index).map(|base_index| {
                (
                    &self.layers[base_index].view,
                    &self.layers[base_index].settings_buffer,
                )
            });
            self.resources.prepare_stroke_preview(
                self.gpu.device(),
                self.gpu.queue(),
                &self.layers[layer_index].view,
                &self.layers[layer_index].settings_buffer,
                clipping_base,
                active_stroke.color,
            );
        }
        let needs_history_sync = self.history.layer_needs_sync(layer_id);
        if needs_history_sync || tool == PaintTool::Smudge {
            let mut encoder =
                self.gpu
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("stroke setup encoder"),
                    });
            if needs_history_sync {
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
            }
            if tool == PaintTool::Smudge {
                // Smudge samples this snapshot because the layer cannot be sampled while attached.
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.layers[layer_index].texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.resources.smudge_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: self.document_size[0],
                        height: self.document_size[1],
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.gpu.queue().submit(std::iter::once(encoder.finish()));
        }
        self.stamp_queue.begin_stroke(origin);
        true
    }

    pub fn end_stroke(&mut self) {
        let Some(active_stroke) = self.active_stroke else {
            return;
        };
        self.flush_all_stamps();
        let Some(rect) = self.stamp_queue.end_stroke() else {
            self.history.end_empty_stroke();
            self.finish_active_stroke();
            return;
        };

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("history commit encoder"),
                });
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active_stroke.layer_id)
            .expect("active stroke layer must exist");
        let commit_pipeline = match active_stroke.tool {
            PaintTool::Brush => Some(&self.resources.brush_commit_pipeline),
            PaintTool::Eraser => Some(&self.resources.eraser_commit_pipeline),
            PaintTool::Smudge => None,
        };
        if let Some(commit_pipeline) = commit_pipeline {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stroke commit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.layers[layer_index].view,
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
            pass.set_pipeline(commit_pipeline);
            pass.set_bind_group(0, &self.resources.stroke_commit_bind_group, &[]);
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.draw(0..3, 0..1);
        }
        if active_stroke.render_path() == StrokeRenderPath::Mask {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stroke mask dirty rect clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.resources.stroke_mask_view,
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
            pass.set_pipeline(&self.resources.mask_clear_pipeline);
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.draw(0..3, 0..1);
        }
        self.history.commit_stroke(
            self.gpu.device(),
            &mut encoder,
            active_stroke.layer_id,
            &self.layers[layer_index].texture,
            rect,
        );
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
        self.mark_layer_changed(active_stroke.layer_id);
        self.finish_active_stroke();
    }

    pub fn can_undo(&self) -> bool {
        self.active_stroke.is_none() && !self.stamp_queue.has_pending() && self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.active_stroke.is_none() && !self.stamp_queue.has_pending() && self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        if self.active_stroke.is_some() {
            return false;
        }
        match self.history.undo_target() {
            Some(HistoryTarget::Structure) => {
                let Some(effect) = self.history.undo_structure(
                    &mut self.layers,
                    &mut self.selection,
                    &mut self.background_color,
                ) else {
                    return false;
                };
                self.apply_structure_effect(effect);
                true
            }
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("undo layer must exist");
                let mut encoder =
                    self.gpu
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("undo encoder"),
                        });
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
                self.history
                    .undo_stroke(&mut encoder, &self.layers[layer_index].texture);
                self.gpu.queue().submit(std::iter::once(encoder.finish()));
                self.mark_layer_changed(layer_id);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        if self.active_stroke.is_some() {
            return false;
        }
        match self.history.redo_target() {
            Some(HistoryTarget::Structure) => {
                let Some(effect) = self.history.redo_structure(
                    &mut self.layers,
                    &mut self.selection,
                    &mut self.background_color,
                ) else {
                    return false;
                };
                self.apply_structure_effect(effect);
                true
            }
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("redo layer must exist");
                let mut encoder =
                    self.gpu
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("redo encoder"),
                        });
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
                self.history
                    .redo_stroke(&mut encoder, &self.layers[layer_index].texture);
                self.gpu.queue().submit(std::iter::once(encoder.finish()));
                self.mark_layer_changed(layer_id);
                true
            }
            None => false,
        }
    }

    pub fn queue_stamp(&mut self, point: StrokePoint) -> bool {
        let Some(active_stroke) = self.active_stroke else {
            return false;
        };
        self.stamp_queue.queue_point(
            point,
            active_stroke.color,
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn stamp_line(
        &mut self,
        from: StrokePoint,
        to: StrokePoint,
        spacing: BrushSpacing,
    ) -> usize {
        let Some(active_stroke) = self.active_stroke else {
            return 0;
        };
        self.stamp_queue.stamp_line(
            from,
            to,
            active_stroke.color,
            effective_spacing(active_stroke.tool, spacing),
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn clear_canvas(&mut self) {
        if self.active_stroke.is_some() {
            return;
        }
        self.stamp_queue.clear();
        self.history.clear();
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("clear canvas encoder"),
                });
        let layer_index = self
            .selected_layer_index()
            .expect("clear requires paint layer");
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.layers[layer_index].view,
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
        self.history.sync_layer(
            &mut encoder,
            self.layers[layer_index].id,
            &self.layers[layer_index].texture,
            TextureRect {
                x: 0,
                y: 0,
                width: self.document_size[0],
                height: self.document_size[1],
            },
        );
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
        self.mark_layer_changed(self.layers[layer_index].id);
    }

    pub fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.gpu.acquire_frame()
    }

    pub fn reconfigure_surface(&self) {
        self.gpu.reconfigure_surface();
    }

    pub fn render_to_view(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        brush_cursor: Option<BrushCursor>,
    ) {
        self.flush_stamps(encoder);
        // Keep the active layer's thumbnail dirty until the stroke is committed. Updating it for
        // every dab adds a render pass to the latency-sensitive painting path.
        self.render_layer_previews(encoder);
        self.write_view_uniform();
        self.ensure_clipped_layer_bind_groups();
        if let Some(cursor) = brush_cursor {
            self.write_brush_cursor(cursor);
        }

        let canvas_rect = visible_canvas_rect(
            self.view.snapshot(),
            self.document_size,
            self.surface_size(),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas background pass"),
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
            if let Some(canvas_rect) = canvas_rect {
                pass.set_scissor_rect(
                    canvas_rect.x,
                    canvas_rect.y,
                    canvas_rect.width,
                    canvas_rect.height,
                );
                pass.set_pipeline(&self.resources.background_pipeline);
                pass.set_bind_group(0, &self.layers[0].blit_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        if let Some(canvas_rect) = canvas_rect {
            let mut base_index = 0;
            while base_index < self.layers.len() {
                if self.layers[base_index].clipped {
                    base_index += 1;
                    continue;
                }
                let mut group_end = base_index + 1;
                while group_end < self.layers.len() && self.layers[group_end].clipped {
                    group_end += 1;
                }
                let base = &self.layers[base_index];
                if !base.visible {
                    base_index = group_end;
                    continue;
                }
                let has_visible_clips = self.layers[base_index + 1..group_end]
                    .iter()
                    .any(|layer| layer.visible);

                if has_visible_clips {
                    // Clipped layers must be composed with their base before the group is put over
                    // the canvas. Applying each masked layer directly over the canvas multiplies
                    // the base alpha twice and leaves translucent base color showing through.
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("clipping group pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.resources.clipping_group_view,
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
                    let base_preview = self
                        .active_stroke
                        .filter(|stroke| stroke.layer_id == base.id)
                        .map(|stroke| stroke.tool);
                    match base_preview {
                        Some(PaintTool::Brush) => {
                            pass.set_pipeline(&self.resources.group_brush_preview_pipeline);
                            pass.set_bind_group(0, self.resources.stroke_preview_bind_group(), &[]);
                        }
                        Some(PaintTool::Eraser) => {
                            pass.set_pipeline(&self.resources.group_eraser_preview_pipeline);
                            pass.set_bind_group(0, self.resources.stroke_preview_bind_group(), &[]);
                        }
                        Some(PaintTool::Smudge) | None => {
                            pass.set_pipeline(&self.resources.merge_pipeline);
                            pass.set_bind_group(0, &base.blit_bind_group, &[]);
                        }
                    }
                    pass.draw(0..3, 0..1);

                    for layer in &self.layers[base_index + 1..group_end] {
                        if !layer.visible {
                            continue;
                        }
                        let preview = self
                            .active_stroke
                            .filter(|stroke| stroke.layer_id == layer.id)
                            .map(|stroke| stroke.tool);
                        match preview {
                            Some(PaintTool::Brush) => {
                                pass.set_pipeline(
                                    &self.resources.group_clipped_brush_preview_pipeline,
                                );
                                pass.set_bind_group(
                                    0,
                                    self.resources.stroke_preview_bind_group(),
                                    &[],
                                );
                            }
                            Some(PaintTool::Eraser) => {
                                pass.set_pipeline(
                                    &self.resources.group_clipped_eraser_preview_pipeline,
                                );
                                pass.set_bind_group(
                                    0,
                                    self.resources.stroke_preview_bind_group(),
                                    &[],
                                );
                            }
                            Some(PaintTool::Smudge) | None => {
                                pass.set_pipeline(&self.resources.clipped_layer_merge_pipeline);
                                let bind_group = self
                                    .clipped_layer_bind_groups
                                    .get(&layer.id)
                                    .expect("clipped layer must have a bind group");
                                pass.set_bind_group(0, bind_group, &[]);
                            }
                        }
                        pass.draw(0..3, 0..1);
                    }
                    drop(pass);

                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("clipping group blit pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                    pass.set_scissor_rect(
                        canvas_rect.x,
                        canvas_rect.y,
                        canvas_rect.width,
                        canvas_rect.height,
                    );
                    pass.set_pipeline(&self.resources.layer_pipeline);
                    pass.set_bind_group(0, self.resources.clipping_group_bind_group(), &[]);
                    pass.draw(0..3, 0..1);
                } else {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("layer blit pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                    pass.set_scissor_rect(
                        canvas_rect.x,
                        canvas_rect.y,
                        canvas_rect.width,
                        canvas_rect.height,
                    );
                    let preview = self
                        .active_stroke
                        .filter(|stroke| stroke.layer_id == base.id)
                        .map(|stroke| stroke.tool);
                    match preview {
                        Some(PaintTool::Brush) => {
                            pass.set_pipeline(&self.resources.brush_preview_pipeline);
                            pass.set_bind_group(0, self.resources.stroke_preview_bind_group(), &[]);
                        }
                        Some(PaintTool::Eraser) => {
                            pass.set_pipeline(&self.resources.eraser_preview_pipeline);
                            pass.set_bind_group(0, self.resources.stroke_preview_bind_group(), &[]);
                        }
                        Some(PaintTool::Smudge) | None => {
                            pass.set_pipeline(&self.resources.layer_pipeline);
                            pass.set_bind_group(0, &base.blit_bind_group, &[]);
                        }
                    }
                    pass.draw(0..3, 0..1);
                }
                base_index = group_end;
            }
        }

        if brush_cursor.is_some() {
            let surface_size = self.surface_size();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("brush cursor pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
            pass.set_scissor_rect(0, 0, surface_size[0], surface_size[1]);
            pass.set_pipeline(&self.resources.cursor_pipeline);
            pass.set_bind_group(0, &self.resources.cursor_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    fn ensure_clipped_layer_bind_groups(&mut self) {
        if !self.clipping_bind_groups_dirty {
            return;
        }
        self.clipped_layer_bind_groups.clear();
        let clipped: Vec<_> = self.layers.iter().map(|layer| layer.clipped).collect();
        for (index, layer) in self.layers.iter().enumerate() {
            let Some(base_index) = clipping_base_index(&clipped, index) else {
                continue;
            };
            let bind_group = self.resources.create_clipped_layer_bind_group(
                self.gpu.device(),
                layer,
                &self.layers[base_index],
            );
            self.clipped_layer_bind_groups.insert(layer.id, bind_group);
        }
        self.clipping_bind_groups_dirty = false;
    }

    fn render_layer_previews(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let active_layer = self.active_stroke.map(|stroke| stroke.layer_id);
        for layer in &mut self.layers {
            if !layer.preview_dirty || active_layer == Some(layer.id) {
                continue;
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("layer preview pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &layer.preview_view,
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
                pass.set_pipeline(&self.resources.layer_thumbnail_pipeline);
                pass.set_bind_group(0, &layer.preview_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            layer.preview_dirty = false;
        }
    }

    fn flush_all_stamps(&mut self) {
        while self.stamp_queue.has_pending() {
            let mut encoder =
                self.gpu
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("stroke flush encoder"),
                    });
            self.flush_stamps(&mut encoder);
            self.gpu.queue().submit(std::iter::once(encoder.finish()));
        }
    }

    fn flush_stamps(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let raw = self.stamp_queue.drain_raw(
            self.document_size[0],
            self.document_size[1],
            MAX_STAMPS_PER_FRAME,
        );
        let count = raw.len();
        if count == 0 {
            return;
        }

        self.gpu
            .queue()
            .write_buffer(&self.resources.stamp_buffer, 0, bytemuck::cast_slice(&raw));

        let active_stroke = self.active_stroke.expect("stamp requires active stroke");
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active_stroke.layer_id)
            .expect("active stroke layer must exist");
        self.layers[layer_index].preview_dirty = true;
        if active_stroke.render_path() == StrokeRenderPath::DirectSmudge {
            self.flush_smudge_stamps(encoder, layer_index, &raw);
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stroke mask stamp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.resources.stroke_mask_view,
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
        debug_assert!(matches!(
            active_stroke.tool,
            PaintTool::Brush | PaintTool::Eraser
        ));
        pass.set_pipeline(&self.resources.mask_pipeline);
        pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    fn flush_smudge_stamps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layer_index: usize,
        stamps: &[StampRaw],
    ) {
        for (index, stamp) in stamps.iter().copied().enumerate() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("smudge pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.layers[layer_index].view,
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
                pass.set_pipeline(&self.resources.smudge_pipeline);
                pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
                pass.draw(0..6, index as u32..index as u32 + 1);
            }

            // The next dab must sample the result of this one.
            let target = stamp.target_rect();
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.layers[layer_index].texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: target.x,
                        y: target.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.resources.smudge_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: target.x,
                        y: target.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: target.width,
                    height: target.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn finish_active_stroke(&mut self) {
        self.active_stroke = None;
        self.resources.clear_stroke_preview();
    }

    fn resize_document_resources(&mut self, size: [u32; 2]) {
        if size == self.document_size {
            return;
        }
        self.resources
            .resize_document(self.gpu.device(), self.gpu.queue(), size);
        self.history = PaintHistory::new(self.gpu.device(), size);
        self.document_size = size;
    }

    fn layer_resource_byte_len(&self) -> u64 {
        let canvas = u64::from(self.document_size[0]) * u64::from(self.document_size[1]) * 4;
        let preview = u64::from(LAYER_PREVIEW_SIZE) * u64::from(LAYER_PREVIEW_SIZE) * 4;
        canvas + preview
    }

    fn allocate_layer_resource_id(&mut self) -> LayerResourceId {
        let id = LayerResourceId(self.next_layer_resource_id);
        self.next_layer_resource_id = self
            .next_layer_resource_id
            .checked_add(1)
            .expect("layer resource ID space exhausted");
        id
    }

    fn selected_layer_index(&self) -> Option<usize> {
        self.layers
            .iter()
            .position(|layer| layer.id == self.selection)
    }

    fn reset_change_tracking(&mut self, dirty: bool) {
        let version = u64::from(dirty);
        self.document_generation = version;
        self.metadata_version = version;
        self.layer_versions = self
            .layers
            .iter()
            .map(|layer| (layer.id, version))
            .collect();
    }

    fn next_document_generation(&mut self) -> u64 {
        self.document_generation = self.document_generation.saturating_add(1);
        self.document_generation
    }

    fn mark_entire_document_changed(&mut self) {
        let version = self.next_document_generation();
        self.metadata_version = version;
        self.layer_versions = self
            .layers
            .iter()
            .map(|layer| (layer.id, version))
            .collect();
        for layer in &mut self.layers {
            layer.preview_dirty = true;
        }
    }

    fn mark_metadata_changed(&mut self) {
        self.metadata_version = self.next_document_generation();
        self.clipping_bind_groups_dirty = true;
    }

    fn mark_layer_changed(&mut self, id: LayerId) {
        if let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) {
            layer.preview_dirty = true;
        }
        let version = self.next_document_generation();
        self.layer_versions.insert(id, version);
    }

    fn apply_structure_effect(&mut self, effect: StructureEffect) {
        self.clipping_bind_groups_dirty = true;
        if let StructureEffect::CanvasResized { size } = effect {
            self.resources
                .resize_document(self.gpu.device(), self.gpu.queue(), size);
            self.history.resize_mirror(self.gpu.device(), size);
            self.document_size = size;
            self.fit_to_screen();
        }
        let version = self.next_document_generation();
        self.metadata_version = version;
        match effect {
            StructureEffect::MetadataOnly => {}
            StructureEffect::LayerAdded(id) => {
                self.layer_versions.insert(id, version);
            }
            StructureEffect::LayerRemoved(id) => {
                self.layer_versions.remove(&id);
            }
            StructureEffect::LayersMerged { result, removed } => {
                self.layer_versions.remove(&removed);
                self.layer_versions.insert(result, version);
            }
            StructureEffect::MergeUndone { lower, upper } => {
                self.layer_versions.insert(lower, version);
                self.layer_versions.insert(upper, version);
            }
            StructureEffect::CanvasResized { .. } => {
                self.layer_versions = self
                    .layers
                    .iter()
                    .map(|layer| (layer.id, version))
                    .collect();
                for layer in &mut self.layers {
                    layer.preview_dirty = true;
                }
            }
        }
        for layer in &self.layers {
            self.gpu.queue().write_buffer(
                &layer.settings_buffer,
                0,
                bytemuck::bytes_of(&LayerSettingsUniform {
                    opacity: f32::from(layer.opacity) / 100.0,
                    padding: [0.0; 3],
                }),
            );
        }
    }

    fn write_view_uniform(&mut self) {
        let snapshot = self.view.snapshot();
        let (document_from_window_x, document_from_window_y) = snapshot.window_to_document_rows();
        let uniform = ViewUniform {
            document_from_window_x,
            document_from_window_y,
            paint_dims: [self.document_size[0] as f32, self.document_size[1] as f32],
            padding: [0.0, 0.0],
            background_color: self.background_color,
        };
        if self.last_view_uniform == Some(uniform) {
            return;
        }
        self.gpu.queue().write_buffer(
            &self.resources.view_uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
        self.last_view_uniform = Some(uniform);
    }

    fn write_brush_cursor(&self, cursor: BrushCursor) {
        let half_size = self.brush_outline_half_size(cursor.diameter);
        let (axis_x, axis_y) = self.view.snapshot().document_axes_in_window();
        let surface_size = self.surface_size();
        self.gpu.queue().write_buffer(
            &self.resources.cursor_buffer,
            0,
            bytemuck::bytes_of(&CursorRaw {
                center: cursor.center,
                half_size,
                axis_x,
                axis_y,
                surface_size: [surface_size[0] as f32, surface_size[1] as f32],
                padding: [0.0; 2],
            }),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CanvasResizeCopy {
    source: TextureRect,
    destination: [u32; 2],
}

fn canvas_copy_for_resize(
    before: [u32; 2],
    after: [u32; 2],
    origin: [i32; 2],
) -> Option<CanvasResizeCopy> {
    let old_right = i64::from(before[0]);
    let old_bottom = i64::from(before[1]);
    let new_left = i64::from(origin[0]);
    let new_top = i64::from(origin[1]);
    let new_right = new_left + i64::from(after[0]);
    let new_bottom = new_top + i64::from(after[1]);
    let left = new_left.max(0);
    let top = new_top.max(0);
    let right = new_right.min(old_right);
    let bottom = new_bottom.min(old_bottom);
    if right <= left || bottom <= top {
        return None;
    }
    Some(CanvasResizeCopy {
        source: TextureRect {
            x: left as u32,
            y: top as u32,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        },
        destination: [(left - new_left) as u32, (top - new_top) as u32],
    })
}

fn layer_set_byte_len(size: [u32; 2], layer_count: usize) -> u64 {
    let canvas = u64::from(size[0]) * u64::from(size[1]) * 4;
    let preview = u64::from(LAYER_PREVIEW_SIZE) * u64::from(LAYER_PREVIEW_SIZE) * 4;
    (canvas + preview).saturating_mul(layer_count as u64)
}

fn visible_canvas_rect(
    view: PaintViewSnapshot,
    document_size: [u32; 2],
    surface_size: [u32; 2],
) -> Option<TextureRect> {
    let [width, height] = document_size.map(|dimension| dimension as f32);
    let corners = [
        view.document_to_window([0.0, 0.0]),
        view.document_to_window([width, 0.0]),
        view.document_to_window([0.0, height]),
        view.document_to_window([width, height]),
    ];
    if corners.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let min = corners.iter().fold([f32::INFINITY; 2], |min, corner| {
        [min[0].min(corner[0]), min[1].min(corner[1])]
    });
    let max = corners.iter().fold([f32::NEG_INFINITY; 2], |max, corner| {
        [max[0].max(corner[0]), max[1].max(corner[1])]
    });

    let surface_width = surface_size[0] as f32;
    let surface_height = surface_size[1] as f32;
    let left = min[0].floor().clamp(0.0, surface_width) as u32;
    let top = min[1].floor().clamp(0.0, surface_height) as u32;
    let right = max[0].ceil().clamp(0.0, surface_width) as u32;
    let bottom = max[1].ceil().clamp(0.0, surface_height) as u32;

    (right > left && bottom > top).then_some(TextureRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn effective_spacing(tool: PaintTool, spacing: BrushSpacing) -> BrushSpacing {
    BrushSpacing {
        ratio: if tool == PaintTool::Smudge {
            spacing.ratio.max(SMUDGE_MIN_STEP_RATIO)
        } else {
            spacing.ratio
        },
        minimum: spacing.minimum,
    }
}

fn premultiply(mut color: [f32; 4]) -> [f32; 4] {
    color[0] *= color[3];
    color[1] *= color[3];
    color[2] *= color[3];
    color
}

fn opaque_color(color: [u8; 3]) -> [f32; 4] {
    [
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        1.0,
    ]
}

fn rgb8(color: [f32; 4]) -> [u8; 3] {
    [color[0], color[1], color[2]].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn next_layer_number(layers: &[LayerManifest]) -> u64 {
    layers
        .iter()
        .filter_map(|layer| layer.name.strip_prefix("Layer ")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1)
}

fn clear_layer(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("document layer clear encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("document layer clear pass"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_canvas_resize_maps_the_intersection_into_the_new_canvas() {
        assert_eq!(
            canvas_copy_for_resize([100, 80], [60, 50], [25, 10]),
            Some(CanvasResizeCopy {
                source: TextureRect {
                    x: 25,
                    y: 10,
                    width: 60,
                    height: 50,
                },
                destination: [0, 0],
            })
        );
        assert_eq!(
            canvas_copy_for_resize([100, 80], [140, 120], [-20, -30]),
            Some(CanvasResizeCopy {
                source: TextureRect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 80,
                },
                destination: [20, 30],
            })
        );
    }

    #[test]
    fn canvas_resize_can_create_a_blank_canvas_outside_the_old_bounds() {
        assert_eq!(canvas_copy_for_resize([100, 80], [20, 20], [120, 90]), None);
    }

    #[test]
    fn visible_canvas_rect_clips_to_the_surface() {
        let view = PaintViewSnapshot {
            zoom: 2.0,
            center: [27.25, 45.25],
            workspace_center: [27.25, 45.25],
            viewport_center: [75.0, 50.0],
            rotation: 0.0,
            flip: [1.0, 1.0],
        };

        assert_eq!(
            visible_canvas_rect(view, [100, 80], [150, 100]),
            Some(TextureRect {
                x: 20,
                y: 0,
                width: 130,
                height: 100,
            })
        );
    }

    #[test]
    fn rotated_canvas_scissor_uses_all_four_corners() {
        let view = PaintViewSnapshot {
            zoom: 1.0,
            center: [50.0, 25.0],
            workspace_center: [50.0, 25.0],
            viewport_center: [100.0, 100.0],
            rotation: std::f32::consts::FRAC_PI_2,
            flip: [1.0, 1.0],
        };

        assert_eq!(
            visible_canvas_rect(view, [100, 50], [200, 200]),
            Some(TextureRect {
                x: 75,
                y: 50,
                width: 50,
                height: 100,
            })
        );
    }

    #[test]
    fn canvas_outside_surface_has_no_scissor_rect() {
        let view = PaintViewSnapshot {
            zoom: 1.0,
            center: [250.0, 250.0],
            workspace_center: [250.0, 250.0],
            viewport_center: [50.0, 50.0],
            rotation: 0.0,
            flip: [1.0, 1.0],
        };

        assert_eq!(visible_canvas_rect(view, [100, 100], [100, 100]), None);
    }

    #[test]
    fn persisted_layer_names_advance_the_default_number() {
        let layers = vec![
            LayerManifest {
                id: 1,
                name: "Layer 4".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
                file: "layers/1.png".to_owned(),
            },
            LayerManifest {
                id: 2,
                name: "Reference".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
                file: "layers/2.png".to_owned(),
            },
        ];
        assert_eq!(next_layer_number(&layers), 5);
    }

    #[test]
    fn document_background_round_trips_as_rgb8() {
        assert_eq!(rgb8(opaque_color([12, 34, 56])), [12, 34, 56]);
    }

    #[test]
    fn brush_and_eraser_keep_preset_spacing() {
        let spacing = BrushSpacing {
            ratio: 0.03,
            minimum: 2.0,
        };

        assert_eq!(effective_spacing(PaintTool::Brush, spacing), spacing);
        assert_eq!(effective_spacing(PaintTool::Eraser, spacing), spacing);
    }

    #[test]
    fn smudge_raises_dense_spacing_to_its_minimum_ratio() {
        let spacing = effective_spacing(
            PaintTool::Smudge,
            BrushSpacing {
                ratio: 0.03,
                minimum: 1.0,
            },
        );

        assert_eq!(spacing.ratio, SMUDGE_MIN_STEP_RATIO);
        assert_eq!(spacing.minimum, 1.0);
    }

    #[test]
    fn smudge_keeps_coarser_preset_spacing() {
        let spacing = BrushSpacing {
            ratio: 0.25,
            minimum: 3.0,
        };

        assert_eq!(effective_spacing(PaintTool::Smudge, spacing), spacing);
    }

    #[test]
    fn active_stroke_captures_layer_tool_and_premultiplied_color() {
        let stroke = ActiveStroke::new(LayerId(7), PaintTool::Brush, [0.8, 0.4, 0.2, 0.5]);

        assert_eq!(stroke.layer_id, LayerId(7));
        assert_eq!(stroke.tool, PaintTool::Brush);
        assert_eq!(stroke.color, [0.4, 0.2, 0.1, 0.5]);
    }

    #[test]
    fn canvas_size_constraints_reject_invalid_dimensions() {
        let constraints = CanvasSizeConstraints {
            max_dimension: 8192,
            max_pixels: 32 * 1024 * 1024,
        };

        assert!(constraints.validate(DEFAULT_CANVAS_SIZE).is_ok());
        assert!(constraints.validate([0, 100]).is_err());
        assert!(constraints.validate([8193, 100]).is_err());
        assert!(constraints.validate([8192, 8192]).is_err());
        assert!(constraints.validate([8192, 4096]).is_ok());
    }

    #[test]
    fn brush_and_eraser_use_masks_while_smudge_is_direct() {
        let color = [0.0, 0.0, 0.0, 1.0];

        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Brush, color).render_path(),
            StrokeRenderPath::Mask
        );
        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Eraser, color).render_path(),
            StrokeRenderPath::Mask
        );
        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Smudge, color).render_path(),
            StrokeRenderPath::DirectSmudge
        );
    }
}
