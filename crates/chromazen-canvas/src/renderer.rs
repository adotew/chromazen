use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use bytemuck::{Pod, Zeroable};

mod history;
mod layers;
mod persistence;
mod resources;
mod sampling;
mod stamps;
mod tiles;
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
    tiles::{
        TILE_SIZE, Tile, TileCoord, TileSet, all_tile_coords, copy_texture_region,
        region_has_alpha, tile_document_rect, tile_local_rect, tile_spans,
    },
    view::PaintView,
};
pub use self::{
    layers::{
        DropEdge, LayerId, LayerInfo, LayerResourceId, LayerSnapshot, merge_down_target_index,
    },
    persistence::LayerReadback,
    view::PaintViewSnapshot,
};
use crate::{BrushSpacing, PaintTool, StrokePoint};

pub const DEFAULT_CANVAS_SIZE: [u32; 2] = [4000, 4000];
pub(crate) const DEFAULT_BACKGROUND_COLOR: [f32; 4] = [1.0; 4];
const MAX_CANVAS_DIMENSION: u32 = 8192;
// Caps the document-sized smudge and mask allocations and full-layer readbacks.
const MAX_CANVAS_PIXELS: u64 = 32 * 1024 * 1024;
const DOCUMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const STROKE_MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const LAYER_PREVIEW_SIZE: u32 = 128;
// Eight simulation steps per brush radius retain tip detail without dense-pass overhead.
const SMUDGE_MIN_STEP_RATIO: f32 = 0.125;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasSizeConstraints {
    pub max_dimension: u32,
    pub max_pixels: u64,
}

impl CanvasSizeConstraints {
    pub fn validate(self, size: [u32; 2]) -> Result<(), String> {
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
struct TileUniform {
    origin: [f32; 2],
    padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TransformUniform {
    source_from_destination_x: [f32; 4],
    source_from_destination_y: [f32; 4],
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
pub struct BrushCursor {
    pub center: [f32; 2],
    pub diameter: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerContentBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl LayerContentBounds {
    fn center(self) -> [f32; 2] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
        ]
    }

    fn pixel_rect(self) -> TextureRect {
        let min = self.min.map(|value| value.floor().max(0.0) as u32);
        let max = self.max.map(|value| value.ceil().max(0.0) as u32);
        TextureRect {
            x: min[0],
            y: min[1],
            width: max[0].saturating_sub(min[0]),
            height: max[1].saturating_sub(min[1]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerTransform {
    pub translation: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
}

impl Default for LayerTransform {
    fn default() -> Self {
        Self {
            translation: [0.0; 2],
            scale: [1.0; 2],
            rotation: 0.0,
        }
    }
}

impl LayerTransform {
    const MIN_SCALE: f32 = 0.01;
    const MAX_SCALE: f32 = 100.0;

    fn normalized(mut self) -> Option<Self> {
        if self
            .translation
            .into_iter()
            .chain(self.scale)
            .chain([self.rotation])
            .any(|value| !value.is_finite())
            || self.scale.into_iter().any(|scale| scale <= 0.0)
        {
            return None;
        }
        self.scale = self
            .scale
            .map(|scale| scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE));
        self.rotation = (self.rotation + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        Some(self)
    }

    fn is_identity(self) -> bool {
        self.translation == [0.0; 2]
            && self
                .scale
                .into_iter()
                .all(|scale| (scale - 1.0).abs() <= f32::EPSILON)
            && self.rotation.abs() <= f32::EPSILON
    }

    /// Maps a source point to its transformed destination, the inverse of [`Self::uniform`].
    fn map_point(self, pivot: [f32; 2], point: [f32; 2]) -> [f32; 2] {
        let (sin, cos) = self.rotation.sin_cos();
        let local = [
            (point[0] - pivot[0]) * self.scale[0],
            (point[1] - pivot[1]) * self.scale[1],
        ];
        [
            pivot[0] + self.translation[0] + cos * local[0] - sin * local[1],
            pivot[1] + self.translation[1] + sin * local[0] + cos * local[1],
        ]
    }

    fn uniform(self, pivot: [f32; 2]) -> TransformUniform {
        let (sin, cos) = self.rotation.sin_cos();
        let a = cos / self.scale[0];
        let b = sin / self.scale[0];
        let c = -sin / self.scale[1];
        let d = cos / self.scale[1];
        let origin = [
            pivot[0] + self.translation[0],
            pivot[1] + self.translation[1],
        ];
        TransformUniform {
            source_from_destination_x: [a, b, pivot[0] - a * origin[0] - b * origin[1], 0.0],
            source_from_destination_y: [c, d, pivot[1] - c * origin[0] - d * origin[1], 0.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentVersions {
    pub generation: u64,
    pub metadata: u64,
    pub layers: Vec<(LayerId, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasDocument {
    pub size: [u32; 2],
    pub background: [u8; 3],
    pub selected_layer: LayerId,
    pub layers: Vec<LayerInfo>,
}

impl CanvasDocument {
    fn validate(&self) -> Result<(), String> {
        if self.size.into_iter().any(|dimension| dimension == 0) {
            return Err("document dimensions must be non-zero".to_owned());
        }
        if self.layers.is_empty() {
            return Err("document must contain at least one layer".to_owned());
        }
        if !self
            .layers
            .iter()
            .any(|layer| layer.id == self.selected_layer)
        {
            return Err("selected_layer does not identify a document layer".to_owned());
        }
        if self.layers[0].clipped {
            return Err("bottom layer cannot be clipped".to_owned());
        }
        let mut ids = HashSet::new();
        for layer in &self.layers {
            if layer.id.0 == 0 || !ids.insert(layer.id) {
                return Err("layer IDs must be non-zero and unique".to_owned());
            }
            if layer.name.trim().is_empty() {
                return Err("layer names must not be empty".to_owned());
            }
            if layer.opacity > 100 {
                return Err("layer opacity must be between 0 and 100".to_owned());
            }
        }
        Ok(())
    }
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
    opacity: f32,
}

struct ActiveLayerTransform {
    layer_id: LayerId,
    bounds: LayerContentBounds,
    value: LayerTransform,
    bind_group: wgpu::BindGroup,
    /// The layer's tiles before the transform. The layer holds the rendered preview.
    original_tiles: BTreeMap<TileCoord, Tile>,
}

impl ActiveStroke {
    fn new(layer_id: LayerId, tool: PaintTool, color: [f32; 4], opacity: f32) -> Self {
        Self {
            layer_id,
            tool,
            color: premultiply(color),
            opacity,
        }
    }

    fn render_path(self) -> StrokeRenderPath {
        match self.tool {
            PaintTool::Brush | PaintTool::Eraser => StrokeRenderPath::Mask,
            PaintTool::Smudge => StrokeRenderPath::DirectSmudge,
        }
    }

    fn stamp_point(self, mut point: StrokePoint) -> StrokePoint {
        if self.render_path() == StrokeRenderPath::Mask {
            point.opacity *= self.opacity;
        }
        point
    }
}

pub struct Canvas {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_format: wgpu::TextureFormat,
    surface_size: [u32; 2],
    document_size: [u32; 2],
    resources: RenderResources,
    layers: Vec<PaintLayer>,
    selection: LayerId,
    background_color: [f32; 4],
    workspace_background_color: wgpu::Color,
    next_layer_id: u64,
    next_layer_number: u64,
    next_layer_resource_id: u64,
    stamp_queue: StampQueue,
    pending_preview_stamps: Option<Vec<StampRaw>>,
    rendered_preview_rect: Option<TextureRect>,
    active_stroke: Option<ActiveStroke>,
    /// Tiles replaced by copy-on-write during the active stroke, keyed by coordinate.
    stroke_original_tiles: BTreeMap<TileCoord, Option<Tile>>,
    active_transform: Option<ActiveLayerTransform>,
    content_bounds_cache: Option<(LayerId, LayerResourceId, Option<LayerContentBounds>)>,
    history: PaintHistory,
    view: PaintView,
    document_generation: u64,
    metadata_version: u64,
    layer_versions: HashMap<LayerId, u64>,
    clipped_layer_bind_groups: HashMap<LayerId, wgpu::BindGroup>,
    clipping_bind_groups_dirty: bool,
    last_view_uniform: Option<ViewUniform>,
}

impl Canvas {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        surface_size: [u32; 2],
        document_size: [u32; 2],
        brush_stamp: &image::RgbaImage,
        workspace_background_color: [f32; 3],
    ) -> Result<Self, String> {
        CanvasSizeConstraints {
            max_dimension: device
                .limits()
                .max_texture_dimension_2d
                .min(MAX_CANVAS_DIMENSION),
            max_pixels: MAX_CANVAS_PIXELS,
        }
        .validate(document_size)?;
        validate_brush_stamp(brush_stamp, device.limits().max_texture_dimension_2d)?;
        let resources = RenderResources::new(
            &device,
            &queue,
            document_size,
            surface_size,
            surface_format,
            brush_stamp,
        )?;

        let first_layer = resources.create_paint_layer(
            &device,
            LayerId(1),
            LayerResourceId(1),
            LayerProperties::new("Layer 1".to_owned()),
        );
        let stamp_aspect = brush_stamp.width() as f32 / brush_stamp.height() as f32;
        let history = PaintHistory::new();
        let mut renderer = Self {
            device,
            queue,
            surface_format,
            surface_size,
            document_size,
            resources,
            layers: vec![first_layer],
            selection: LayerId(1),
            background_color: DEFAULT_BACKGROUND_COLOR,
            workspace_background_color: wgpu::Color {
                r: f64::from(workspace_background_color[0]),
                g: f64::from(workspace_background_color[1]),
                b: f64::from(workspace_background_color[2]),
                a: 1.0,
            },
            next_layer_id: 2,
            next_layer_number: 2,
            next_layer_resource_id: 2,
            stamp_queue: StampQueue::new(stamp_aspect),
            pending_preview_stamps: None,
            rendered_preview_rect: None,
            active_stroke: None,
            stroke_original_tiles: BTreeMap::new(),
            active_transform: None,
            content_bounds_cache: None,
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
        renderer.reset_change_tracking(false);
        Ok(renderer)
    }

    fn surface_size(&self) -> [u32; 2] {
        self.surface_size
    }
    pub fn document_size(&self) -> [u32; 2] {
        self.document_size
    }

    pub fn set_workspace_background_color(&mut self, color: [f32; 3]) {
        self.workspace_background_color = wgpu::Color {
            r: f64::from(color[0]),
            g: f64::from(color[1]),
            b: f64::from(color[2]),
            a: 1.0,
        };
    }
    pub fn zoom(&self) -> f32 {
        self.view.zoom()
    }
    pub fn view_snapshot(&self) -> PaintViewSnapshot {
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

    pub fn canvas_size_constraints(&self) -> CanvasSizeConstraints {
        CanvasSizeConstraints {
            max_dimension: self
                .device
                .limits()
                .max_texture_dimension_2d
                .min(MAX_CANVAS_DIMENSION),
            max_pixels: MAX_CANVAS_PIXELS,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        self.surface_size = size;
        self.resources
            .resize_surface(&self.device, size, self.surface_format);
        self.view.set_surface_size(size);
    }

    pub fn try_set_brush_stamp(&mut self, stamp: &image::RgbaImage) -> Result<bool, String> {
        if self.active_stroke.is_some() || self.stamp_queue.has_pending() {
            return Ok(false);
        }
        validate_brush_stamp(stamp, self.device.limits().max_texture_dimension_2d)?;
        self.resources
            .replace_brush_stamp(&self.device, &self.queue, stamp)?;
        self.stamp_queue
            .set_stamp_aspect(stamp.width() as f32 / stamp.height() as f32);
        Ok(true)
    }

    fn fit_to_screen(&mut self) {
        self.view
            .fit_to_screen(self.surface_size(), self.document_size);
    }

    pub fn prepare_canvas_crop_view(&mut self) {
        self.fit_to_screen();
        let [width, height] = self.surface_size();
        self.view
            .apply_zoom_at(0.8, [width as f32 * 0.5, height as f32 * 0.5]);
    }

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        self.view.apply_zoom_at(factor, cursor);
    }

    pub fn canvas_rotation(&self) -> f32 {
        self.view.snapshot().rotation()
    }

    pub fn canvas_center_in_window(&self) -> [f32; 2] {
        self.view
            .snapshot()
            .document_to_window(self.document_center())
    }

    pub fn set_canvas_rotation(&mut self, radians: f32) -> bool {
        let center = self.document_center();
        self.view.set_rotation_around(radians, center)
    }

    pub fn rotate_canvas_view(&mut self, radians: f32) -> bool {
        let center = self.document_center();
        self.view.rotate_by_around(radians, center)
    }

    pub fn reset_canvas_rotation(&mut self) -> bool {
        let center = self.document_center();
        self.view.reset_rotation_around(center)
    }

    pub fn toggle_canvas_flip_horizontal(&mut self) {
        let center = self.document_center();
        self.view.toggle_flip_horizontal_around(center);
    }

    pub fn toggle_canvas_flip_vertical(&mut self) {
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

    pub fn window_to_workspace(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.snapshot().window_to_workspace(point)
    }

    pub fn sample_composited_color(&self, window_point: [f32; 2]) -> Option<[u8; 3]> {
        if !self.document_is_idle() {
            return None;
        }
        let pixel = document_pixel(
            self.view.window_to_document(window_point),
            self.document_size,
        )?;
        read_composited_color(
            &self.device,
            &self.queue,
            &self.layers,
            pixel,
            self.background_color,
        )
    }

    pub fn can_paint(&self) -> bool {
        self.document_is_idle()
            && self
                .selected_layer_index()
                .is_some_and(|index| self.layers[index].visible)
    }

    pub fn active_layer_transform(&self) -> Option<LayerTransform> {
        self.active_transform.as_ref().map(|active| active.value)
    }

    pub fn read_selected_layer_content_bounds(&mut self) -> Option<LayerContentBounds> {
        if let Some(active) = self.active_transform.as_ref() {
            return Some(active.bounds);
        }
        if !self.can_paint() {
            return None;
        }
        let layer_index = self.selected_layer_index()?;
        let layer_id = self.layers[layer_index].id;
        let layer_resource_id = self.layers[layer_index].resource_id;
        if self.layers[layer_index].tiles.is_empty() {
            return None;
        }
        if let Some((cached_id, cached_resource_id, bounds)) = self.content_bounds_cache
            && cached_id == layer_id
            && cached_resource_id == layer_resource_id
        {
            return bounds;
        }
        let bounds = persistence::begin_read_layers(
            &self.device,
            &self.queue,
            std::slice::from_ref(&self.layers[layer_index]),
            self.document_size,
        )
        .finish()
        .map_err(|error| log::error!("failed to find layer content bounds: {error}"))
        .ok()
        .and_then(|layers| layers.into_iter().next())
        .and_then(|(_, image)| alpha_content_bounds(&image));
        self.content_bounds_cache = Some((layer_id, layer_resource_id, bounds));
        bounds
    }

    fn begin_layer_transform(&mut self) -> bool {
        if self.active_transform.is_some() {
            return true;
        }
        if !self.can_paint() {
            return false;
        }
        let Some(bounds) = self.read_selected_layer_content_bounds() else {
            return false;
        };
        let layer_index = self
            .selected_layer_index()
            .expect("transform requires a selected layer");
        let layer_id = self.layers[layer_index].id;
        if !self.history.begin_stroke(layer_id) {
            return false;
        }

        // The transform samples arbitrary source positions, so the content is gathered into one
        // texture that covers only its bounds.
        let source_rect = bounds.pixel_rect();
        let source = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer transform source texture"),
            size: wgpu::Extent3d {
                width: source_rect.width,
                height: source_rect.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DOCUMENT_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer transform setup encoder"),
            });
        for span in tile_spans(source_rect) {
            if let Some(tile) = self.layers[layer_index].tiles.get(span.coord) {
                let local = span.local_rect();
                copy_texture_region(
                    &mut encoder,
                    &tile.texture,
                    [local.x, local.y],
                    &source,
                    [
                        span.document_rect.x - source_rect.x,
                        span.document_rect.y - source_rect.y,
                    ],
                    [local.width, local.height],
                );
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self
            .resources
            .create_transform_bind_group(&self.device, &source_view);
        let original_tiles = self.layers[layer_index].tiles.take_all();
        self.active_transform = Some(ActiveLayerTransform {
            layer_id,
            bounds,
            value: LayerTransform::default(),
            bind_group,
            original_tiles,
        });
        true
    }

    pub fn update_layer_transform(&mut self, transform: LayerTransform) -> bool {
        let Some(transform) = transform.normalized() else {
            return false;
        };
        if self.active_transform.is_none() && transform.is_identity() {
            return false;
        }
        if !self.begin_layer_transform()
            || self
                .active_transform
                .as_ref()
                .is_some_and(|active| active.value == transform)
        {
            return false;
        }

        let active = self
            .active_transform
            .as_ref()
            .expect("transform session must exist");
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active.layer_id)
            .expect("transformed layer must exist");
        let pivot = active.bounds.center();
        let source_rect = active.bounds.pixel_rect();
        self.resources.write_layer_transform_uniform(
            &self.queue,
            transform,
            pivot,
            [source_rect.x, source_rect.y],
        );

        // Render every tile the transformed bounds can reach; bilinear filtering may touch one
        // extra pixel on each side.
        let corners = [
            [active.bounds.min[0], active.bounds.min[1]],
            [active.bounds.max[0], active.bounds.min[1]],
            [active.bounds.min[0], active.bounds.max[1]],
            [active.bounds.max[0], active.bounds.max[1]],
        ]
        .map(|corner| transform.map_point(pivot, corner));
        let destination = document_rect_from_points(&corners, 1.0, self.document_size);
        let coords: BTreeSet<_> = destination
            .into_iter()
            .flat_map(tile_spans)
            .map(|span| span.coord)
            .collect();
        let uncovered: Vec<_> = self.layers[layer_index]
            .tiles
            .coords()
            .filter(|coord| !coords.contains(coord))
            .collect();
        for coord in uncovered {
            self.layers[layer_index].tiles.remove(coord);
        }
        for &coord in &coords {
            if !self.layers[layer_index].tiles.contains(coord) {
                let tile = self.resources.create_tile(&self.device, coord);
                self.layers[layer_index].tiles.insert(coord, tile);
            }
        }

        let active = self
            .active_transform
            .as_ref()
            .expect("transform session must exist");
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer transform preview encoder"),
            });
        for coord in coords {
            let tile = self.layers[layer_index]
                .tiles
                .get(coord)
                .expect("transform tile was allocated");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer transform preview pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tile.view,
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
            // Texels beyond the document edge must stay transparent.
            let document_area =
                tile_local_rect(coord, tile_document_rect(coord, self.document_size));
            pass.set_scissor_rect(
                document_area.x,
                document_area.y,
                document_area.width,
                document_area.height,
            );
            pass.set_pipeline(&self.resources.transform_pipeline);
            pass.set_bind_group(0, &active.bind_group, &[]);
            pass.set_bind_group(1, &self.resources.tile_slot(coord).origin_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.layers[layer_index].preview_dirty = true;
        self.active_transform
            .as_mut()
            .expect("transform session must exist")
            .value = transform;
        true
    }

    pub fn commit_layer_transform(&mut self) -> bool {
        let Some(active) = self.active_transform.take() else {
            return false;
        };
        if active.value.is_identity() {
            self.active_transform = Some(active);
            return self.cancel_layer_transform();
        }
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active.layer_id)
            .expect("transformed layer must exist");
        let mut original_tiles = active.original_tiles;
        let coords: BTreeSet<_> = original_tiles
            .keys()
            .copied()
            .chain(self.layers[layer_index].tiles.coords())
            .collect();
        let replaced = coords
            .into_iter()
            .map(|coord| (coord, original_tiles.remove(&coord)))
            .collect();
        self.history.commit_stroke(active.layer_id, replaced);
        self.mark_layer_changed(active.layer_id);
        true
    }

    pub fn cancel_layer_transform(&mut self) -> bool {
        let Some(active) = self.active_transform.take() else {
            return false;
        };
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active.layer_id)
            .expect("transformed layer must exist");
        self.layers[layer_index].tiles = TileSet::from(active.original_tiles);
        self.history.end_empty_stroke();
        self.layers[layer_index].preview_dirty = true;
        true
    }

    pub fn document_versions(&self) -> DocumentVersions {
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

    fn document_is_idle(&self) -> bool {
        self.active_stroke.is_none()
            && !self.history.stroke_active()
            && !self.stamp_queue.has_pending()
    }

    pub fn document_snapshot(&self) -> CanvasDocument {
        CanvasDocument {
            size: self.document_size,
            background: rgb8(self.background_color),
            selected_layer: self.selection,
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
        }
    }

    pub fn begin_document_layer_readback(&self) -> Result<LayerReadback, String> {
        if !self.document_is_idle() {
            return Err("the current document is busy".to_owned());
        }
        Ok(persistence::begin_read_layers(
            &self.device,
            &self.queue,
            &self.layers,
            self.document_size,
        ))
    }

    pub fn reset_document(&mut self, size: [u32; 2]) -> Result<(), String> {
        if !self.document_is_idle() {
            return Err("the current document is busy".to_owned());
        }
        self.canvas_size_constraints().validate(size)?;
        self.resize_document_resources(size);

        let id = LayerId(1);
        let resource_id = self.allocate_layer_resource_id();
        let layer = self.resources.create_paint_layer(
            &self.device,
            id,
            resource_id,
            LayerProperties::new("Layer 1".to_owned()),
        );
        self.layers = vec![layer];
        self.selection = id;
        self.background_color = DEFAULT_BACKGROUND_COLOR;
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

    pub fn resize_canvas(&mut self, size: [u32; 2], origin: [i32; 2]) -> Result<bool, String> {
        if !self.document_is_idle() {
            return Err("the current document is busy".to_owned());
        }
        self.canvas_size_constraints().validate(size)?;
        if size == self.document_size && origin == [0, 0] {
            return Ok(false);
        }

        let before_size = self.document_size;
        let copy = canvas_copy_for_resize(before_size, size, origin);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("canvas resize encoder"),
            });
        let mut replacement_layers = Vec::with_capacity(self.layers.len());
        for index in 0..self.layers.len() {
            let resource_id = self.allocate_layer_resource_id();
            let source = &self.layers[index];
            let mut layer = self.resources.create_paint_layer(
                &self.device,
                source.id,
                resource_id,
                LayerProperties {
                    name: source.name.clone(),
                    visible: source.visible,
                    opacity: source.opacity,
                    clipped: source.clipped,
                },
            );
            if let Some(copy) = copy {
                self.copy_tiles_shifted(&mut encoder, &source.tiles, copy, &mut layer.tiles);
            }
            replacement_layers.push(layer);
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        let previous_layers = std::mem::replace(&mut self.layers, replacement_layers);
        self.resources
            .resize_document(&self.device, &self.queue, size);
        self.document_size = size;
        self.history
            .record_canvas_resize(before_size, size, previous_layers);
        self.clipping_bind_groups_dirty = true;
        self.fit_to_screen();
        self.mark_entire_document_changed();
        Ok(true)
    }

    /// Copies `copy.source` from `source` into `destination` at `copy.destination`, allocating
    /// destination tiles only where a source tile exists.
    fn copy_tiles_shifted(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &TileSet,
        copy: CanvasResizeCopy,
        destination: &mut TileSet,
    ) {
        for source_span in tile_spans(copy.source) {
            let Some(source_tile) = source.get(source_span.coord) else {
                continue;
            };
            let [source_tile_x, source_tile_y] = source_span.coord.origin();
            let shifted = TextureRect {
                x: source_span.document_rect.x - copy.source.x + copy.destination[0],
                y: source_span.document_rect.y - copy.source.y + copy.destination[1],
                ..source_span.document_rect
            };
            for destination_span in tile_spans(shifted) {
                if !destination.contains(destination_span.coord) {
                    let tile = self
                        .resources
                        .create_tile(&self.device, destination_span.coord);
                    destination.insert(destination_span.coord, tile);
                }
                let destination_tile = destination
                    .get(destination_span.coord)
                    .expect("destination tile was allocated");
                let local = destination_span.local_rect();
                let source_x =
                    destination_span.document_rect.x - copy.destination[0] + copy.source.x;
                let source_y =
                    destination_span.document_rect.y - copy.destination[1] + copy.source.y;
                copy_texture_region(
                    encoder,
                    &source_tile.texture,
                    [source_x - source_tile_x, source_y - source_tile_y],
                    &destination_tile.texture,
                    [local.x, local.y],
                    [local.width, local.height],
                );
            }
        }
    }

    pub fn load_document(
        &mut self,
        document: &CanvasDocument,
        pixels: Vec<image::RgbaImage>,
    ) -> Result<(), String> {
        if !self.document_is_idle() {
            return Err("the current document is busy".to_owned());
        }
        document.validate()?;
        let document_size = document.size;
        self.canvas_size_constraints().validate(document_size)?;
        if pixels.len() != document.layers.len() {
            return Err("loaded layer count does not match document metadata".to_owned());
        }

        for (metadata, image) in document.layers.iter().zip(&pixels) {
            if image.dimensions() != (document.size[0], document.size[1]) {
                return Err(format!(
                    "layer {} has dimensions {}x{}; expected {}x{}",
                    metadata.id.0,
                    image.width(),
                    image.height(),
                    document.size[0],
                    document.size[1]
                ));
            }
        }

        self.resize_document_resources(document_size);
        let mut layers = Vec::with_capacity(document.layers.len());
        for (metadata, image) in document.layers.iter().zip(pixels) {
            let resource_id = self.allocate_layer_resource_id();
            let mut layer = self.resources.create_paint_layer(
                &self.device,
                metadata.id,
                resource_id,
                LayerProperties {
                    name: metadata.name.clone(),
                    visible: metadata.visible,
                    opacity: metadata.opacity,
                    clipped: metadata.clipped,
                },
            );
            for coord in all_tile_coords(document_size) {
                let rect = tile_document_rect(coord, document_size);
                if !region_has_alpha(&image, rect) {
                    continue;
                }
                let tile = self.resources.create_tile(&self.device, coord);
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tile.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    image.as_raw(),
                    wgpu::TexelCopyBufferLayout {
                        offset: (u64::from(rect.y) * u64::from(document_size[0])
                            + u64::from(rect.x))
                            * 4,
                        bytes_per_row: Some(document_size[0] * 4),
                        rows_per_image: Some(rect.height),
                    },
                    wgpu::Extent3d {
                        width: rect.width,
                        height: rect.height,
                        depth_or_array_layers: 1,
                    },
                );
                layer.tiles.insert(coord, tile);
            }
            layers.push(layer);
        }

        self.layers = layers;
        self.selection = document.selected_layer;
        self.background_color = opaque_color(document.background);
        self.next_layer_id = document
            .layers
            .iter()
            .map(|layer| layer.id.0)
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

    pub fn layer_snapshot(&self) -> LayerSnapshot {
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

    pub fn layer_preview_views(
        &self,
    ) -> impl Iterator<Item = (LayerId, LayerResourceId, &wgpu::TextureView)> {
        self.layers
            .iter()
            .map(|layer| (layer.id, layer.resource_id, &layer.preview_view))
    }

    pub fn select_layer(&mut self, id: LayerId) -> bool {
        if !self.document_is_idle() || self.selection == id {
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

    pub fn rename_layer(&mut self, id: LayerId, name: &str) -> bool {
        if !self.document_is_idle() {
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

    pub fn set_layer_clipped(&mut self, id: LayerId, clipped: bool) -> bool {
        if !self.document_is_idle() {
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

    pub fn set_layer_visibility(&mut self, id: LayerId, visible: bool) -> bool {
        if !self.document_is_idle() {
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

    pub fn set_layer_opacity(&mut self, id: LayerId, opacity: u8) -> bool {
        if !self.document_is_idle() || opacity > 100 {
            return false;
        }
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.opacity == opacity {
            return false;
        }
        layer.opacity = opacity;
        self.queue.write_buffer(
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

    pub fn commit_layer_opacity(&mut self, id: LayerId, before: u8, after: u8) -> bool {
        if !self.document_is_idle() || before == after || after > 100 {
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

    pub fn move_layer_relative(
        &mut self,
        dragged: LayerId,
        target: LayerId,
        edge: DropEdge,
    ) -> bool {
        if !self.document_is_idle() {
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

    pub fn set_background_color(&mut self, color: [u8; 3]) {
        let color = opaque_color(color);
        if self.document_is_idle() && self.background_color != color {
            self.background_color = color;
            self.mark_metadata_changed();
        }
    }

    pub fn commit_background_color(&mut self, before: [u8; 3], after: [u8; 3]) {
        if !self.document_is_idle() {
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

    pub fn add_layer(&mut self) -> bool {
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
            &self.device,
            id,
            resource_id,
            LayerProperties::new(name),
        );
        self.layers.insert(index, layer);
        self.selection = id;
        self.history
            .record_layer_addition(id, index, selection_before);
        self.mark_metadata_changed();
        self.layer_versions.insert(id, self.document_generation);
        true
    }

    pub fn duplicate_selected_layer(&mut self) -> bool {
        if !self.document_is_idle() {
            return false;
        }
        let Some(source_index) = self.selected_layer_index() else {
            return false;
        };
        let selection_before = self.selection;
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;
        let resource_id = self.allocate_layer_resource_id();
        let source = &self.layers[source_index];
        let properties = LayerProperties {
            name: format!("{} copy", source.name),
            visible: source.visible,
            opacity: source.opacity,
            clipped: source.clipped,
        };
        let mut layer =
            self.resources
                .create_paint_layer(&self.device, id, resource_id, properties);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("duplicate layer encoder"),
            });
        for (coord, source_tile) in self.layers[source_index].tiles.iter() {
            let tile = self.resources.create_tile(&self.device, coord);
            copy_texture_region(
                &mut encoder,
                &source_tile.texture,
                [0, 0],
                &tile.texture,
                [0, 0],
                [TILE_SIZE; 2],
            );
            layer.tiles.insert(coord, tile);
        }
        let index = insertion_index(Some(source_index), self.layers.len());
        self.layers.insert(index, layer);
        self.selection = id;
        self.history
            .record_layer_addition(id, index, selection_before);
        self.mark_metadata_changed();
        self.layer_versions.insert(id, self.document_generation);
        self.queue.submit(std::iter::once(encoder.finish()));
        true
    }

    fn can_merge_layer_down(&self, id: LayerId) -> bool {
        self.document_is_idle()
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

    pub fn merge_layer_down(&mut self, id: LayerId) -> bool {
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
        let mut merged = self.resources.create_paint_layer(
            &self.device,
            lower_id,
            resource_id,
            LayerProperties::new(lower_name),
        );

        let upper = self.layers.remove(upper_index);
        let lower = self.layers.remove(lower_index);
        let clipped_bind_group = upper.clipped.then(|| {
            self.resources
                .create_clipped_layer_bind_group(&self.device, &upper, &lower)
        });
        // A clipped layer only shows where its base has content.
        let coords: BTreeSet<_> = if upper.clipped {
            lower.tiles.coords().collect()
        } else {
            lower.tiles.coords().chain(upper.tiles.coords()).collect()
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer merge encoder"),
            });
        for coord in coords {
            let tile = self.resources.create_tile(&self.device, coord);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("layer merge pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &tile.view,
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
                let lower_tile = lower.tiles.get(coord);
                if let Some(lower_tile) = lower_tile {
                    pass.set_pipeline(&self.resources.merge_pipeline);
                    pass.set_bind_group(0, &lower.blit_bind_group, &[]);
                    pass.set_bind_group(1, &lower_tile.bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                if let Some(upper_tile) = upper.tiles.get(coord) {
                    if let (Some(bind_group), Some(lower_tile)) = (&clipped_bind_group, lower_tile)
                    {
                        pass.set_pipeline(&self.resources.clipped_layer_merge_pipeline);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.set_bind_group(1, &upper_tile.bind_group, &[]);
                        pass.set_bind_group(2, &lower_tile.bind_group, &[]);
                    } else {
                        pass.set_pipeline(&self.resources.merge_pipeline);
                        pass.set_bind_group(0, &upper.blit_bind_group, &[]);
                        pass.set_bind_group(1, &upper_tile.bind_group, &[]);
                    }
                    pass.draw(0..3, 0..1);
                }
            }
            merged.tiles.insert(coord, tile);
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        self.layers.insert(lower_index, merged);
        self.selection = lower_id;
        self.history
            .record_merge_down(upper, lower, lower_index, selection_before);
        self.layer_versions.remove(&id);
        let version = self.advance_document_generation();
        self.layer_versions.insert(lower_id, version);
        self.metadata_version = version;
        true
    }

    fn can_delete_selected_layer(&self) -> bool {
        if self.layers.len() == 1 {
            return self.document_is_idle();
        }
        self.selected_layer_index()
            .is_some_and(|index| index != 0 || !self.layers[1].clipped)
            && self.document_is_idle()
    }

    pub fn delete_selected_layer(&mut self) -> bool {
        if self.layers.len() == 1 {
            return self.clear_selected_layer();
        }
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
        self.history
            .record_layer_deletion(layer, index, selection_before, self.selection);
        self.layer_versions.remove(&selection_before);
        self.mark_metadata_changed();
        true
    }

    pub fn clear_selected_layer(&mut self) -> bool {
        if !self.document_is_idle() {
            return false;
        }
        let Some(layer_index) = self.selected_layer_index() else {
            return false;
        };
        let layer_id = self.layers[layer_index].id;
        if !self.history.begin_stroke(layer_id) {
            return false;
        }
        let replaced = self.layers[layer_index]
            .tiles
            .take_all()
            .into_iter()
            .map(|(coord, tile)| (coord, Some(tile)))
            .collect();
        self.history.commit_stroke(layer_id, replaced);
        self.mark_layer_changed(layer_id);
        true
    }

    pub fn begin_stroke(
        &mut self,
        tool: PaintTool,
        origin: StrokePoint,
        color: [f32; 4],
        opacity: f32,
    ) -> bool {
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
        let active_stroke = ActiveStroke::new(layer_id, tool, color, opacity);
        self.active_stroke = Some(active_stroke);
        if active_stroke.render_path() == StrokeRenderPath::Mask {
            let clipped: Vec<_> = self.layers.iter().map(|layer| layer.clipped).collect();
            let clipping_base = clipping_base_index(&clipped, layer_index)
                .map(|base_index| &self.layers[base_index].settings_buffer);
            self.resources.prepare_stroke_preview(
                &self.device,
                &self.queue,
                &self.layers[layer_index].settings_buffer,
                clipping_base,
                active_stroke.color,
            );
        }
        if tool == PaintTool::Smudge {
            // Smudge samples this snapshot because a tile cannot be sampled while it is
            // rendered. The new texture starts transparent, so only stored tiles are copied.
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("stroke setup encoder"),
                });
            let smudge = self
                .resources
                .begin_smudge(&self.device, self.document_size);
            for (coord, tile) in self.layers[layer_index].tiles.iter() {
                let rect = tile_document_rect(coord, self.document_size);
                copy_texture_region(
                    &mut encoder,
                    &tile.texture,
                    [0, 0],
                    &smudge.texture,
                    [rect.x, rect.y],
                    [rect.width, rect.height],
                );
            }
            self.queue.submit(std::iter::once(encoder.finish()));
        }
        self.stamp_queue
            .begin_stroke(active_stroke.stamp_point(origin));
        true
    }

    pub fn end_stroke(&mut self) {
        let Some(active_stroke) = self.active_stroke else {
            return;
        };
        self.flush_all_stamps();
        let Some((rect, touched_tiles)) = self.stamp_queue.end_stroke() else {
            self.history.end_empty_stroke();
            self.clear_active_stroke_state();
            return;
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("history commit encoder"),
            });
        let layer_index = self
            .layers
            .iter()
            .position(|layer| layer.id == active_stroke.layer_id)
            .expect("active stroke layer must exist");
        if active_stroke.render_path() == StrokeRenderPath::Mask {
            // Tiles inside the bounds that no stamp touched have no mask coverage. Erasing
            // cannot change a coordinate without a tile.
            let spans: Vec<_> = tile_spans(rect)
                .filter(|span| {
                    touched_tiles.contains(&span.coord)
                        && (active_stroke.tool == PaintTool::Brush
                            || self.layers[layer_index].tiles.contains(span.coord))
                })
                .collect();
            for span in &spans {
                self.prepare_tile_for_write(&mut encoder, layer_index, span.coord);
            }
            let commit_pipeline = if active_stroke.tool == PaintTool::Brush {
                &self.resources.brush_commit_pipeline
            } else {
                &self.resources.eraser_commit_pipeline
            };
            for span in spans {
                let tile = self.layers[layer_index]
                    .tiles
                    .get(span.coord)
                    .expect("committed tile was prepared");
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("stroke commit pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &tile.view,
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
                let local = span.local_rect();
                pass.set_pipeline(commit_pipeline);
                pass.set_bind_group(0, &self.resources.stroke_commit_bind_group, &[]);
                pass.set_bind_group(
                    1,
                    &self.resources.tile_slot(span.coord).origin_bind_group,
                    &[],
                );
                pass.set_scissor_rect(local.x, local.y, local.width, local.height);
                pass.draw(0..3, 0..1);
            }

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
        let replaced = std::mem::take(&mut self.stroke_original_tiles)
            .into_iter()
            .collect();
        self.history.commit_stroke(active_stroke.layer_id, replaced);
        self.queue.submit(std::iter::once(encoder.finish()));
        self.mark_layer_changed(active_stroke.layer_id);
        self.clear_active_stroke_state();
    }

    /// Replaces the layer's tile at `coord` with a writable copy the first time the active
    /// stroke touches it, keeping the original for history. Missing tiles are allocated.
    fn prepare_tile_for_write(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer_index: usize,
        coord: TileCoord,
    ) {
        if self.stroke_original_tiles.contains_key(&coord) {
            return;
        }
        let tile = self.resources.create_tile(&self.device, coord);
        let original = self.layers[layer_index].tiles.remove(coord);
        if let Some(original) = &original {
            copy_texture_region(
                encoder,
                &original.texture,
                [0, 0],
                &tile.texture,
                [0, 0],
                [TILE_SIZE; 2],
            );
        }
        self.layers[layer_index].tiles.insert(coord, tile);
        self.stroke_original_tiles.insert(coord, original);
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
                self.apply_history_structure_effect(effect);
                true
            }
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("undo layer must exist");
                self.history
                    .undo_stroke(&mut self.layers[layer_index].tiles);
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
                self.apply_history_structure_effect(effect);
                true
            }
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("redo layer must exist");
                self.history
                    .redo_stroke(&mut self.layers[layer_index].tiles);
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
            active_stroke.stamp_point(point),
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
            active_stroke.stamp_point(from),
            active_stroke.stamp_point(to),
            active_stroke.color,
            effective_spacing(active_stroke.tool, spacing),
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn update_stroke_preview(
        &mut self,
        committed_tip: StrokePoint,
        preview_points: &[StrokePoint],
        spacing: BrushSpacing,
    ) -> bool {
        let Some(active_stroke) = self
            .active_stroke
            .filter(|stroke| stroke.render_path() == StrokeRenderPath::Mask)
        else {
            return false;
        };
        let committed_tip = active_stroke.stamp_point(committed_tip);
        self.pending_preview_stamps = Some(
            self.stamp_queue.preview_stamps(
                committed_tip,
                preview_points
                    .iter()
                    .copied()
                    .map(|point| active_stroke.stamp_point(point)),
                active_stroke.color,
                effective_spacing(active_stroke.tool, spacing),
                self.document_size[0],
                self.document_size[1],
            ),
        );
        true
    }

    pub fn render_to_view(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        brush_cursor: Option<BrushCursor>,
    ) {
        self.render_to_view_with_backdrop(encoder, view, brush_cursor, false);
    }

    /// Renders to `view`, optionally retaining the cursor-free result in [`Self::backdrop_view`].
    /// Retaining the backdrop adds a full-surface blit and is intended for compositors that cannot
    /// sample their presentation surface directly.
    pub fn render_to_view_with_backdrop(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        brush_cursor: Option<BrushCursor>,
        retain_backdrop: bool,
    ) {
        self.flush_stamps(encoder);
        self.flush_stroke_preview(encoder);
        // Keep the active layer's thumbnail dirty until the stroke is committed. Updating it for
        // every dab adds a render pass to the latency-sensitive painting path.
        self.render_layer_previews(encoder);
        self.write_view_uniform();
        self.ensure_clipped_layer_bind_groups();
        if let Some(cursor) = brush_cursor {
            self.write_brush_cursor(cursor);
        }

        // The cursor and external backdrop consumers sample the completed canvas, so those frames
        // compose offscreen first.
        let use_backdrop = brush_cursor.is_some() || retain_backdrop;
        let canvas_view = if use_backdrop {
            &self.resources.backdrop_view
        } else {
            view
        };
        let canvas_rect = visible_canvas_rect(
            self.view.snapshot(),
            self.document_size,
            self.surface_size(),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas background pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: canvas_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.workspace_background_color),
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
                pass.set_bind_group(0, self.resources.clipping_group_bind_group(), &[]);
                pass.draw(0..3, 0..1);
            }
        }

        if canvas_rect.is_some() {
            let view = self.view.snapshot();
            let surface_size = self.surface_size();
            let mask_stroke = self
                .active_stroke
                .filter(|stroke| stroke.render_path() == StrokeRenderPath::Mask);
            // Stroke previews may draw where the layer has no tile yet.
            let stroke_coords: BTreeSet<_> = mask_stroke
                .and_then(|_| {
                    [self.stamp_queue.dirty_rect(), self.rendered_preview_rect]
                        .into_iter()
                        .flatten()
                        .reduce(TextureRect::union)
                })
                .into_iter()
                .flat_map(tile_spans)
                .map(|span| span.coord)
                .collect();
            let preview_tool = |layer: &PaintLayer| {
                mask_stroke
                    .filter(|stroke| stroke.layer_id == layer.id)
                    .map(|stroke| stroke.tool)
            };
            let tile_bind_group = |layer, coord| {
                layer_tile_bind_group(
                    &self.resources,
                    layer,
                    coord,
                    preview_tool(layer).is_some() && stroke_coords.contains(&coord),
                )
            };
            let window_tile_rect = |coord: TileCoord| {
                document_rect_in_window(
                    view,
                    tile_document_rect(coord, self.document_size),
                    surface_size,
                )
            };

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
                let clips: Vec<_> = self.layers[base_index + 1..group_end]
                    .iter()
                    .filter(|layer| layer.visible)
                    .collect();
                let mut base_coords: BTreeSet<_> = base.tiles.coords().collect();
                if preview_tool(base).is_some() {
                    base_coords.extend(stroke_coords.iter().copied());
                }

                if !clips.is_empty() {
                    // Clipped layers must be composed with their base before the group is put over
                    // the canvas. Applying each masked layer directly over the canvas multiplies
                    // the base alpha twice and leaves translucent base color showing through.
                    for coord in base_coords {
                        let Some(scissor) = window_tile_rect(coord) else {
                            continue;
                        };
                        let base_tile = tile_bind_group(base, coord)
                            .expect("base coordinates have a tile or a preview");
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("clipping group pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &self.resources.scratch_tile_view,
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
                        match preview_tool(base) {
                            Some(PaintTool::Brush) => {
                                pass.set_pipeline(&self.resources.group_brush_preview_pipeline);
                                pass.set_bind_group(
                                    0,
                                    self.resources.stroke_preview_bind_group(),
                                    &[],
                                );
                            }
                            Some(PaintTool::Eraser) => {
                                pass.set_pipeline(&self.resources.group_eraser_preview_pipeline);
                                pass.set_bind_group(
                                    0,
                                    self.resources.stroke_preview_bind_group(),
                                    &[],
                                );
                            }
                            Some(PaintTool::Smudge) | None => {
                                pass.set_pipeline(&self.resources.merge_pipeline);
                                pass.set_bind_group(0, &base.blit_bind_group, &[]);
                            }
                        }
                        pass.set_bind_group(1, base_tile, &[]);
                        if preview_tool(base).is_some() {
                            // The base preview ignores group 2, but its layout declares it.
                            pass.set_bind_group(2, base_tile, &[]);
                        }
                        pass.draw(0..3, 0..1);

                        for layer in &clips {
                            let Some(layer_tile) = tile_bind_group(layer, coord) else {
                                continue;
                            };
                            match preview_tool(layer) {
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
                            pass.set_bind_group(1, layer_tile, &[]);
                            pass.set_bind_group(2, base_tile, &[]);
                            pass.draw(0..3, 0..1);
                        }
                        drop(pass);

                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("clipping group blit pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: canvas_view,
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
                        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                        pass.set_pipeline(&self.resources.layer_pipeline);
                        pass.set_bind_group(0, self.resources.clipping_group_bind_group(), &[]);
                        pass.set_bind_group(
                            1,
                            &self.resources.tile_slot(coord).scratch_tile_bind_group,
                            &[],
                        );
                        pass.draw(0..3, 0..1);
                    }
                } else {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("layer blit pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: canvas_view,
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
                    match preview_tool(base) {
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
                    for coord in base_coords {
                        let Some(scissor) = window_tile_rect(coord) else {
                            continue;
                        };
                        let tile = tile_bind_group(base, coord)
                            .expect("base coordinates have a tile or a preview");
                        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                        pass.set_bind_group(1, tile, &[]);
                        if preview_tool(base).is_some() {
                            // The unclipped preview ignores group 2, but its layout declares it.
                            pass.set_bind_group(2, tile, &[]);
                        }
                        pass.draw(0..3, 0..1);
                    }
                }
                base_index = group_end;
            }
        }

        if use_backdrop {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("canvas screen pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
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
                pass.set_pipeline(&self.resources.screen_pipeline);
                pass.set_bind_group(0, &self.resources.cursor_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if brush_cursor.is_some() {
                self.draw_brush_cursor(encoder, view);
            }
        }
    }

    /// Draws the cursor over the composed frame. `frame` must be the COPY_SRC texture backing
    /// `view`, with the same size and format as the canvas surface. Capture it before drawing:
    /// sampling `view` while it is a render attachment would be invalid and would miss the UI.
    pub fn render_brush_cursor_over_view(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Texture,
        view: &wgpu::TextureView,
        cursor: BrushCursor,
    ) {
        let surface_size = self.surface_size();
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: frame,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.resources.backdrop_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: surface_size[0],
                height: surface_size[1],
                depth_or_array_layers: 1,
            },
        );
        self.write_brush_cursor(cursor);
        self.draw_brush_cursor(encoder, view);
    }

    fn draw_brush_cursor(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
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

    pub fn backdrop_view(&self) -> &wgpu::TextureView {
        &self.resources.backdrop_view
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
                &self.device,
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
                pass.set_bind_group(0, &self.resources.thumbnail_bind_group, &[]);
                for (_, tile) in layer.tiles.iter() {
                    pass.set_bind_group(1, &tile.bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
            layer.preview_dirty = false;
        }
    }

    fn flush_all_stamps(&mut self) {
        while self.stamp_queue.has_pending() {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("stroke flush encoder"),
                });
            self.flush_stamps(&mut encoder);
            self.queue.submit(std::iter::once(encoder.finish()));
        }
    }

    fn flush_stamps(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut raw = self.stamp_queue.drain_raw(
            self.document_size[0],
            self.document_size[1],
            MAX_STAMPS_PER_FRAME,
        );
        let count = raw.len();
        if count == 0 {
            return;
        }

        let active_stroke = self.active_stroke.expect("stamp requires active stroke");
        if active_stroke.render_path() == StrokeRenderPath::DirectSmudge {
            for stamp in &mut raw {
                stamp.scale_source_offset(active_stroke.opacity);
            }
        }
        self.queue
            .write_buffer(&self.resources.stamp_buffer, 0, bytemuck::cast_slice(&raw));
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

        debug_assert!(matches!(
            active_stroke.tool,
            PaintTool::Brush | PaintTool::Eraser
        ));
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
        pass.set_pipeline(&self.resources.mask_pipeline);
        pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    fn flush_stroke_preview(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(stamps) = self.pending_preview_stamps.take() else {
            return;
        };
        let previous_rect = self.rendered_preview_rect.take();
        let next_rect = stamps
            .iter()
            .copied()
            .map(StampRaw::target_rect)
            .reduce(TextureRect::union);
        if next_rect.is_some() {
            self.queue.write_buffer(
                &self.resources.preview_stamp_buffer,
                0,
                bytemuck::cast_slice(&stamps),
            );
        }
        if previous_rect.is_none() && next_rect.is_none() {
            return;
        }
        self.rendered_preview_rect = next_rect;

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stroke preview pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.resources.preview_mask_view,
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
        if let Some(rect) = previous_rect {
            pass.set_pipeline(&self.resources.mask_clear_pipeline);
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.draw(0..3, 0..1);
        }
        if let Some(rect) = next_rect {
            pass.set_pipeline(&self.resources.mask_pipeline);
            pass.set_bind_group(0, &self.resources.preview_stamp_bind_group, &[]);
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.draw(0..6, 0..stamps.len() as u32);
        }
    }

    fn flush_smudge_stamps(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer_index: usize,
        stamps: &[StampRaw],
    ) {
        for (index, stamp) in stamps.iter().copied().enumerate() {
            let spans: Vec<_> = tile_spans(stamp.target_rect()).collect();
            for span in &spans {
                self.prepare_tile_for_write(encoder, layer_index, span.coord);
            }
            for span in &spans {
                let tile = self.layers[layer_index]
                    .tiles
                    .get(span.coord)
                    .expect("smudged tile was prepared");
                let local = span.local_rect();
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("smudge pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &tile.view,
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
                pass.set_bind_group(0, &self.resources.smudge_snapshot().bind_group, &[]);
                pass.set_bind_group(
                    1,
                    &self.resources.tile_slot(span.coord).origin_bind_group,
                    &[],
                );
                pass.set_scissor_rect(local.x, local.y, local.width, local.height);
                pass.draw(0..6, index as u32..index as u32 + 1);
            }
            // The next dab must sample the result of this one. Copy only after every span is
            // drawn, so one dab never samples its own output across a tile edge.
            for span in &spans {
                let tile = self.layers[layer_index]
                    .tiles
                    .get(span.coord)
                    .expect("smudged tile was prepared");
                let local = span.local_rect();
                copy_texture_region(
                    encoder,
                    &tile.texture,
                    [local.x, local.y],
                    &self.resources.smudge_snapshot().texture,
                    [span.document_rect.x, span.document_rect.y],
                    [local.width, local.height],
                );
            }
        }
    }

    fn clear_active_stroke_state(&mut self) {
        self.active_stroke = None;
        self.resources.end_smudge();
        self.pending_preview_stamps = self.rendered_preview_rect.is_some().then(Vec::new);
        self.resources.clear_stroke_preview();
    }

    fn resize_document_resources(&mut self, size: [u32; 2]) {
        if size == self.document_size {
            return;
        }
        self.resources
            .resize_document(&self.device, &self.queue, size);
        self.history = PaintHistory::new();
        self.pending_preview_stamps = None;
        self.rendered_preview_rect = None;
        self.document_size = size;
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
        self.content_bounds_cache = None;
        let version = u64::from(dirty);
        self.document_generation = version;
        self.metadata_version = version;
        self.layer_versions = self
            .layers
            .iter()
            .map(|layer| (layer.id, version))
            .collect();
    }

    fn advance_document_generation(&mut self) -> u64 {
        self.document_generation = self.document_generation.saturating_add(1);
        self.document_generation
    }

    fn mark_entire_document_changed(&mut self) {
        self.content_bounds_cache = None;
        let version = self.advance_document_generation();
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
        self.metadata_version = self.advance_document_generation();
        self.clipping_bind_groups_dirty = true;
    }

    fn mark_layer_changed(&mut self, id: LayerId) {
        self.content_bounds_cache = None;
        if let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) {
            layer.preview_dirty = true;
        }
        let version = self.advance_document_generation();
        self.layer_versions.insert(id, version);
    }

    fn apply_history_structure_effect(&mut self, effect: StructureEffect) {
        self.clipping_bind_groups_dirty = true;
        if let StructureEffect::CanvasResized { size } = effect {
            self.resources
                .resize_document(&self.device, &self.queue, size);
            self.document_size = size;
            self.fit_to_screen();
        }
        let version = self.advance_document_generation();
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
            self.queue.write_buffer(
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
        self.queue.write_buffer(
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
        self.queue.write_buffer(
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

fn visible_canvas_rect(
    view: PaintViewSnapshot,
    document_size: [u32; 2],
    surface_size: [u32; 2],
) -> Option<TextureRect> {
    document_rect_in_window(
        view,
        TextureRect {
            x: 0,
            y: 0,
            width: document_size[0],
            height: document_size[1],
        },
        surface_size,
    )
}

/// The surface pixels covered by a document rectangle, clipped to the surface.
fn document_rect_in_window(
    view: PaintViewSnapshot,
    rect: TextureRect,
    surface_size: [u32; 2],
) -> Option<TextureRect> {
    let left = rect.x as f32;
    let top = rect.y as f32;
    let right = (rect.x + rect.width) as f32;
    let bottom = (rect.y + rect.height) as f32;
    let corners = [
        view.document_to_window([left, top]),
        view.document_to_window([right, top]),
        view.document_to_window([left, bottom]),
        view.document_to_window([right, bottom]),
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

fn validate_brush_stamp(stamp: &image::RgbaImage, max_dimension: u32) -> Result<(), String> {
    let (width, height) = stamp.dimensions();
    if width == 0 || height == 0 {
        return Err("brush stamp dimensions must be non-zero".to_owned());
    }
    if width > max_dimension || height > max_dimension {
        return Err(format!(
            "brush stamp width and height cannot exceed {max_dimension} pixels"
        ));
    }
    Ok(())
}

/// Resolves a clipped layer to the nearest non-clipped layer below it.
fn clipping_base_index(clipped: &[bool], layer_index: usize) -> Option<usize> {
    if !clipped.get(layer_index).copied().unwrap_or(false) {
        return None;
    }
    (0..layer_index).rev().find(|index| !clipped[*index])
}

fn next_layer_number(layers: &[LayerInfo]) -> u64 {
    layers
        .iter()
        .filter_map(|layer| layer.name.strip_prefix("Layer ")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1)
}

/// A layer's tile at `coord`, or a transparent stand-in when a stroke preview may draw there.
fn layer_tile_bind_group<'a>(
    resources: &'a RenderResources,
    layer: &'a PaintLayer,
    coord: TileCoord,
    empty_when_missing: bool,
) -> Option<&'a wgpu::BindGroup> {
    match layer.tiles.get(coord) {
        Some(tile) => Some(&tile.bind_group),
        None => empty_when_missing.then(|| &resources.tile_slot(coord).empty_tile_bind_group),
    }
}

/// The document pixels covered by `points` grown by `padding`, clipped to the document.
fn document_rect_from_points(
    points: &[[f32; 2]],
    padding: f32,
    document_size: [u32; 2],
) -> Option<TextureRect> {
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let min = points.iter().fold([f32::INFINITY; 2], |min, point| {
        [min[0].min(point[0]), min[1].min(point[1])]
    });
    let max = points.iter().fold([f32::NEG_INFINITY; 2], |max, point| {
        [max[0].max(point[0]), max[1].max(point[1])]
    });
    let clamp = |value: f32, limit: u32| value.clamp(0.0, limit as f32) as u32;
    let left = clamp((min[0] - padding).floor(), document_size[0]);
    let top = clamp((min[1] - padding).floor(), document_size[1]);
    let right = clamp((max[0] + padding).ceil(), document_size[0]);
    let bottom = clamp((max[1] + padding).ceil(), document_size[1]);
    (right > left && bottom > top).then_some(TextureRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn alpha_content_bounds(image: &image::RgbaImage) -> Option<LayerContentBounds> {
    let mut min = [image.width(), image.height()];
    let mut max = [0, 0];
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        min[0] = min[0].min(x);
        min[1] = min[1].min(y);
        max[0] = max[0].max(x + 1);
        max[1] = max[1].max(y + 1);
    }
    (min[0] < max[0] && min[1] < max[1]).then_some(LayerContentBounds {
        min: [min[0] as f32, min[1] as f32],
        max: [max[0] as f32, max[1] as f32],
    })
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
            LayerInfo {
                id: LayerId(1),
                name: "Layer 4".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
            },
            LayerInfo {
                id: LayerId(2),
                name: "Reference".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
            },
        ];
        assert_eq!(next_layer_number(&layers), 5);
    }

    #[test]
    fn canvas_document_rejects_invalid_layer_metadata() {
        let mut document = CanvasDocument {
            size: [20, 30],
            background: [255; 3],
            selected_layer: LayerId(1),
            layers: vec![LayerInfo {
                id: LayerId(1),
                name: "Paint".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
            }],
        };
        assert!(document.validate().is_ok());

        document.layers[0].opacity = 101;
        assert!(document.validate().is_err());
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
        let stroke = ActiveStroke::new(LayerId(7), PaintTool::Brush, [0.8, 0.4, 0.2, 0.5], 0.75);

        assert_eq!(stroke.layer_id, LayerId(7));
        assert_eq!(stroke.tool, PaintTool::Brush);
        assert_eq!(stroke.color, [0.4, 0.2, 0.1, 0.5]);
        assert_eq!(stroke.opacity, 0.75);
    }

    #[test]
    fn tool_opacity_scales_mask_strength_but_not_smudge_dab_strength() {
        let point = StrokePoint {
            x: 0.0,
            y: 0.0,
            radius: 10.0,
            opacity: 0.8,
        };
        let color = [0.0, 0.0, 0.0, 1.0];

        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Brush, color, 0.25)
                .stamp_point(point)
                .opacity,
            0.2
        );
        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Smudge, color, 0.25)
                .stamp_point(point)
                .opacity,
            0.8
        );
    }

    #[test]
    fn alpha_bounds_wrap_visible_pixels() {
        let mut image = image::RgbaImage::new(8, 6);
        assert_eq!(alpha_content_bounds(&image), None);
        image.put_pixel(2, 4, image::Rgba([1, 2, 3, 1]));
        image.put_pixel(6, 1, image::Rgba([1, 2, 3, 255]));
        assert_eq!(
            alpha_content_bounds(&image),
            Some(LayerContentBounds {
                min: [2.0, 1.0],
                max: [7.0, 5.0],
            })
        );
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
    fn brush_stamps_must_fit_gpu_limits() {
        assert!(validate_brush_stamp(&image::RgbaImage::new(1, 1), 8).is_ok());
        assert!(validate_brush_stamp(&image::RgbaImage::new(0, 1), 8).is_err());
        assert!(validate_brush_stamp(&image::RgbaImage::new(9, 1), 8).is_err());
    }

    #[test]
    fn layer_transform_maps_destination_back_to_source() {
        fn map(uniform: TransformUniform, point: [f32; 2]) -> [f32; 2] {
            [
                uniform.source_from_destination_x[0] * point[0]
                    + uniform.source_from_destination_x[1] * point[1]
                    + uniform.source_from_destination_x[2],
                uniform.source_from_destination_y[0] * point[0]
                    + uniform.source_from_destination_y[1] * point[1]
                    + uniform.source_from_destination_y[2],
            ]
        }

        let identity = LayerTransform::default().uniform([50.0, 40.0]);
        assert_eq!(map(identity, [20.5, 30.5]), [20.5, 30.5]);

        let translated = LayerTransform {
            translation: [10.0, -5.0],
            ..Default::default()
        }
        .uniform([50.0, 40.0]);
        assert_eq!(map(translated, [30.5, 25.5]), [20.5, 30.5]);

        let scaled = LayerTransform {
            scale: [2.0, 4.0],
            ..Default::default()
        }
        .uniform([50.0, 40.0]);
        assert_eq!(map(scaled, [70.0, 80.0]), [60.0, 50.0]);

        let rotated = LayerTransform {
            rotation: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        }
        .uniform([50.0, 40.0]);
        let source = map(rotated, [50.0, 50.0]);
        assert!((source[0] - 60.0).abs() < 0.0001);
        assert!((source[1] - 40.0).abs() < 0.0001);
    }

    #[test]
    fn layer_transform_apply_inverts_the_shader_mapping() {
        let transform = LayerTransform {
            translation: [30.0, -12.0],
            scale: [0.5, 2.0],
            rotation: 0.7,
        };
        let pivot = [200.0, 150.0];
        let uniform = transform.uniform(pivot);
        for point in [[0.0, 0.0], [640.0, 20.0], [205.5, 149.0]] {
            let destination = transform.map_point(pivot, point);
            let source = [
                uniform.source_from_destination_x[0] * destination[0]
                    + uniform.source_from_destination_x[1] * destination[1]
                    + uniform.source_from_destination_x[2],
                uniform.source_from_destination_y[0] * destination[0]
                    + uniform.source_from_destination_y[1] * destination[1]
                    + uniform.source_from_destination_y[2],
            ];
            assert!((source[0] - point[0]).abs() < 1e-3 && (source[1] - point[1]).abs() < 1e-3);
        }
    }

    #[test]
    fn transformed_points_cover_padded_document_pixels() {
        assert_eq!(
            document_rect_from_points(&[[10.2, 5.0], [30.5, 40.9]], 1.0, [100, 100]),
            Some(TextureRect {
                x: 9,
                y: 4,
                width: 23,
                height: 38,
            })
        );
        assert_eq!(
            document_rect_from_points(&[[-50.0, -50.0], [-10.0, -5.0]], 1.0, [100, 100]),
            None
        );
    }

    #[test]
    fn content_bounds_cover_whole_pixels() {
        let bounds = LayerContentBounds {
            min: [3.0, 4.5],
            max: [10.0, 11.2],
        };
        assert_eq!(
            bounds.pixel_rect(),
            TextureRect {
                x: 3,
                y: 4,
                width: 7,
                height: 8,
            }
        );
    }

    #[test]
    fn invalid_layer_transforms_are_rejected_and_scale_is_clamped() {
        assert!(
            LayerTransform {
                scale: [0.0, 1.0],
                ..Default::default()
            }
            .normalized()
            .is_none()
        );
        assert!(
            LayerTransform {
                rotation: f32::NAN,
                ..Default::default()
            }
            .normalized()
            .is_none()
        );
        assert_eq!(
            LayerTransform {
                scale: [1_000.0, 0.001],
                ..Default::default()
            }
            .normalized()
            .unwrap()
            .scale,
            [LayerTransform::MAX_SCALE, LayerTransform::MIN_SCALE]
        );
    }

    #[test]
    fn brush_and_eraser_use_masks_while_smudge_is_direct() {
        let color = [0.0, 0.0, 0.0, 1.0];

        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Brush, color, 1.0).render_path(),
            StrokeRenderPath::Mask
        );
        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Eraser, color, 1.0).render_path(),
            StrokeRenderPath::Mask
        );
        assert_eq!(
            ActiveStroke::new(LayerId(1), PaintTool::Smudge, color, 1.0).render_path(),
            StrokeRenderPath::DirectSmudge
        );
    }
}
