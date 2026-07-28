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
    history::{HistoryTarget, PaintHistory, TextureRect},
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
    layers::{DropEdge, LayerId, LayerInfo, LayerResourceId, LayerSnapshot},
    persistence::LayerReadback,
};
use crate::{
    artwork::{DOCUMENT_SCHEMA_VERSION, DocumentManifest, LayerManifest},
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
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    scale: [f32; 2],
    offset: [f32; 2],
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
    pub fn zoom(&self) -> f32 {
        self.view.zoom()
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

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        self.view.apply_zoom_at(factor, cursor);
    }

    pub fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.view.pan_by_window_delta(delta);
    }

    pub fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.window_to_document(point)
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
            selected_layer: self.selection.0,
            layers: self
                .layers
                .iter()
                .map(|layer| LayerManifest {
                    id: layer.id.0,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    file: format!("layers/{}.png", layer.id.0),
                })
                .collect(),
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
        self.fit_to_screen();
        self.reset_change_tracking(true);
        Ok(())
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
        layer.name = name;
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
        layer.visible = visible;
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
        let layer = self.layers.remove(dragged_index);
        self.layers.insert(insertion, layer);
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

    pub(crate) fn can_delete_selected_layer(&self) -> bool {
        self.layers.len() > 1
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
            self.resources.prepare_stroke_preview(
                self.gpu.device(),
                self.gpu.queue(),
                &self.layers[layer_index].view,
                &self.layers[layer_index].settings_buffer,
                active_stroke.color,
            );
        }
        let needs_history_sync = self.history.layer_needs_sync(layer_id);
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
        } else {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stroke mask clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.resources.stroke_mask_view,
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
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
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
        let changed = match self.history.undo_target() {
            Some(HistoryTarget::Structure) => self.history.undo_structure(
                &mut self.layers,
                &mut self.selection,
                &mut self.background_color,
            ),
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
                true
            }
            None => false,
        };
        if changed {
            self.mark_all_changed();
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        if self.active_stroke.is_some() {
            return false;
        }
        let changed = match self.history.redo_target() {
            Some(HistoryTarget::Structure) => self.history.redo_structure(
                &mut self.layers,
                &mut self.selection,
                &mut self.background_color,
            ),
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
                true
            }
            None => false,
        };
        if changed {
            self.mark_all_changed();
        }
        changed
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
        // egui samples these views later in this encoder, so previews include this frame's dabs.
        self.render_layer_previews(encoder);
        self.write_view_uniform();
        if let Some(cursor) = brush_cursor {
            self.write_brush_cursor(cursor);
        }

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
        pass.set_pipeline(&self.resources.background_pipeline);
        pass.set_bind_group(0, &self.layers[0].blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
        for layer in &self.layers {
            if !layer.visible {
                continue;
            }
            let preview_tool = self
                .active_stroke
                .filter(|stroke| stroke.layer_id == layer.id)
                .map(|stroke| stroke.tool);
            match preview_tool {
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
                    pass.set_bind_group(0, &layer.blit_bind_group, &[]);
                }
            }
            pass.draw(0..3, 0..1);
        }
        if brush_cursor.is_some() {
            pass.set_pipeline(&self.resources.cursor_pipeline);
            pass.set_bind_group(0, &self.resources.cursor_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }

    fn render_layer_previews(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let active_stroke = self.active_stroke;
        for layer in &mut self.layers {
            if !layer.preview_dirty {
                continue;
            }
            let pipeline = match active_stroke.filter(|stroke| stroke.layer_id == layer.id) {
                Some(ActiveStroke {
                    tool: PaintTool::Brush,
                    ..
                }) => &self.resources.brush_thumbnail_pipeline,
                Some(ActiveStroke {
                    tool: PaintTool::Eraser,
                    ..
                }) => &self.resources.eraser_thumbnail_pipeline,
                Some(ActiveStroke {
                    tool: PaintTool::Smudge,
                    ..
                })
                | None => &self.resources.layer_thumbnail_pipeline,
            };
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
                pass.set_pipeline(pipeline);
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

    fn mark_metadata_changed(&mut self) {
        self.metadata_version = self.next_document_generation();
    }

    fn mark_layer_changed(&mut self, id: LayerId) {
        if let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) {
            layer.preview_dirty = true;
        }
        let version = self.next_document_generation();
        self.layer_versions.insert(id, version);
    }

    fn mark_all_changed(&mut self) {
        for layer in &mut self.layers {
            layer.preview_dirty = true;
        }
        let version = self.next_document_generation();
        self.metadata_version = version;
        self.layer_versions = self
            .layers
            .iter()
            .map(|layer| (layer.id, version))
            .collect();
    }

    fn write_view_uniform(&self) {
        self.gpu.queue().write_buffer(
            &self.resources.view_uniform_buffer,
            0,
            bytemuck::bytes_of(&ViewUniform {
                scale: [1.0 / self.view.zoom(), 1.0 / self.view.zoom()],
                offset: self.view.offset(),
                paint_dims: [self.document_size[0] as f32, self.document_size[1] as f32],
                padding: [0.0, 0.0],
                background_color: self.background_color,
            }),
        );
    }

    fn write_brush_cursor(&self, cursor: BrushCursor) {
        let half_size = self.brush_outline_half_size(cursor.diameter);
        let surface_size = self.surface_size();
        self.gpu.queue().write_buffer(
            &self.resources.cursor_buffer,
            0,
            bytemuck::bytes_of(&CursorRaw {
                center: cursor.center,
                half_size,
                surface_size: [surface_size[0] as f32, surface_size[1] as f32],
                padding: [0.0; 2],
            }),
        );
    }
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
    fn persisted_layer_names_advance_the_default_number() {
        let layers = vec![
            LayerManifest {
                id: 1,
                name: "Layer 4".to_owned(),
                visible: true,
                opacity: 100,
                file: "layers/1.png".to_owned(),
            },
            LayerManifest {
                id: 2,
                name: "Reference".to_owned(),
                visible: true,
                opacity: 100,
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
