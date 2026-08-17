mod brush_preview;
mod color_picker;
mod gallery;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    artwork::ArtworkSummary,
    config::{AppConfig, BrushCatalog, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PaintTool, PressureSettings},
    renderer::{
        CanvasSizeConstraints, DEFAULT_CANVAS_SIZE, DropEdge, LayerContentBounds, LayerId,
        LayerResourceId, LayerSnapshot, LayerTransform, PaintRenderer, PaintViewSnapshot,
        merge_down_target_index,
    },
};

use super::{
    autosave::SaveStatus,
    command::AppCommand,
    input::EditorTool,
    references::{ReferenceId, ReferenceImage},
};

const TOOL_RAIL_THICKNESS: f32 = 42.0;
const LAYER_PANEL_WIDTH: f32 = 300.0;
const LAYER_LIST_MAX_HEIGHT: f32 = 440.0;

#[cfg(target_os = "macos")]
const SAVE_SHORTCUT: &str = "⌘-S";
#[cfg(not(target_os = "macos"))]
const SAVE_SHORTCUT: &str = "Ctrl-S";
#[cfg(target_os = "macos")]
const EXPORT_SHORTCUT: &str = "⌘-Shift-E";
#[cfg(not(target_os = "macos"))]
const EXPORT_SHORTCUT: &str = "Ctrl-Shift-E";
#[cfg(target_os = "macos")]
const GALLERY_SHORTCUT: &str = "⌘-G";
#[cfg(not(target_os = "macos"))]
const GALLERY_SHORTCUT: &str = "Ctrl-G";
#[cfg(target_os = "macos")]
const UNDO_SHORTCUT: &str = "⌘-Z";
#[cfg(not(target_os = "macos"))]
const UNDO_SHORTCUT: &str = "Ctrl-Z";
#[cfg(target_os = "macos")]
const REDO_SHORTCUT: &str = "⌘-Shift-Z";
#[cfg(not(target_os = "macos"))]
const REDO_SHORTCUT: &str = "Ctrl-Y / Ctrl-Shift-Z";
#[cfg(target_os = "macos")]
const ROTATE_SHORTCUT: &str = "⌘-Option-← / →";
#[cfg(not(target_os = "macos"))]
const ROTATE_SHORTCUT: &str = "Ctrl-Alt-← / →";
#[cfg(target_os = "macos")]
const FLIP_SHORTCUT: &str = "⌘-Option-H / V";
#[cfg(not(target_os = "macos"))]
const FLIP_SHORTCUT: &str = "Ctrl-Alt-H / V";
#[cfg(target_os = "macos")]
const CROP_SHORTCUT: &str = "⌘-Option-C";
#[cfg(not(target_os = "macos"))]
const CROP_SHORTCUT: &str = "Ctrl-Alt-C";

#[derive(Clone, Copy)]
pub(crate) struct BrushResizeLabel {
    pub(crate) center: [f32; 2],
    pub(crate) outline_half_width: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct EyedropperIndicator {
    pub(crate) center: [f32; 2],
    pub(crate) color: egui::Color32,
}

pub(crate) struct EditorUiState<'a> {
    pub(crate) layers: &'a LayerSnapshot,
    pub(crate) tool: EditorTool,
    pub(crate) layer_transform: Option<LayerTransform>,
    pub(crate) layer_content_bounds: Option<LayerContentBounds>,
    pub(crate) brush_resize_label: Option<BrushResizeLabel>,
    pub(crate) eyedropper_indicator: Option<EyedropperIndicator>,
    pub(crate) save_status: SaveStatus,
    pub(crate) pending_navigation: Option<&'a str>,
    pub(crate) brush_import_dialog_delay: Option<Duration>,
    pub(crate) reference_import_dialog_delay: Option<Duration>,
    pub(crate) reference_load_dialog_delay: Option<Duration>,
    pub(crate) references: &'a [ReferenceImage],
    pub(crate) workspace_view: PaintViewSnapshot,
}

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    tool_brushes: [String; 3],
    tool_sizes: [f32; 3],
    brushes: Vec<crate::config::BrushSummary>,
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    commands: Vec<AppCommand>,
    message_dialog: Option<MessageDialog>,
    shortcuts_dialog_open: bool,
    background_edit_start: Option<[u8; 3]>,
    layer_name_edit: Option<LayerNameEdit>,
    layer_opacity_edit: Option<LayerOpacityEdit>,
    layer_opacity_open: Option<LayerId>,
    layer_thumbnails: Vec<LayerThumbnail>,
    reference_textures: Vec<ReferenceTexture>,
    selected_reference: Option<ReferenceId>,
    reference_transform_edit: Option<ReferenceTransformEdit>,
    reference_hit_rects: Vec<egui::Rect>,
    pointer_over_reference: bool,
    pointer_over_selected_reference: bool,
    brush_previews: Vec<(String, egui::TextureHandle)>,
    failed_brush_previews: Vec<String>,
    color_window_open: bool,
    layers_window_open: bool,
    canvas_size_constraints: CanvasSizeConstraints,
    new_artwork_dialog: Option<NewArtworkDialog>,
    canvas_crop: Option<CanvasCrop>,
    layer_transform_drag: Option<LayerTransformDrag>,
    gallery: gallery::GalleryUi,
}

struct MessageDialog {
    title: &'static str,
    message: String,
}

impl MessageDialog {
    fn error(message: impl Into<String>, details: impl std::fmt::Display) -> Self {
        let message = message.into();
        log::error!("{message}: {details}");
        Self {
            title: "Something went wrong",
            message,
        }
    }

    fn success(message: impl Into<String>) -> Self {
        Self {
            title: "Done",
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayerPreviewKey {
    layer_id: LayerId,
    resource_id: LayerResourceId,
}

struct LayerThumbnail {
    key: LayerPreviewKey,
    texture_id: egui::TextureId,
}

struct ReferenceTexture {
    id: ReferenceId,
    resource_version: u64,
    texture: egui::TextureHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceDrag {
    Move,
    Resize,
}

#[derive(Clone, Copy)]
struct ReferenceTransformEdit {
    id: ReferenceId,
    drag: ReferenceDrag,
    position: [f32; 2],
    size: [f32; 2],
}

struct LayerNameEdit {
    id: LayerId,
    name: String,
}

struct LayerOpacityEdit {
    id: LayerId,
    before: u8,
    current: u8,
}

struct NewArtworkDialog {
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CanvasCropRect {
    min: [f32; 2],
    max: [f32; 2],
}

impl CanvasCropRect {
    fn from_size(size: [u32; 2]) -> Self {
        Self {
            min: [0.0, 0.0],
            max: [size[0] as f32, size[1] as f32],
        }
    }

    fn contains(self, point: [f32; 2]) -> bool {
        point[0] >= self.min[0]
            && point[0] <= self.max[0]
            && point[1] >= self.min[1]
            && point[1] <= self.max[1]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasCropHandle {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy)]
struct CanvasCropDrag {
    handle: CanvasCropHandle,
    start_rect: CanvasCropRect,
    start_pointer: [f32; 2],
}

struct CanvasCrop {
    rect: CanvasCropRect,
    drag: Option<CanvasCropDrag>,
    restore_color_window: bool,
    restore_layers_window: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerTransformHandle {
    Move,
    Scale(CanvasCropHandle),
    Rotate,
}

#[derive(Clone, Copy)]
struct LayerTransformDrag {
    handle: LayerTransformHandle,
    start_transform: LayerTransform,
    start_pointer: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CanvasCropRequest {
    size: [u32; 2],
    origin: [i32; 2],
}

impl GuiLayer {
    pub fn new(
        window: &Window,
        paint: &PaintRenderer,
        config: &AppConfig,
        brush_preset: &LoadedBrushPreset,
        catalog: BrushCatalog,
        load_error: Option<String>,
    ) -> Self {
        let context = egui::Context::default();
        install_fonts(&context);
        install_rounded_ui_style(&context);
        egui_extras::install_image_loaders(&context);
        let state = EguiWinitState::new(
            context.clone(),
            ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(paint.device().limits().max_texture_dimension_2d as usize),
        );
        let renderer = EguiRenderer::new(
            paint.device(),
            paint.surface_format(),
            RendererOptions::default(),
        );
        let preset = &brush_preset.preset;
        let message_dialog = load_error.map(|error| {
            MessageDialog::error(
                "Chromazen couldn’t load your settings and is using the defaults.",
                error,
            )
        });

        Self {
            context,
            state,
            renderer,
            brush: brush_settings_from_config(&config.brush, brush_preset),
            tool_brushes: [
                brush_preset.id.clone(),
                config.eraser_brush.clone(),
                config.smudge_brush.clone(),
            ],
            tool_sizes: [
                config.size_for_tool(PaintTool::Brush),
                config.size_for_tool(PaintTool::Eraser),
                config.size_for_tool(PaintTool::Smudge),
            ],
            brushes: catalog.brushes,
            size_range: preset.size.min..=preset.size.max,
            default_size: preset.size.default,
            commands: Vec::new(),
            message_dialog,
            shortcuts_dialog_open: false,
            background_edit_start: None,
            layer_name_edit: None,
            layer_opacity_edit: None,
            layer_opacity_open: None,
            layer_thumbnails: Vec::new(),
            reference_textures: Vec::new(),
            selected_reference: None,
            reference_transform_edit: None,
            reference_hit_rects: Vec::new(),
            pointer_over_reference: false,
            pointer_over_selected_reference: false,
            brush_previews: Vec::new(),
            failed_brush_previews: Vec::new(),
            color_window_open: false,
            layers_window_open: false,
            canvas_size_constraints: paint.canvas_size_constraints(),
            new_artwork_dialog: None,
            canvas_crop: None,
            layer_transform_drag: None,
            gallery: gallery::GalleryUi::default(),
        }
    }

    pub(crate) fn sync_layer_thumbnails(&mut self, paint: &PaintRenderer) {
        let current_keys: Vec<_> = paint
            .layer_preview_views()
            .map(|(layer_id, resource_id, _)| LayerPreviewKey {
                layer_id,
                resource_id,
            })
            .collect();

        let mut index = 0;
        while index < self.layer_thumbnails.len() {
            if layer_preview_is_current(&current_keys, self.layer_thumbnails[index].key) {
                index += 1;
            } else {
                let thumbnail = self.layer_thumbnails.remove(index);
                self.renderer.free_texture(&thumbnail.texture_id);
            }
        }

        for (layer_id, resource_id, view) in paint.layer_preview_views() {
            let key = LayerPreviewKey {
                layer_id,
                resource_id,
            };
            if self
                .layer_thumbnails
                .iter()
                .all(|thumbnail| thumbnail.key != key)
            {
                let texture_id = self.renderer.register_native_texture(
                    paint.device(),
                    view,
                    wgpu::FilterMode::Linear,
                );
                self.layer_thumbnails
                    .push(LayerThumbnail { key, texture_id });
            }
        }
    }

    fn sync_reference_textures(&mut self, references: &[ReferenceImage]) {
        self.reference_textures.retain(|cached| {
            references.iter().any(|reference| {
                reference.id == cached.id && reference.resource_version == cached.resource_version
            })
        });
        for reference in references {
            if self.reference_texture(reference.id).is_some() {
                continue;
            }
            let size = [
                reference.pixels.width() as usize,
                reference.pixels.height() as usize,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, reference.pixels.as_raw());
            let texture = self.context.load_texture(
                format!("reference {}", reference.id.0),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.reference_textures.push(ReferenceTexture {
                id: reference.id,
                resource_version: reference.resource_version,
                texture,
            });
        }
        if self
            .selected_reference
            .is_some_and(|id| references.iter().all(|reference| reference.id != id))
        {
            self.selected_reference = None;
        }
    }

    fn reference_texture(&self, id: ReferenceId) -> Option<egui::TextureId> {
        self.reference_textures
            .iter()
            .find(|cached| cached.id == id)
            .map(|cached| cached.texture.id())
    }

    fn show_workspace_references(
        &mut self,
        context: &egui::Context,
        references: &[ReferenceImage],
        view: PaintViewSnapshot,
        workspace_rect: egui::Rect,
    ) {
        self.reference_hit_rects.clear();
        self.pointer_over_reference = false;
        self.pointer_over_selected_reference = false;
        let pointer_position = context.pointer_latest_pos();
        let pixels_per_point = context.pixels_per_point();
        for reference in references.iter().filter(|reference| reference.visible) {
            let Some(texture_id) = self.reference_texture(reference.id) else {
                continue;
            };
            let window_position = view.workspace_to_window(reference.position);
            let position = egui::pos2(
                window_position[0] / pixels_per_point,
                window_position[1] / pixels_per_point,
            );
            let size = egui::vec2(
                reference.size[0] * view.zoom / pixels_per_point,
                reference.size[1] * view.zoom / pixels_per_point,
            );
            if size.x <= 0.5 || size.y <= 0.5 {
                continue;
            }

            let rect = egui::Rect::from_min_size(position, size);
            let pointer_over_image =
                pointer_over_visible_reference(pointer_position, rect, workspace_rect);
            let pointer_over_resize = !reference.locked
                && self.selected_reference == Some(reference.id)
                && pointer_position.is_some_and(|pointer| {
                    workspace_rect.contains(pointer)
                        && reference_resize_handle(rect).1.contains(pointer)
                });
            self.pointer_over_reference |= pointer_over_image || pointer_over_resize;
            if self.selected_reference == Some(reference.id) {
                self.pointer_over_selected_reference = pointer_over_image || pointer_over_resize;
            }
            let visible_rect = rect.intersect(workspace_rect);
            if !visible_rect.is_positive() {
                continue;
            }
            self.reference_hit_rects.push(visible_rect);
            if !reference.locked && self.selected_reference == Some(reference.id) {
                let resize_rect = reference_resize_handle(rect).1.intersect(workspace_rect);
                if resize_rect.is_positive() {
                    self.reference_hit_rects.push(resize_rect);
                }
            }

            let area = egui::Area::new(egui::Id::new(("reference", reference.id.0)))
                .order(egui::Order::Middle)
                .fixed_pos(visible_rect.min)
                .default_size(visible_rect.size())
                // Keep the canvas-relative transform authoritative. The area's interactive bounds
                // are only the visible image, while its painter clips the full image to the
                // workspace instead of moving the reference back inside the window.
                .constrain(false)
                .show(context, |ui| {
                    ui.shrink_clip_rect(workspace_rect);
                    let sense = if reference.locked {
                        egui::Sense::click()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let response = ui.allocate_rect(visible_rect, sense);
                    ui.painter().image(
                        texture_id,
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let resize = (!reference.locked
                        && self.selected_reference == Some(reference.id))
                    .then(|| {
                        let (_, handle_rect) = reference_resize_handle(rect);
                        ui.interact(
                            handle_rect.intersect(workspace_rect),
                            egui::Id::new(("reference resize", reference.id.0)),
                            egui::Sense::drag(),
                        )
                        .on_hover_and_drag_cursor(egui::CursorIcon::ResizeNwSe)
                    });
                    (response, resize)
                });
            let (response, resize) = area.inner;
            if response.clicked()
                || response.secondary_clicked()
                || resize.as_ref().is_some_and(egui::Response::drag_started)
            {
                self.selected_reference = Some(reference.id);
            }
            self.show_reference_context_menu(&response, reference);

            if !reference.locked {
                let active = if let Some(resize) = resize.as_ref().filter(|resize| resize.dragged())
                {
                    resize
                        .total_drag_delta()
                        .map(|delta| (ReferenceDrag::Resize, delta))
                } else if response.dragged() {
                    response
                        .total_drag_delta()
                        .map(|delta| (ReferenceDrag::Move, delta))
                } else {
                    None
                };
                let resize_started = resize.as_ref().is_some_and(egui::Response::drag_started);
                let started_drag = if resize_started {
                    Some(ReferenceDrag::Resize)
                } else if response.drag_started() {
                    Some(ReferenceDrag::Move)
                } else {
                    None
                };
                if let Some(drag) = started_drag {
                    self.reference_transform_edit = Some(ReferenceTransformEdit {
                        id: reference.id,
                        drag,
                        position: reference.position,
                        size: reference.size,
                    });
                }
                if let Some((drag, delta)) = active {
                    self.update_reference_drag(reference.id, drag, delta, view, pixels_per_point);
                }
                let resize_stopped = resize.as_ref().is_some_and(egui::Response::drag_stopped);
                if response.drag_stopped() || resize_stopped {
                    self.commit_reference_drag(reference.id);
                }
            }

            if self.selected_reference == Some(reference.id) {
                let painter = context
                    .layer_painter(area.response.layer_id)
                    .with_clip_rect(workspace_rect);
                paint_reference_selection(&painter, rect, !reference.locked);
            }
        }

        if !context.input(|input| input.pointer.primary_down())
            && let Some(id) = self.reference_transform_edit.as_ref().map(|edit| edit.id)
        {
            self.commit_reference_drag(id);
        }
    }

    fn show_reference_context_menu(
        &mut self,
        response: &egui::Response,
        reference: &ReferenceImage,
    ) {
        response.context_menu(|ui| {
            ui.set_min_width(80.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if ui
                .button(if reference.locked { "Unlock" } else { "Lock" })
                .clicked()
            {
                self.commands
                    .push(AppCommand::ToggleReferenceLocked(reference.id));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                self.commands
                    .push(AppCommand::DeleteReference(reference.id));
                ui.close();
            }
        });
    }

    pub(crate) fn pointer_over_reference(&self) -> bool {
        self.pointer_over_reference
    }

    pub(crate) fn reference_drag_active(&self) -> bool {
        self.reference_transform_edit.is_some()
    }

    pub(crate) fn reference_resize_active(&self) -> bool {
        self.reference_transform_edit
            .is_some_and(|edit| edit.drag == ReferenceDrag::Resize)
    }

    pub(crate) fn window_point_over_reference(&self, point: [f32; 2]) -> bool {
        window_point_over_rects(
            point,
            self.context.pixels_per_point(),
            &self.reference_hit_rects,
        )
    }

    fn clear_reference_selection_on_outside_press(&mut self, context: &egui::Context) {
        let primary_pressed = context.input(|input| input.pointer.primary_pressed());
        if should_clear_reference_selection(
            self.selected_reference.is_some(),
            primary_pressed,
            self.pointer_over_selected_reference,
            context.is_pointer_over_egui(),
        ) {
            self.selected_reference = None;
        }
    }

    fn update_reference_drag(
        &mut self,
        id: ReferenceId,
        drag: ReferenceDrag,
        drag_delta: egui::Vec2,
        view: PaintViewSnapshot,
        pixels_per_point: f32,
    ) {
        let Some(origin) = self.reference_transform_edit.filter(|edit| edit.id == id) else {
            return;
        };
        let delta = view.window_delta_to_workspace([
            drag_delta.x * pixels_per_point,
            drag_delta.y * pixels_per_point,
        ]);
        let (position, size) =
            reference_transform_from_drag_origin(origin.position, origin.size, drag, delta);
        self.commands
            .push(AppCommand::SetReferenceTransform { id, position, size });
    }

    fn commit_reference_drag(&mut self, id: ReferenceId) {
        if self
            .reference_transform_edit
            .is_some_and(|edit| edit.id == id)
        {
            self.reference_transform_edit = None;
        }
    }

    fn brush_preview_texture(&self, brush_id: &str) -> Option<egui::TextureId> {
        self.brush_previews
            .iter()
            .find(|(id, _)| id == brush_id)
            .map(|(_, texture)| texture.id())
    }

    fn load_brush_preview(&mut self, brush_id: &str) {
        if self.brush_preview_texture(brush_id).is_some()
            || self.failed_brush_previews.iter().any(|id| id == brush_id)
        {
            return;
        }
        let Some(brush) = self
            .brushes
            .iter()
            .find(|brush| brush.id == brush_id)
            .cloned()
        else {
            return;
        };

        match brush_preview::generate(&brush) {
            Ok(image) => {
                let texture = self.context.load_texture(
                    format!("brush preview {}", brush.id),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                self.brush_previews.push((brush.id, texture));
            }
            Err(error) => {
                log::warn!(
                    "failed to generate preview for brush '{}': {error}",
                    brush.id
                );
                self.failed_brush_previews.push(brush.id);
            }
        }
    }

    fn load_next_brush_preview(&mut self) {
        let next_id = self
            .brushes
            .iter()
            .find(|brush| {
                self.brush_preview_texture(&brush.id).is_none()
                    && !self.failed_brush_previews.iter().any(|id| id == &brush.id)
            })
            .map(|brush| brush.id.clone());
        if let Some(id) = next_id {
            self.load_brush_preview(&id);
            self.context.request_repaint();
        }
    }

    fn show_brush_controls(&mut self, ui: &mut egui::Ui) {
        if color_picker::show(ui, &mut self.brush.color) {
            self.commands
                .push(AppCommand::SetBrushColor(self.brush.color.to_array()));
        }
    }

    fn show_layers(
        &mut self,
        ui: &mut egui::Ui,
        layers: &LayerSnapshot,
        background: egui::Color32,
    ) {
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("layer list")
            .max_height(LAYER_LIST_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for layer in layers.layers.iter().rev() {
                    let selected = layers.selection == layer.id;
                    let thumbnail = self
                        .layer_thumbnails
                        .iter()
                        .find(|thumbnail| thumbnail.key.layer_id == layer.id)
                        .map(|thumbnail| thumbnail.texture_id);
                    let renaming = self
                        .layer_name_edit
                        .as_ref()
                        .is_some_and(|edit| edit.id == layer.id);
                    let clipped_name = layer.clipped.then(|| format!("↳ {}", layer.name));
                    let row = show_layer_row(
                        ui,
                        LayerRow {
                            name: if renaming {
                                ""
                            } else {
                                clipped_name.as_deref().unwrap_or(&layer.name)
                            },
                            selected,
                            texture_id: thumbnail,
                            solid_color: None,
                            visible: Some(layer.visible),
                            opacity: Some(layer.opacity),
                            drag_id: Some(layer.id),
                        },
                    );
                    if let (Some(pointer), Some(dragged)) = (
                        ui.input(|input| input.pointer.interact_pos()),
                        row.row.dnd_hover_payload::<LayerId>(),
                    ) && *dragged != layer.id
                    {
                        let edge = if pointer.y < row.row.rect.center().y {
                            DropEdge::Above
                        } else {
                            DropEdge::Below
                        };
                        let y = match edge {
                            DropEdge::Above => row.row.rect.top(),
                            DropEdge::Below => row.row.rect.bottom(),
                        };
                        ui.painter().hline(
                            row.row.rect.x_range().shrink(8.0),
                            y,
                            egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
                        );
                        if let Some(dragged) = row.row.dnd_release_payload::<LayerId>() {
                            self.commands.push(AppCommand::MoveLayer {
                                dragged: *dragged,
                                target: layer.id,
                                edge,
                            });
                        }
                    }
                    if row.mode.as_ref().is_some_and(egui::Response::clicked) {
                        self.layer_opacity_open = if self.layer_opacity_open == Some(layer.id) {
                            None
                        } else {
                            Some(layer.id)
                        };
                        if !selected {
                            self.commands.push(AppCommand::SelectLayer(layer.id));
                        }
                    } else if row.visibility.as_ref().is_some_and(egui::Response::clicked) {
                        self.commands.push(AppCommand::SetLayerVisibility {
                            id: layer.id,
                            visible: !layer.visible,
                        });
                    } else if (row.row.clicked() || row.row.secondary_clicked()) && !selected {
                        self.commands.push(AppCommand::SelectLayer(layer.id));
                    }

                    let mut start_rename = false;
                    let layer_index = layers
                        .layers
                        .iter()
                        .position(|candidate| candidate.id == layer.id)
                        .expect("displayed layer must exist in snapshot");
                    let can_merge_down = layer.visible
                        && merge_down_target_index(layer_index).is_some_and(|lower_index| {
                            layers.layers[lower_index].visible
                                && !layers.layers[lower_index].clipped
                        })
                        && !layers
                            .layers
                            .get(layer_index + 1)
                            .is_some_and(|upper| upper.clipped);
                    let can_delete =
                        layers.layers.len() > 1 && (layer_index != 0 || !layers.layers[1].clipped);
                    row.row.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            start_rename = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(can_merge_down, egui::Button::new("Merge Down"))
                            .clicked()
                        {
                            self.commands.push(AppCommand::MergeLayerDown(layer.id));
                            ui.close();
                        }
                        let clipping_label = if layer.clipped {
                            "Release Clipping Mask"
                        } else {
                            "Clip to Layer Below"
                        };
                        if ui
                            .add_enabled(
                                layer.clipped || layer_index > 0,
                                egui::Button::new(clipping_label),
                            )
                            .clicked()
                        {
                            self.commands.push(AppCommand::SetLayerClipped {
                                id: layer.id,
                                clipped: !layer.clipped,
                            });
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                can_delete,
                                egui::Button::new(
                                    egui::RichText::new("Delete").color(egui::Color32::LIGHT_RED),
                                ),
                            )
                            .clicked()
                        {
                            self.commands.push(AppCommand::DeleteSelectedLayer);
                            ui.close();
                        }
                    });
                    if start_rename {
                        self.layer_name_edit = Some(LayerNameEdit {
                            id: layer.id,
                            name: layer.name.clone(),
                        });
                    }

                    if renaming {
                        let mut edited_name = self
                            .layer_name_edit
                            .as_ref()
                            .map_or_else(|| layer.name.clone(), |edit| edit.name.clone());
                        let response = ui.put(
                            row.name_rect,
                            egui::TextEdit::singleline(&mut edited_name)
                                .desired_width(row.name_rect.width()),
                        );
                        if !response.has_focus() && !response.lost_focus() {
                            response.request_focus();
                        }
                        let cancel = response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Escape));
                        let commit = !cancel
                            && (response.lost_focus()
                                || (response.has_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))));
                        if cancel {
                            self.layer_name_edit = None;
                        } else if commit {
                            if !edited_name.trim().is_empty() {
                                self.commands.push(AppCommand::RenameLayer {
                                    id: layer.id,
                                    name: edited_name,
                                });
                            }
                            self.layer_name_edit = None;
                        } else if let Some(edit) = self.layer_name_edit.as_mut() {
                            edit.name = edited_name;
                        }
                    }

                    if self.layer_opacity_open == Some(layer.id) {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 14,
                                right: 14,
                                top: 6,
                                bottom: 0,
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Opacity");
                                    let mut opacity = layer.opacity;
                                    let opacity_changed = ui
                                        .add(
                                            egui::Slider::new(&mut opacity, 0..=100)
                                                .suffix("%")
                                                .show_value(true),
                                        )
                                        .on_hover_text("Layer opacity")
                                        .changed();
                                    if opacity_changed {
                                        let edit = self.layer_opacity_edit.get_or_insert(
                                            LayerOpacityEdit {
                                                id: layer.id,
                                                before: layer.opacity,
                                                current: opacity,
                                            },
                                        );
                                        if edit.id != layer.id {
                                            *edit = LayerOpacityEdit {
                                                id: layer.id,
                                                before: layer.opacity,
                                                current: opacity,
                                            };
                                        } else {
                                            edit.current = opacity;
                                        }
                                        self.commands.push(AppCommand::SetLayerOpacity {
                                            id: layer.id,
                                            opacity,
                                        });
                                    }
                                });
                            });
                    }

                    ui.add_space(4.0);
                }

                if !ui.ctx().input(|input| input.pointer.primary_down())
                    && let Some(edit) = self.layer_opacity_edit.take()
                {
                    self.commands.push(AppCommand::CommitLayerOpacity {
                        id: edit.id,
                        before: edit.before,
                        after: edit.current,
                    });
                }

                let mut color = background;
                let response = show_layer_row(
                    ui,
                    LayerRow {
                        name: "Background",
                        selected: false,
                        texture_id: None,
                        solid_color: Some(background),
                        visible: None,
                        opacity: None,
                        drag_id: None,
                    },
                );
                egui::Popup::from_toggle_button_response(&response.row)
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if color_picker::show(ui, &mut color) {
                            self.background_edit_start.get_or_insert(rgb(background));
                            self.commands
                                .push(AppCommand::SetBackgroundColor(rgb(color)));
                        }
                    });
                if !ui.ctx().input(|input| input.pointer.primary_down())
                    && let Some(before) = self.background_edit_start.take()
                {
                    self.commands.push(AppCommand::CommitBackgroundColor {
                        before,
                        after: rgb(color),
                    });
                }
            });
    }

    fn show_tool_rail(&mut self, ui: &mut egui::Ui, active_tool: EditorTool) -> Option<EditorTool> {
        const TOOL_WIDTH: f32 = 40.0;
        const TOOL_COUNT: usize = 3;
        const HORIZONTAL_PADDING: f32 = 6.0;

        let tools = [PaintTool::Brush, PaintTool::Eraser, PaintTool::Smudge];
        let button_count = TOOL_COUNT + 2;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                TOOL_WIDTH * button_count as f32 + 2.0 * HORIZONTAL_PADDING,
                TOOL_RAIL_THICKNESS,
            ),
            egui::Sense::hover(),
        );
        paint_rounded_panel(ui, rect, egui::CornerRadius::same(16));
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.left() + HORIZONTAL_PADDING, rect.top()),
            egui::pos2(rect.right() - HORIZONTAL_PADDING, rect.bottom()),
        );

        let mut selected_tool = None;
        for (index, paint_tool) in tools.into_iter().enumerate() {
            let tool = EditorTool::from(paint_tool);
            let tool_rect = egui::Rect::from_min_size(
                egui::pos2(body.left() + index as f32 * TOOL_WIDTH, body.top()),
                egui::vec2(TOOL_WIDTH, TOOL_RAIL_THICKNESS),
            );
            let response = show_tool_button(ui, tool_rect, paint_tool, tool == active_tool);
            if tool != active_tool {
                if response.clicked() {
                    egui::Popup::close_all(ui.ctx());
                    selected_tool = Some(tool);
                }
                continue;
            }

            let popup_id = egui::Popup::default_response_id(&response);
            let selected_brush = self.tool_brushes[tool_index(paint_tool)].clone();
            let brushes = self.brushes.clone();
            egui::Popup::menu(&response)
                .align(egui::RectAlign::BOTTOM_START)
                .width(300.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for brush in &brushes {
                                let selected = brush.id == selected_brush;
                                let preview = self.brush_preview_texture(&brush.id);
                                let response = show_brush_row(ui, &brush.name, preview, selected);
                                if response.clicked() {
                                    if !selected {
                                        self.commands.push(AppCommand::SwitchBrush {
                                            tool: paint_tool,
                                            id: brush.id.clone(),
                                        });
                                    }
                                    ui.close();
                                }
                                ui.add_space(4.0);
                            }
                        });
                });
            if egui::Popup::is_id_open(ui.ctx(), popup_id) {
                self.load_next_brush_preview();
            }
        }

        {
            let separator_x = body.left() + TOOL_COUNT as f32 * TOOL_WIDTH;
            let layers_rect = egui::Rect::from_min_size(
                egui::pos2(separator_x, body.top()),
                egui::vec2(TOOL_WIDTH, TOOL_RAIL_THICKNESS),
            );
            let layers_response = ui
                .interact(layers_rect, ui.id().with("Layers"), egui::Sense::click())
                .on_hover_text("Layers");
            let layers_color = if self.layers_window_open || layers_response.hovered() {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            egui::Image::new(egui::include_image!("../../assets/icons/layers.svg"))
                .fit_to_exact_size(egui::Vec2::splat(20.0))
                .tint(layers_color)
                .alt_text("Layers")
                .paint_at(
                    ui,
                    egui::Rect::from_center_size(layers_rect.center(), egui::Vec2::splat(20.0)),
                );
            if layers_response.clicked() {
                self.layers_window_open = !self.layers_window_open;
            }

            let color_rect = layers_rect.translate(egui::vec2(TOOL_WIDTH, 0.0));
            let color_response = ui
                .interact(color_rect, ui.id().with("Color"), egui::Sense::click())
                .on_hover_text("Color");
            ui.painter()
                .circle_filled(color_rect.center(), 9.0, self.brush.color);
            if color_response.clicked() {
                self.color_window_open = !self.color_window_open;
            }
        }
        selected_tool
    }

    fn show_layer_transform(
        &mut self,
        ui: &mut egui::Ui,
        view: PaintViewSnapshot,
        bounds: Option<LayerContentBounds>,
        active: Option<LayerTransform>,
        workspace_rect: egui::Rect,
    ) {
        let Some(bounds) = bounds else {
            self.layer_transform_drag = None;
            return;
        };
        let context = ui.ctx().clone();
        let transform = active.unwrap_or_default();
        let pixels_per_point = context.pixels_per_point();
        let corners = layer_transform_screen_corners(bounds, transform, view, pixels_per_point);
        let handles = canvas_crop_handle_positions(corners);
        let rotation_handle = layer_rotation_handle(corners);
        let pointer = context.pointer_latest_pos();
        let hovered_handle = pointer.and_then(|pointer| {
            layer_transform_handle_at(pointer, corners, &handles, rotation_handle)
        });
        let cursor_handle = self
            .layer_transform_drag
            .map(|drag| drag.handle)
            .or(hovered_handle);
        let (panning, preserve_aspect) =
            context.input(|input| (input.key_down(egui::Key::Space), input.modifiers.shift));
        let interaction_rect = if panning {
            egui::Rect::NOTHING
        } else if self.layer_transform_drag.is_some() {
            workspace_rect
        } else if let (Some(_), Some(pointer)) = (hovered_handle, pointer) {
            egui::Rect::from_center_size(pointer, egui::Vec2::splat(2.0))
        } else {
            egui::Rect::NOTHING
        };
        let response = ui.interact(
            interaction_rect,
            ui.id().with("layer transform interaction"),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter().with_clip_rect(workspace_rect);
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            egui::Stroke::new(2.0, egui::Color32::from_gray(96)),
        ));
        let top_center = corners[0] + (corners[1] - corners[0]) * 0.5;
        painter.line_segment(
            [top_center, rotation_handle],
            egui::Stroke::new(1.5, egui::Color32::from_gray(160)),
        );
        paint_resize_handles(&painter, &handles);
        painter.circle_filled(rotation_handle, 7.0, egui::Color32::from_gray(232));
        painter.circle_stroke(
            rotation_handle,
            7.0,
            egui::Stroke::new(1.5, egui::Color32::from_gray(72)),
        );
        painter.text(
            egui::pos2(workspace_rect.center().x, workspace_rect.top() + 16.0),
            egui::Align2::CENTER_TOP,
            "Enter to apply  •  Esc to cancel",
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
        let response = if let Some(handle) = cursor_handle {
            response.on_hover_cursor(layer_transform_cursor(handle))
        } else {
            response
        };

        if response.drag_started()
            && let (Some(pointer), Some(handle)) = (pointer, hovered_handle)
        {
            self.layer_transform_drag = Some(LayerTransformDrag {
                handle,
                start_transform: transform,
                start_pointer: pointer_document(pointer, view, pixels_per_point),
            });
        }
        if response.dragged()
            && let (Some(pointer), Some(drag)) = (pointer, self.layer_transform_drag)
        {
            let pointer = pointer_document(pointer, view, pixels_per_point);
            self.commands
                .push(AppCommand::SetLayerTransform(layer_transform_from_drag(
                    drag,
                    pointer,
                    bounds,
                    preserve_aspect,
                )));
        }
        if response.drag_stopped() {
            self.layer_transform_drag = None;
        }
    }

    pub fn run_editor(&mut self, window: &Window, state: EditorUiState<'_>) -> egui::FullOutput {
        let EditorUiState {
            layers,
            tool,
            layer_transform,
            layer_content_bounds,
            brush_resize_label,
            eyedropper_indicator,
            save_status,
            pending_navigation,
            brush_import_dialog_delay,
            reference_import_dialog_delay,
            reference_load_dialog_delay,
            references,
            workspace_view,
        } = state;
        if let Some(paint_tool) = tool.paint_tool() {
            self.tool_sizes[tool_index(paint_tool)] = self.brush.size;
            self.load_brush_preview(&self.tool_brushes[tool_index(paint_tool)].clone());
        }
        self.sync_reference_textures(references);
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        context.run_ui(raw_input, |ui| {
            if !ui.ctx().egui_wants_keyboard_input()
                && ui
                    .ctx()
                    .input(|input| input.key_pressed(egui::Key::Questionmark))
            {
                self.shortcuts_dialog_open = true;
            }
            let background = background_color(layers.background_color);
            if let Some(id) = self.selected_reference
                && !ui.ctx().egui_wants_keyboard_input()
                && ui.ctx().input(|input| input.key_pressed(egui::Key::Delete))
            {
                self.commands.push(AppCommand::DeleteReference(id));
            }

            if self.color_window_open {
                egui::Window::new("Color")
                    .id(egui::Id::new("floating color picker"))
                    .default_pos(egui::pos2(24.0, 80.0))
                    .default_width(280.0)
                    .resizable(false)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| self.show_brush_controls(ui));
            }

            if self.layers_window_open {
                let layers_response = egui::Window::new("Layers")
                    .id(egui::Id::new("floating layers"))
                    .default_pos(egui::pos2(340.0, 80.0))
                    .auto_sized()
                    .default_width(LAYER_PANEL_WIDTH)
                    .min_width(LAYER_PANEL_WIDTH)
                    .max_width(LAYER_PANEL_WIDTH)
                    .max_height(LAYER_LIST_MAX_HEIGHT + 48.0)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| self.show_layers(ui, layers, background));
                if let Some(response) = layers_response {
                    let button_rect = egui::Rect::from_min_size(
                        response.response.rect.left_top() + egui::vec2(8.0, 6.0),
                        egui::Vec2::splat(28.0),
                    );
                    let mut button_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id(egui::Id::new("floating layer add button"))
                            .layer_id(response.response.layer_id)
                            .max_rect(button_rect),
                    );
                    button_ui.set_clip_rect(response.response.rect);
                    if add_layer_button(&mut button_ui) {
                        self.commands.push(AppCommand::AddLayer);
                    }
                }
            }

            if !ui.ctx().egui_wants_keyboard_input() {
                if ui.ctx().input(|input| input.key_pressed(egui::Key::Enter))
                    && tool == EditorTool::Transform
                {
                    self.layer_transform_drag = None;
                    self.commands.push(AppCommand::ApplyLayerTransform);
                }
                if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
                    if tool == EditorTool::Transform {
                        self.layer_transform_drag = None;
                        self.commands.push(AppCommand::CancelLayerTransform);
                    } else {
                        self.color_window_open = false;
                        self.layers_window_open = false;
                    }
                }
            }

            let workspace_rect = ui.available_rect_before_wrap();
            self.show_workspace_references(ui.ctx(), references, workspace_view, workspace_rect);
            if tool == EditorTool::Transform {
                self.show_layer_transform(
                    ui,
                    workspace_view,
                    layer_content_bounds,
                    layer_transform,
                    workspace_rect,
                );
            } else {
                self.layer_transform_drag = None;
            }

            let selected_tool = egui::Area::new(egui::Id::new("tool rail"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| self.show_tool_rail(ui, tool))
                .inner;
            if let Some(tool) = selected_tool {
                self.commands.push(AppCommand::SelectTool(tool));
            }
            if let Some(label) = brush_resize_label {
                show_brush_resize_label(ui, label, self.brush.size);
            }
            if let Some(indicator) = eyedropper_indicator {
                show_eyedropper_indicator(ui, indicator);
            }
            if let Some(action) = pending_navigation {
                show_save_blocker(ui.ctx(), action, &save_status, &mut self.commands);
            }
            if let Some(delay) = brush_import_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(
                        ui.ctx(),
                        "brush import dialog",
                        "Importing Photoshop brushes…",
                    );
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            if let Some(delay) = reference_import_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(
                        ui.ctx(),
                        "reference import dialog",
                        "Importing reference…",
                    );
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            if let Some(delay) = reference_load_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(ui.ctx(), "reference load dialog", "Loading references…");
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            self.show_new_artwork_dialog(ui.ctx());
            self.show_canvas_crop(ui.ctx(), workspace_view, workspace_rect);
            self.clear_reference_selection_on_outside_press(ui.ctx());
            self.show_message_dialog(ui.ctx());
            self.show_shortcuts_dialog(ui.ctx());
        })
    }

    pub fn run_gallery(
        &mut self,
        window: &Window,
        artworks: &[ArtworkSummary],
        discovery_warning: Option<&str>,
    ) -> egui::FullOutput {
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();
        context.run_ui(raw_input, |ui| {
            if !ui.ctx().egui_wants_keyboard_input()
                && ui
                    .ctx()
                    .input(|input| input.key_pressed(egui::Key::Questionmark))
            {
                self.shortcuts_dialog_open = true;
            }
            self.gallery
                .show(ui, artworks, discovery_warning, &mut self.commands);
            self.show_new_artwork_dialog(ui.ctx());
            self.show_message_dialog(ui.ctx());
            self.show_shortcuts_dialog(ui.ctx());
        })
    }

    pub(crate) fn open_shortcuts_dialog(&mut self) {
        self.shortcuts_dialog_open = true;
        self.context.request_repaint();
    }

    pub(crate) fn open_new_artwork_dialog(&mut self) {
        self.close_canvas_crop();
        self.new_artwork_dialog = Some(NewArtworkDialog {
            width: DEFAULT_CANVAS_SIZE[0],
            height: DEFAULT_CANVAS_SIZE[1],
        });
        self.context.request_repaint();
    }

    pub(crate) fn close_new_artwork_dialog(&mut self) {
        self.new_artwork_dialog = None;
    }

    fn show_new_artwork_dialog(&mut self, context: &egui::Context) {
        let Some(dialog) = self.new_artwork_dialog.as_mut() else {
            return;
        };
        let mut close = false;
        let mut create = None;
        let response = egui::Modal::new(egui::Id::new("new artwork dialog")).show(context, |ui| {
            ui.heading("New Artwork");
            ui.add_space(8.0);
            egui::Grid::new("new artwork dimensions")
                .num_columns(3)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut dialog.width)
                            .range(1..=self.canvas_size_constraints.max_dimension)
                            .speed(1),
                    );
                    ui.label("px");
                    ui.end_row();

                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut dialog.height)
                            .range(1..=self.canvas_size_constraints.max_dimension)
                            .speed(1),
                    );
                    ui.label("px");
                    ui.end_row();
                });
            let validation = self
                .canvas_size_constraints
                .validate([dialog.width, dialog.height]);
            if let Err(error) = &validation {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                let submit = ui.add_enabled(validation.is_ok(), egui::Button::new("Create"));
                if submit.clicked()
                    || (validation.is_ok() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    create = Some((dialog.width, dialog.height));
                }
            });
        });
        close |= response.should_close();
        if let Some((width, height)) = create {
            self.commands
                .push(AppCommand::CreateArtwork { width, height });
            close = true;
        }
        if close {
            self.new_artwork_dialog = None;
        }
    }

    pub(crate) fn open_canvas_crop(&mut self, size: [u32; 2]) {
        self.new_artwork_dialog = None;
        let (restore_color_window, restore_layers_window) = self
            .canvas_crop
            .as_ref()
            .map_or((self.color_window_open, self.layers_window_open), |crop| {
                (crop.restore_color_window, crop.restore_layers_window)
            });
        self.canvas_crop = Some(CanvasCrop {
            rect: CanvasCropRect::from_size(size),
            drag: None,
            restore_color_window,
            restore_layers_window,
        });
        self.color_window_open = false;
        self.layers_window_open = false;
        self.selected_reference = None;
        self.reference_transform_edit = None;
        self.context.request_repaint();
    }

    pub(crate) fn close_canvas_crop(&mut self) {
        if let Some(crop) = self.canvas_crop.take() {
            self.color_window_open = crop.restore_color_window;
            self.layers_window_open = crop.restore_layers_window;
            self.context.request_repaint();
        }
    }

    pub(crate) fn canvas_crop_active(&self) -> bool {
        self.canvas_crop.is_some()
    }

    fn show_canvas_crop(
        &mut self,
        context: &egui::Context,
        view: PaintViewSnapshot,
        workspace_rect: egui::Rect,
    ) {
        let Some(rect) = self.canvas_crop.as_ref().map(|crop| crop.rect) else {
            return;
        };
        let pixels_per_point = context.pixels_per_point();
        let screen_corners = canvas_crop_screen_corners(rect, view, pixels_per_point);
        let handle_positions = canvas_crop_handle_positions(screen_corners);
        let pointer = context.pointer_latest_pos();
        let hovered_handle = pointer.and_then(|pointer| {
            let pointer_document = pointer_document(pointer, view, pixels_per_point);
            canvas_crop_handle_at(pointer, pointer_document, rect, &handle_positions)
        });
        let cursor_handle = self
            .canvas_crop
            .as_ref()
            .and_then(|crop| crop.drag.map(|drag| drag.handle))
            .or(hovered_handle);
        let validation = canvas_crop_request(rect, self.canvas_size_constraints);
        let status = validation
            .as_ref()
            .map(|request| {
                format!(
                    "{} × {} px  •  Enter to apply  •  Esc to cancel",
                    request.size[0], request.size[1]
                )
            })
            .unwrap_or_else(|error| format!("{error}  •  Esc to cancel"));
        let status_color = if validation.is_ok() {
            egui::Color32::WHITE
        } else {
            egui::Color32::LIGHT_RED
        };

        let response = egui::Area::new(egui::Id::new("canvas crop overlay"))
            .fixed_pos(workspace_rect.min)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                let (overlay_rect, response) =
                    ui.allocate_exact_size(workspace_rect.size(), egui::Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(overlay_rect);
                painter.add(egui::Shape::convex_polygon(
                    screen_corners.to_vec(),
                    egui::Color32::from_white_alpha(8),
                    egui::Stroke::new(2.0, egui::Color32::from_gray(96)),
                ));
                paint_resize_handles(&painter, &handle_positions);
                painter.text(
                    egui::pos2(overlay_rect.center().x, overlay_rect.top() + 16.0),
                    egui::Align2::CENTER_TOP,
                    status,
                    egui::FontId::proportional(14.0),
                    status_color,
                );
                response
            })
            .inner;
        let response = if let Some(handle) = cursor_handle {
            response.on_hover_cursor(canvas_crop_cursor(handle))
        } else {
            response
        };

        if response.drag_started()
            && let Some(pointer) = pointer
            && let Some(handle) = hovered_handle
            && let Some(crop) = self.canvas_crop.as_mut()
        {
            let pointer_document = pointer_document(pointer, view, pixels_per_point);
            crop.drag = Some(CanvasCropDrag {
                handle,
                start_rect: crop.rect,
                start_pointer: pointer_document,
            });
        }
        if response.dragged()
            && let Some(pointer) = pointer
            && let Some(crop) = self.canvas_crop.as_mut()
            && let Some(drag) = crop.drag
        {
            let pointer_document = pointer_document(pointer, view, pixels_per_point);
            crop.rect = canvas_crop_rect_from_drag(drag, pointer_document);
        }
        if response.drag_stopped()
            && let Some(crop) = self.canvas_crop.as_mut()
        {
            crop.drag = None;
        }

        let cancel = context.input(|input| input.key_pressed(egui::Key::Escape));
        let apply = context.input(|input| input.key_pressed(egui::Key::Enter));
        if cancel {
            self.close_canvas_crop();
        } else if apply
            && let Some(rect) = self.canvas_crop.as_ref().map(|crop| crop.rect)
            && let Ok(request) = canvas_crop_request(rect, self.canvas_size_constraints)
        {
            self.commands.push(AppCommand::ResizeCanvas {
                width: request.size[0],
                height: request.size[1],
                origin: request.origin,
            });
            self.close_canvas_crop();
        }
    }

    pub(crate) fn toggle_panels(&mut self) {
        let open = !self.color_window_open && !self.layers_window_open;
        self.color_window_open = open;
        self.layers_window_open = open;
    }

    pub(crate) fn close_popups(&self) -> bool {
        let was_open = egui::Popup::is_any_open(&self.context);
        egui::Popup::close_all(&self.context);
        was_open
    }

    pub(crate) fn take_commands(&mut self) -> Vec<AppCommand> {
        std::mem::take(&mut self.commands)
    }

    pub(crate) fn brush_size_range(&self) -> std::ops::RangeInclusive<f32> {
        self.size_range.clone()
    }

    pub(crate) fn settings_snapshot(&self) -> (CurrentBrushConfig, [String; 3], [f32; 3]) {
        let mut brush = self.current_brush_config();
        brush.size = self.tool_sizes[tool_index(PaintTool::Brush)];
        (brush, self.tool_brushes.clone(), self.tool_sizes)
    }

    pub(crate) fn brush_for_tool(&self, tool: PaintTool) -> &str {
        &self.tool_brushes[tool_index(tool)]
    }

    pub(crate) fn remember_tool_size(&mut self, tool: PaintTool) {
        self.tool_sizes[tool_index(tool)] = self.brush.size;
    }

    pub(crate) fn set_brush_color(&mut self, color: [u8; 4]) {
        self.brush.color =
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    }

    pub(crate) fn reset_brush(&mut self) {
        self.brush.size = self.default_size;
        self.brush.color = brush_color(&CurrentBrushConfig::default());
        self.context.request_repaint();
    }

    pub fn current_brush_config(&self) -> CurrentBrushConfig {
        CurrentBrushConfig {
            size: self.brush.size,
            color: self.brush.color.to_array(),
        }
    }

    pub(crate) fn apply_brush_preset(
        &mut self,
        tool: PaintTool,
        loaded: &LoadedBrushPreset,
        catalog: BrushCatalog,
        reloaded: bool,
        reset_size: bool,
    ) {
        let preset = &loaded.preset;
        let index = tool_index(tool);
        self.tool_brushes[index].clone_from(&loaded.id);
        if reloaded {
            self.brush_previews.clear();
        } else {
            self.brush_previews
                .retain(|(id, _)| catalog.brushes.iter().any(|brush| brush.id == *id));
        }
        self.failed_brush_previews.clear();
        self.brushes = catalog.brushes;
        self.size_range = preset.size.min..=preset.size.max;
        self.default_size = preset.size.default;
        if reset_size {
            self.tool_sizes[index] = self.default_size;
        } else {
            self.tool_sizes[index] = self.tool_sizes[index].clamp(preset.size.min, preset.size.max);
        }
        self.brush.size = self.tool_sizes[index];
        self.brush.pressure = PressureSettings {
            min_size: preset.pressure.min_size,
            min_opacity: preset.pressure.min_opacity,
            full_opacity_pressure: preset.pressure.full_opacity_pressure,
            opacity_gamma: preset.pressure.opacity_gamma,
        };
        self.brush.spacing = BrushSpacing {
            ratio: preset.spacing.ratio,
            minimum: preset.spacing.minimum,
        };
    }

    pub(crate) fn settings_reloaded(&mut self, config: &AppConfig, active_tool: PaintTool) {
        self.tool_brushes = [
            config.active_brush.clone(),
            config.eraser_brush.clone(),
            config.smudge_brush.clone(),
        ];
        self.tool_sizes = [
            config.size_for_tool(PaintTool::Brush),
            config.size_for_tool(PaintTool::Eraser),
            config.size_for_tool(PaintTool::Smudge),
        ];
        self.brush.color = brush_color(&config.brush);
        self.brush.size = self.tool_sizes[tool_index(active_tool)]
            .clamp(*self.size_range.start(), *self.size_range.end());
        self.context.request_repaint();
    }

    pub(crate) fn show_error(
        &mut self,
        message: impl Into<String>,
        details: impl std::fmt::Display,
    ) {
        self.message_dialog = Some(MessageDialog::error(message, details));
        self.context.request_repaint();
    }

    pub(crate) fn show_success(&mut self, message: impl Into<String>) {
        self.message_dialog = Some(MessageDialog::success(message));
        self.context.request_repaint();
    }

    fn show_message_dialog(&mut self, context: &egui::Context) {
        let Some(dialog) = self.message_dialog.as_ref() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("message dialog")).show(context, |ui| {
            ui.set_width(320.0);
            ui.heading(dialog.title);
            ui.add_space(8.0);
            ui.label(&dialog.message);
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button("OK").clicked()
            })
            .inner
        });
        if response.inner || context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.message_dialog = None;
        }
    }

    fn show_shortcuts_dialog(&mut self, context: &egui::Context) {
        if !self.shortcuts_dialog_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("keyboard shortcuts")).show(context, |ui| {
            ui.set_width(700.0);
            ui.add_space(16.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 0))
                .show(ui, |ui| {
                    ui.heading("Keyboard Shortcuts");
                });
            ui.add_space(16.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(520.0)
                        .show(ui, |ui| {
                            shortcut_section(
                                ui,
                                "Tools",
                                &[
                                    ("Brush / Eraser / Smudge", "B / E / S"),
                                    ("Transform", "T"),
                                    ("Cycle paint tools", "Shift-Tab"),
                                    ("Show or hide panels", "Tab"),
                                    ("Resize brush", "Shift-drag"),
                                    ("Eyedropper", "Alt/Option"),
                                ],
                            );
                            ui.add_space(14.0);
                            ui.add_space(14.0);
                            shortcut_section(
                                ui,
                                "Canvas",
                                &[
                                    ("Pan", "Space-drag or middle/right-drag"),
                                    ("Zoom", "Wheel"),
                                    ("Rotate freely", "R-drag"),
                                    ("Reset rotation", "Shift-R"),
                                    ("Rotate left / right", ROTATE_SHORTCUT),
                                    ("Flip horizontal / vertical", FLIP_SHORTCUT),
                                    ("Crop or resize", CROP_SHORTCUT),
                                ],
                            );
                            ui.add_space(14.0);
                            ui.add_space(14.0);
                            shortcut_section(
                                ui,
                                "Document",
                                &[
                                    ("Save", SAVE_SHORTCUT),
                                    ("Export PNG", EXPORT_SHORTCUT),
                                    ("Return to gallery", GALLERY_SHORTCUT),
                                    ("Undo", UNDO_SHORTCUT),
                                    ("Redo", REDO_SHORTCUT),
                                    ("Apply transform or crop", "Enter"),
                                    ("Cancel transform or crop", "Escape"),
                                ],
                            );
                        });
                });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button("Close").clicked()
            })
            .inner
        });
        if response.inner || context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.shortcuts_dialog_open = false;
        }
    }
}

fn shortcut_section(ui: &mut egui::Ui, title: &str, shortcuts: &[(&str, &str)]) {
    ui.strong(title);
    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 9.0;
        for &(action, shortcut) in shortcuts {
            let (rect, response) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("{action}: {shortcut}"),
                )
            });
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                action,
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().text_color(),
            );
            ui.painter().text(
                egui::pos2(rect.center().x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                shortcut,
                egui::TextStyle::Monospace.resolve(ui.style()),
                ui.visuals().text_color(),
            );
        }
    });
}

fn canvas_crop_screen_corners(
    rect: CanvasCropRect,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [egui::Pos2; 4] {
    [
        [rect.min[0], rect.min[1]],
        [rect.max[0], rect.min[1]],
        [rect.max[0], rect.max[1]],
        [rect.min[0], rect.max[1]],
    ]
    .map(|point| {
        let point = view.document_to_window(point);
        egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point)
    })
}

fn canvas_crop_handle_positions(corners: [egui::Pos2; 4]) -> [(CanvasCropHandle, egui::Pos2); 8] {
    let midpoint = |a: egui::Pos2, b: egui::Pos2| a + (b - a) * 0.5;
    [
        (CanvasCropHandle::TopLeft, corners[0]),
        (CanvasCropHandle::Top, midpoint(corners[0], corners[1])),
        (CanvasCropHandle::TopRight, corners[1]),
        (CanvasCropHandle::Right, midpoint(corners[1], corners[2])),
        (CanvasCropHandle::BottomRight, corners[2]),
        (CanvasCropHandle::Bottom, midpoint(corners[2], corners[3])),
        (CanvasCropHandle::BottomLeft, corners[3]),
        (CanvasCropHandle::Left, midpoint(corners[3], corners[0])),
    ]
}

fn paint_resize_handles(painter: &egui::Painter, handles: &[(CanvasCropHandle, egui::Pos2); 8]) {
    for (_, position) in handles {
        let handle_rect = egui::Rect::from_center_size(*position, egui::Vec2::splat(14.0));
        painter.rect_filled(handle_rect, 3.0, egui::Color32::from_gray(232));
        painter.rect_stroke(
            handle_rect,
            2.0,
            egui::Stroke::new(1.5, egui::Color32::from_gray(72)),
            egui::StrokeKind::Inside,
        );
    }
}

fn layer_transform_screen_corners(
    bounds: LayerContentBounds,
    transform: LayerTransform,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [egui::Pos2; 4] {
    layer_transform_document_corners(bounds, transform).map(|point| {
        let point = view.document_to_window(point);
        egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point)
    })
}

fn layer_transform_document_corners(
    bounds: LayerContentBounds,
    transform: LayerTransform,
) -> [[f32; 2]; 4] {
    let pivot = [
        (bounds.min[0] + bounds.max[0]) * 0.5,
        (bounds.min[1] + bounds.max[1]) * 0.5,
    ];
    let (sin, cos) = transform.rotation.sin_cos();
    [
        bounds.min,
        [bounds.max[0], bounds.min[1]],
        bounds.max,
        [bounds.min[0], bounds.max[1]],
    ]
    .map(|point| {
        let x = (point[0] - pivot[0]) * transform.scale[0];
        let y = (point[1] - pivot[1]) * transform.scale[1];
        [
            pivot[0] + transform.translation[0] + cos * x - sin * y,
            pivot[1] + transform.translation[1] + sin * x + cos * y,
        ]
    })
}

fn layer_rotation_handle(corners: [egui::Pos2; 4]) -> egui::Pos2 {
    let top_center = corners[0] + (corners[1] - corners[0]) * 0.5;
    let center = corners[0] + (corners[2] - corners[0]) * 0.5;
    top_center + (top_center - center).normalized() * 36.0
}

fn pointer_document(
    pointer: egui::Pos2,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [f32; 2] {
    view.window_to_document([pointer.x * pixels_per_point, pointer.y * pixels_per_point])
}

fn canvas_crop_handle_at(
    pointer: egui::Pos2,
    pointer_document: [f32; 2],
    rect: CanvasCropRect,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    resize_handle_at(pointer, handles)
        .or_else(|| resize_edge_at(pointer, handles))
        .or_else(|| {
            rect.contains(pointer_document)
                .then_some(CanvasCropHandle::Move)
        })
}

fn resize_handle_at(
    pointer: egui::Pos2,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    const CORNER_HIT_RADIUS: f32 = 28.0;
    const HANDLE_HIT_RADIUS: f32 = 24.0;
    let nearest = |corner_only: bool, radius: f32| {
        handles
            .iter()
            .filter(|(handle, _)| !corner_only || canvas_crop_handle_is_corner(*handle))
            .filter_map(|(handle, position)| {
                let distance = position.distance_sq(pointer);
                (distance <= radius * radius).then_some((*handle, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(handle, _)| handle)
    };
    nearest(true, CORNER_HIT_RADIUS).or_else(|| nearest(false, HANDLE_HIT_RADIUS))
}

fn resize_edge_at(
    pointer: egui::Pos2,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    const EDGE_HIT_RADIUS: f32 = 14.0;
    [
        (CanvasCropHandle::Top, handles[0].1, handles[2].1),
        (CanvasCropHandle::Right, handles[2].1, handles[4].1),
        (CanvasCropHandle::Bottom, handles[6].1, handles[4].1),
        (CanvasCropHandle::Left, handles[0].1, handles[6].1),
    ]
    .into_iter()
    .map(|(handle, start, end)| (handle, point_segment_distance_sq(pointer, start, end)))
    .filter(|(_, distance)| *distance <= EDGE_HIT_RADIUS * EDGE_HIT_RADIUS)
    .min_by(|left, right| left.1.total_cmp(&right.1))
    .map(|(handle, _)| handle)
}

fn layer_transform_handle_at(
    pointer: egui::Pos2,
    corners: [egui::Pos2; 4],
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
    rotation_handle: egui::Pos2,
) -> Option<LayerTransformHandle> {
    (pointer.distance_sq(rotation_handle) <= 18.0 * 18.0)
        .then_some(LayerTransformHandle::Rotate)
        .or_else(|| resize_handle_at(pointer, handles).map(LayerTransformHandle::Scale))
        .or_else(|| resize_edge_at(pointer, handles).map(LayerTransformHandle::Scale))
        .or_else(|| point_in_quad(pointer, corners).then_some(LayerTransformHandle::Move))
}

fn point_in_quad(point: egui::Pos2, corners: [egui::Pos2; 4]) -> bool {
    let crosses = std::array::from_fn::<_, 4, _>(|index| {
        let start = corners[index];
        let edge = corners[(index + 1) % 4] - start;
        let offset = point - start;
        edge.x * offset.y - edge.y * offset.x
    });
    crosses.iter().all(|cross| *cross >= 0.0) || crosses.iter().all(|cross| *cross <= 0.0)
}

fn layer_transform_from_drag(
    drag: LayerTransformDrag,
    pointer: [f32; 2],
    bounds: LayerContentBounds,
    preserve_aspect: bool,
) -> LayerTransform {
    let pivot = [
        (bounds.min[0] + bounds.max[0]) * 0.5 + drag.start_transform.translation[0],
        (bounds.min[1] + bounds.max[1]) * 0.5 + drag.start_transform.translation[1],
    ];
    match drag.handle {
        LayerTransformHandle::Move => LayerTransform {
            translation: [
                drag.start_transform.translation[0] + pointer[0] - drag.start_pointer[0],
                drag.start_transform.translation[1] + pointer[1] - drag.start_pointer[1],
            ],
            ..drag.start_transform
        },
        LayerTransformHandle::Scale(handle) => {
            let mut scale = drag.start_transform.scale;
            if preserve_aspect {
                let distance = |point: [f32; 2]| (point[0] - pivot[0]).hypot(point[1] - pivot[1]);
                let factor = distance(pointer) / distance(drag.start_pointer).max(f32::EPSILON);
                scale = scale.map(|value| (value * factor).max(0.01));
            } else {
                let delta = [pointer[0] - pivot[0], pointer[1] - pivot[1]];
                let (sin, cos) = drag.start_transform.rotation.sin_cos();
                let local = [
                    cos * delta[0] + sin * delta[1],
                    -sin * delta[0] + cos * delta[1],
                ];
                let half = [
                    (bounds.max[0] - bounds.min[0]) * 0.5,
                    (bounds.max[1] - bounds.min[1]) * 0.5,
                ];
                if matches!(
                    handle,
                    CanvasCropHandle::Left
                        | CanvasCropHandle::TopLeft
                        | CanvasCropHandle::BottomLeft
                ) {
                    scale[0] = (-local[0] / half[0]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Right
                        | CanvasCropHandle::TopRight
                        | CanvasCropHandle::BottomRight
                ) {
                    scale[0] = (local[0] / half[0]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Top | CanvasCropHandle::TopLeft | CanvasCropHandle::TopRight
                ) {
                    scale[1] = (-local[1] / half[1]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Bottom
                        | CanvasCropHandle::BottomLeft
                        | CanvasCropHandle::BottomRight
                ) {
                    scale[1] = (local[1] / half[1]).max(0.01);
                }
            }
            LayerTransform {
                scale,
                ..drag.start_transform
            }
        }
        LayerTransformHandle::Rotate => LayerTransform {
            rotation: drag.start_transform.rotation
                + angle_delta(
                    pointer_angle(pivot, pointer),
                    pointer_angle(pivot, drag.start_pointer),
                ),
            ..drag.start_transform
        },
    }
}

fn pointer_angle(center: [f32; 2], point: [f32; 2]) -> f32 {
    (point[1] - center[1]).atan2(point[0] - center[0])
}

fn angle_delta(angle: f32, origin: f32) -> f32 {
    (angle - origin + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn canvas_crop_handle_is_corner(handle: CanvasCropHandle) -> bool {
    matches!(
        handle,
        CanvasCropHandle::TopLeft
            | CanvasCropHandle::TopRight
            | CanvasCropHandle::BottomLeft
            | CanvasCropHandle::BottomRight
    )
}

fn point_segment_distance_sq(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return point.distance_sq(start);
    }
    let projection = ((point - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    point.distance_sq(start + segment * projection)
}

fn canvas_crop_cursor(handle: CanvasCropHandle) -> egui::CursorIcon {
    match handle {
        CanvasCropHandle::Move => egui::CursorIcon::Grab,
        CanvasCropHandle::Left | CanvasCropHandle::Right => egui::CursorIcon::ResizeHorizontal,
        CanvasCropHandle::Top | CanvasCropHandle::Bottom => egui::CursorIcon::ResizeVertical,
        CanvasCropHandle::TopLeft | CanvasCropHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
        CanvasCropHandle::TopRight | CanvasCropHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

fn layer_transform_cursor(handle: LayerTransformHandle) -> egui::CursorIcon {
    match handle {
        LayerTransformHandle::Move => egui::CursorIcon::Grab,
        LayerTransformHandle::Scale(handle) => canvas_crop_cursor(handle),
        LayerTransformHandle::Rotate => egui::CursorIcon::Crosshair,
    }
}

fn canvas_crop_rect_from_drag(drag: CanvasCropDrag, pointer: [f32; 2]) -> CanvasCropRect {
    let delta = [
        pointer[0] - drag.start_pointer[0],
        pointer[1] - drag.start_pointer[1],
    ];
    let mut rect = drag.start_rect;
    if drag.handle == CanvasCropHandle::Move {
        rect.min[0] += delta[0];
        rect.max[0] += delta[0];
        rect.min[1] += delta[1];
        rect.max[1] += delta[1];
        return rect;
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Left | CanvasCropHandle::TopLeft | CanvasCropHandle::BottomLeft
    ) {
        rect.min[0] = (drag.start_rect.min[0] + delta[0]).min(drag.start_rect.max[0] - 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Right | CanvasCropHandle::TopRight | CanvasCropHandle::BottomRight
    ) {
        rect.max[0] = (drag.start_rect.max[0] + delta[0]).max(drag.start_rect.min[0] + 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Top | CanvasCropHandle::TopLeft | CanvasCropHandle::TopRight
    ) {
        rect.min[1] = (drag.start_rect.min[1] + delta[1]).min(drag.start_rect.max[1] - 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Bottom | CanvasCropHandle::BottomLeft | CanvasCropHandle::BottomRight
    ) {
        rect.max[1] = (drag.start_rect.max[1] + delta[1]).max(drag.start_rect.min[1] + 1.0);
    }
    rect
}

fn canvas_crop_request(
    rect: CanvasCropRect,
    constraints: CanvasSizeConstraints,
) -> Result<CanvasCropRequest, String> {
    if rect
        .min
        .into_iter()
        .chain(rect.max)
        .any(|value| !value.is_finite())
    {
        return Err("crop bounds must be finite".to_owned());
    }
    let left = rect.min[0].round() as i64;
    let top = rect.min[1].round() as i64;
    let right = rect.max[0].round() as i64;
    let bottom = rect.max[1].round() as i64;
    let width = right
        .checked_sub(left)
        .and_then(|width| u32::try_from(width).ok())
        .ok_or_else(|| "crop width must be at least 1 pixel".to_owned())?;
    let height = bottom
        .checked_sub(top)
        .and_then(|height| u32::try_from(height).ok())
        .ok_or_else(|| "crop height must be at least 1 pixel".to_owned())?;
    constraints.validate([width, height])?;
    let origin = [
        i32::try_from(left).map_err(|_| "crop origin is outside the supported range".to_owned())?,
        i32::try_from(top).map_err(|_| "crop origin is outside the supported range".to_owned())?,
    ];
    Ok(CanvasCropRequest {
        size: [width, height],
        origin,
    })
}

fn layer_preview_is_current(current: &[LayerPreviewKey], cached: LayerPreviewKey) -> bool {
    current.contains(&cached)
}

fn show_loading_dialog(context: &egui::Context, id: &str, message: &str) {
    egui::Modal::new(egui::Id::new(id)).show(context, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(message);
        });
    });
    context.request_repaint_after(Duration::from_millis(16));
}

fn show_save_blocker(
    context: &egui::Context,
    action: &str,
    status: &SaveStatus,
    commands: &mut Vec<AppCommand>,
) {
    egui::Window::new(action)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(context, |ui| {
            match status {
                SaveStatus::Failed(error) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, "The artwork could not be saved.");
                    ui.label(error);
                }
                _ => {
                    ui.label("Saving the artwork before continuing…");
                    ui.spinner();
                }
            }
            ui.horizontal(|ui| {
                if matches!(status, SaveStatus::Failed(_)) && ui.button("Retry").clicked() {
                    commands.push(AppCommand::SaveArtwork);
                }
                if ui.button("Cancel").clicked() {
                    commands.push(AppCommand::CancelPendingNavigation);
                }
            });
        });
}

/// Allocates a full-width row and paints the selection chrome shared by the
/// brush and layer rows. Callers paint their own contents on top.
fn selectable_row(
    ui: &mut egui::Ui,
    height: f32,
    sense: egui::Sense,
    selected: bool,
) -> (egui::Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), sense);
    let dark_mode = ui.visuals().dark_mode;
    let fill = if selected {
        if dark_mode {
            egui::Color32::from_rgb(34, 39, 46)
        } else {
            egui::Color32::from_gray(224)
        }
    } else if response.hovered() {
        if dark_mode {
            egui::Color32::from_rgb(24, 28, 34)
        } else {
            egui::Color32::from_gray(240)
        }
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 12, fill);
    (rect, response)
}

fn show_brush_row(
    ui: &mut egui::Ui,
    name: &str,
    texture_id: Option<egui::TextureId>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = selectable_row(ui, 58.0, egui::Sense::click(), selected);
    let dark_mode = ui.visuals().dark_mode;
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();

    painter
        .with_clip_rect(egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.min.x + 82.0, rect.max.y),
        ))
        .text(
            egui::pos2(rect.min.x + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::TextStyle::Button.resolve(ui.style()),
            visuals.text_color(),
        );

    let preview = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 82.0, rect.min.y + 5.0),
        egui::pos2(rect.max.x - 8.0, rect.max.y - 5.0),
    );
    if let Some(texture_id) = texture_id {
        painter.image(
            texture_id,
            preview,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            visuals.text_color(),
        );
    } else {
        painter.line_segment(
            [preview.left_center(), preview.right_center()],
            egui::Stroke::new(
                2.0_f32,
                egui::Color32::from_gray(if dark_mode { 90 } else { 175 }),
            ),
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct LayerRow<'a> {
    name: &'a str,
    selected: bool,
    texture_id: Option<egui::TextureId>,
    solid_color: Option<egui::Color32>,
    visible: Option<bool>,
    opacity: Option<u8>,
    drag_id: Option<LayerId>,
}

struct LayerRowResponse {
    row: egui::Response,
    visibility: Option<egui::Response>,
    mode: Option<egui::Response>,
    name_rect: egui::Rect,
}

fn show_layer_row(ui: &mut egui::Ui, layer: LayerRow<'_>) -> LayerRowResponse {
    let LayerRow {
        name,
        selected,
        texture_id,
        solid_color,
        visible,
        opacity,
        drag_id,
    } = layer;
    let sense = if drag_id.is_some() {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };
    let (rect, response) = selectable_row(ui, 60.0, sense, selected);
    if let Some(layer_id) = drag_id {
        response.dnd_set_drag_payload(layer_id);
    }
    let visibility = visible.map(|_| {
        let eye_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 16.0, rect.center().y),
            egui::Vec2::splat(24.0),
        );
        ui.interact(
            eye_rect,
            response.id.with("visibility"),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    let mode = opacity.map(|opacity| {
        let mode_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 45.0, rect.center().y),
            egui::vec2(24.0, 28.0),
        );
        ui.interact(mode_rect, response.id.with("mode"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Normal · {opacity}%"))
    });
    let visuals = ui.style().interact(&response);
    let dark_mode = ui.visuals().dark_mode;
    let painter = ui.painter();

    if let (Some(visible), Some(visibility)) = (visible, &visibility) {
        let icon = if visible {
            egui::include_image!("../../assets/icons/eye.svg")
        } else {
            egui::include_image!("../../assets/icons/eye-off.svg")
        };
        egui::Image::new(icon)
            .fit_to_exact_size(egui::Vec2::splat(16.0))
            .tint(ui.visuals().weak_text_color())
            .paint_at(ui, visibility.rect.shrink(4.0));
    }

    if let Some(mode) = &mode {
        if mode.hovered() {
            painter.rect_filled(
                mode.rect,
                6,
                egui::Color32::from_white_alpha(if dark_mode { 20 } else { 35 }),
            );
        }
        painter.text(
            mode.rect.center(),
            egui::Align2::CENTER_CENTER,
            "N",
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
    }

    let thumbnail =
        egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::Vec2::splat(48.0));
    if let Some(color) = solid_color {
        painter.rect_filled(thumbnail, 8, color);
        painter.rect_stroke(
            thumbnail,
            8,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
    } else {
        let (light, dark) = if ui.visuals().dark_mode {
            (egui::Color32::from_gray(82), egui::Color32::from_gray(62))
        } else {
            (egui::Color32::from_gray(220), egui::Color32::from_gray(195))
        };
        painter.rect_filled(thumbnail, 8, light);
        let checker = thumbnail.shrink(3.0);
        let checker_size = checker.width() / 4.0;
        for y in 0..4 {
            for x in 0..4 {
                let square = egui::Rect::from_min_size(
                    checker.min + egui::vec2(x as f32 * checker_size, y as f32 * checker_size),
                    egui::Vec2::splat(checker_size),
                );
                painter.rect_filled(square, 0, if (x + y) % 2 == 0 { light } else { dark });
            }
        }
        if let Some(texture_id) = texture_id {
            egui::Image::new((texture_id, thumbnail.size()))
                .corner_radius(8)
                .paint_at(ui, thumbnail);
        }
    }
    let text_color = visuals.text_color();
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(thumbnail.max.x + 6.0, rect.top() + 14.0),
        egui::pos2(
            mode.as_ref()
                .map_or(rect.right() - 8.0, |mode| mode.rect.left() - 4.0),
            rect.bottom() - 14.0,
        ),
    );
    painter.with_clip_rect(name_rect).text(
        egui::pos2(name_rect.left() + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::TextStyle::Button.resolve(ui.style()),
        text_color,
    );

    let row = if drag_id.is_some() {
        response
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text("Drag to reorder")
    } else {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    };

    LayerRowResponse {
        row,
        visibility,
        mode,
        name_rect,
    }
}

fn show_eyedropper_indicator(ui: &egui::Ui, indicator: EyedropperIndicator) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let center = egui::pos2(
        indicator.center[0] / pixels_per_point,
        indicator.center[1] / pixels_per_point,
    );
    let canvas_rect = ui.available_rect_before_wrap();
    if !canvas_rect.contains(center) {
        return;
    }

    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("eyedropper indicator"),
        ))
        .with_clip_rect(canvas_rect);
    for direction in [
        egui::vec2(1.0, 0.0),
        egui::vec2(-1.0, 0.0),
        egui::vec2(0.0, 1.0),
        egui::vec2(0.0, -1.0),
    ] {
        let segment = [center + direction * 10.0, center + direction * 15.0];
        painter.line_segment(segment, egui::Stroke::new(3.0_f32, egui::Color32::BLACK));
        painter.line_segment(segment, egui::Stroke::new(1.0_f32, egui::Color32::WHITE));
    }
    painter.circle_filled(center, 8.0, indicator.color);
    painter.circle_stroke(
        center,
        9.0,
        egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
    );
    painter.circle_stroke(
        center,
        10.0,
        egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
    );
}

fn show_brush_resize_label(ui: &egui::Ui, overlay: BrushResizeLabel, brush_size: f32) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let center = egui::pos2(
        overlay.center[0] / pixels_per_point,
        overlay.center[1] / pixels_per_point,
    );
    let canvas_rect = ui.available_rect_before_wrap();
    if !canvas_rect.contains(center) {
        return;
    }

    let painter = ui.painter().with_clip_rect(canvas_rect);
    let text = format!("{brush_size:.0} px");
    let font = egui::FontId::proportional(16.0);
    let text_width = painter
        .layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    let half_width = overlay.outline_half_width / pixels_per_point;
    let gap = 10.0;
    let right_x = center.x + half_width + gap;
    let (position, align) = if right_x + text_width <= canvas_rect.right() {
        (egui::pos2(right_x, center.y), egui::Align2::LEFT_CENTER)
    } else {
        (
            egui::pos2(center.x - half_width - gap, center.y),
            egui::Align2::RIGHT_CENTER,
        )
    };

    painter.text(
        position + egui::vec2(1.0, 1.0),
        align,
        &text,
        font.clone(),
        egui::Color32::BLACK,
    );
    painter.text(position, align, text, font, egui::Color32::WHITE);
}

fn add_layer_button(ui: &mut egui::Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add layer"));

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let icon_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0));
        egui::Image::new(egui::include_image!("../../assets/icons/plus.svg"))
            .tint(visuals.fg_stroke.color)
            .alt_text("Add layer")
            .paint_at(ui, icon_rect);
    }

    response.on_hover_text("Add layer").clicked()
}

fn show_tool_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    tool: PaintTool,
    selected: bool,
) -> egui::Response {
    let (icon, label, shortcut, accent) = match tool {
        PaintTool::Brush => (
            egui::include_image!("../../assets/icons/paintbrush.svg"),
            "Brush",
            "B",
            egui::Color32::from_rgb(169, 186, 200),
        ),
        PaintTool::Eraser => (
            egui::include_image!("../../assets/icons/eraser.svg"),
            "Eraser",
            "E",
            egui::Color32::from_rgb(213, 170, 109),
        ),
        PaintTool::Smudge => (
            egui::include_image!("../../assets/icons/waves.svg"),
            "Smudge",
            "S",
            egui::Color32::from_rgb(177, 159, 204),
        ),
    };
    let response = ui.interact(rect, ui.id().with(label), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let icon_tint = if selected {
        accent
    } else if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(20.0));
    egui::Image::new(icon)
        .tint(icon_tint)
        .alt_text(label)
        .paint_at(ui, icon_rect);

    response.on_hover_text(format!("{label} ({shortcut})"))
}

const REFERENCE_RESIZE_HANDLE_SIZE: f32 = 14.0;
const REFERENCE_RESIZE_HANDLE_INSET: f32 = 2.0;
const REFERENCE_RESIZE_HIT_SIZE: f32 = 28.0;

fn reference_transform_from_drag_origin(
    position: [f32; 2],
    size: [f32; 2],
    drag: ReferenceDrag,
    delta: [f32; 2],
) -> ([f32; 2], [f32; 2]) {
    match drag {
        ReferenceDrag::Move => ([position[0] + delta[0], position[1] + delta[1]], size),
        ReferenceDrag::Resize => {
            // Project the pointer delta onto the reference diagonal. This keeps the corner as
            // close to the pointer as possible while preserving the original aspect ratio.
            let diagonal_length_squared = size[0] * size[0] + size[1] * size[1];
            let scale = (1.0 + (delta[0] * size[0] + delta[1] * size[1]) / diagonal_length_squared)
                .max(40.0 / size[0].min(size[1]));
            (position, [size[0] * scale, size[1] * scale])
        }
    }
}

fn reference_resize_handle(reference_rect: egui::Rect) -> (egui::Pos2, egui::Rect) {
    let center = reference_rect.right_bottom() - egui::Vec2::splat(REFERENCE_RESIZE_HANDLE_INSET);
    let hit_rect =
        egui::Rect::from_center_size(center, egui::Vec2::splat(REFERENCE_RESIZE_HIT_SIZE));
    (center, hit_rect)
}

fn paint_reference_selection(
    painter: &egui::Painter,
    reference_rect: egui::Rect,
    show_resize_handle: bool,
) {
    let shadow = egui::Color32::from_black_alpha(160);
    let accent = egui::Color32::from_gray(210);
    painter.rect_stroke(
        reference_rect,
        0.0,
        egui::Stroke::new(2.0_f32, shadow),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        reference_rect,
        0.0,
        egui::Stroke::new(1.0_f32, accent),
        egui::StrokeKind::Inside,
    );
    if show_resize_handle {
        let (center, _) = reference_resize_handle(reference_rect);
        let handle_rect =
            egui::Rect::from_center_size(center, egui::Vec2::splat(REFERENCE_RESIZE_HANDLE_SIZE));
        painter.rect_filled(handle_rect, 2.0, shadow);
        painter.rect_filled(handle_rect.shrink(1.0), 1.0, accent);
    }
}

fn window_point_over_rects(point: [f32; 2], pixels_per_point: f32, rects: &[egui::Rect]) -> bool {
    let point = egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point);
    rects.iter().any(|rect| rect.contains(point))
}

fn pointer_over_visible_reference(
    pointer: Option<egui::Pos2>,
    reference_rect: egui::Rect,
    workspace_rect: egui::Rect,
) -> bool {
    pointer
        .is_some_and(|pointer| workspace_rect.contains(pointer) && reference_rect.contains(pointer))
}

fn should_clear_reference_selection(
    has_selection: bool,
    primary_pressed: bool,
    pointer_over_selected_reference: bool,
    pointer_over_ui: bool,
) -> bool {
    has_selection && primary_pressed && !pointer_over_selected_reference && !pointer_over_ui
}

fn tool_index(tool: PaintTool) -> usize {
    match tool {
        PaintTool::Brush => 0,
        PaintTool::Eraser => 1,
        PaintTool::Smudge => 2,
    }
}

fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "elms_sans".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/ElmsSans-Medium.ttf"
        ))),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .expect("default proportional font family")
        .insert(0, "inter".to_owned());
    fonts.families.insert(
        egui::FontFamily::Name("elms_sans".into()),
        vec!["elms_sans".to_owned(), "inter".to_owned()],
    );
    context.set_fonts(fonts);
    context.all_styles_mut(|style| {
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Proportional),
        );
    });
}

fn install_rounded_ui_style(context: &egui::Context) {
    context.all_styles_mut(|style| {
        let dark_mode = style.visuals.dark_mode;
        let visuals = &mut style.visuals;
        visuals.window_corner_radius = egui::CornerRadius::same(16);
        visuals.menu_corner_radius = egui::CornerRadius::same(12);
        visuals.window_fill = if dark_mode {
            egui::Color32::from_rgb(17, 19, 24)
        } else {
            egui::Color32::from_rgb(248, 250, 252)
        };
        // egui uses the window stroke for both the outer border and the title separator.
        // Removing it keeps the title bar visually continuous with the window body.
        visuals.window_stroke = egui::Stroke::NONE;
        visuals.window_highlight_topmost = false;
        visuals.window_shadow = egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 1,
            color: egui::Color32::from_black_alpha(if dark_mode { 105 } else { 48 }),
        };
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 8],
            blur: 22,
            spread: 0,
            color: egui::Color32::from_black_alpha(if dark_mode { 100 } else { 42 }),
        };

        let radius = egui::CornerRadius::same(9);
        visuals.widgets.noninteractive.corner_radius = radius;
        visuals.widgets.inactive.corner_radius = radius;
        visuals.widgets.hovered.corner_radius = radius;
        visuals.widgets.active.corner_radius = radius;
        visuals.widgets.open.corner_radius = radius;
        visuals.widgets.inactive.weak_bg_fill = if dark_mode {
            egui::Color32::from_white_alpha(12)
        } else {
            egui::Color32::from_black_alpha(10)
        };
        visuals.widgets.hovered.weak_bg_fill = if dark_mode {
            egui::Color32::from_white_alpha(28)
        } else {
            egui::Color32::from_white_alpha(150)
        };
        visuals.widgets.active.weak_bg_fill = if dark_mode {
            egui::Color32::from_white_alpha(38)
        } else {
            egui::Color32::from_white_alpha(190)
        };
        visuals.widgets.open.weak_bg_fill = visuals.widgets.hovered.weak_bg_fill;
        visuals.slider_trailing_fill = true;
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    });
}

fn paint_rounded_panel(ui: &egui::Ui, rect: egui::Rect, corner_radius: egui::CornerRadius) {
    ui.painter()
        .rect_filled(rect, corner_radius, ui.visuals().window_fill());
}

fn brush_settings_from_config(
    config: &CurrentBrushConfig,
    loaded: &LoadedBrushPreset,
) -> BrushSettings {
    let preset = &loaded.preset;
    BrushSettings {
        color: brush_color(config),
        size: config.size.clamp(preset.size.min, preset.size.max),
        pressure: PressureSettings {
            min_size: preset.pressure.min_size,
            min_opacity: preset.pressure.min_opacity,
            full_opacity_pressure: preset.pressure.full_opacity_pressure,
            opacity_gamma: preset.pressure.opacity_gamma,
        },
        spacing: BrushSpacing {
            ratio: preset.spacing.ratio,
            minimum: preset.spacing.minimum,
        },
    }
}

fn brush_color(config: &CurrentBrushConfig) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        config.color[0],
        config.color[1],
        config.color[2],
        config.color[3],
    )
}

fn background_color(color: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (color[0] * 255.0).round() as u8,
        (color[1] * 255.0).round() as u8,
        (color[2] * 255.0).round() as u8,
    )
}

fn rgb(color: egui::Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

pub fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |viewport| viewport.repaint_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_dialog_hides_technical_details() {
        let dialog = MessageDialog::error(
            "Chromazen couldn’t export the PNG.",
            "renderer unavailable at adapter 0",
        );

        assert_eq!(dialog.title, "Something went wrong");
        assert_eq!(dialog.message, "Chromazen couldn’t export the PNG.");
        assert!(!dialog.message.contains("adapter 0"));
    }

    #[test]
    fn crop_handles_have_large_targets_and_entire_edges_are_draggable() {
        let corners = [
            egui::pos2(0.0, 0.0),
            egui::pos2(400.0, 0.0),
            egui::pos2(400.0, 200.0),
            egui::pos2(0.0, 200.0),
        ];
        let handles = canvas_crop_handle_positions(corners);
        let rect = CanvasCropRect {
            min: [0.0, 0.0],
            max: [400.0, 200.0],
        };

        assert_eq!(
            canvas_crop_handle_at(egui::pos2(-18.0, -18.0), [-18.0, -18.0], rect, &handles),
            Some(CanvasCropHandle::TopLeft)
        );
        assert_eq!(
            canvas_crop_handle_at(egui::pos2(200.0, 20.0), [200.0, 20.0], rect, &handles),
            Some(CanvasCropHandle::Top)
        );
        assert_eq!(
            canvas_crop_handle_at(egui::pos2(100.0, 10.0), [100.0, 10.0], rect, &handles),
            Some(CanvasCropHandle::Top)
        );
        assert_eq!(
            canvas_crop_handle_at(egui::pos2(200.0, 100.0), [200.0, 100.0], rect, &handles),
            Some(CanvasCropHandle::Move)
        );
    }

    #[test]
    fn transform_handles_move_scale_and_rotate_from_the_drag_origin() {
        let start = LayerTransform {
            translation: [10.0, -5.0],
            scale: [2.0, 2.0],
            rotation: 0.0,
        };
        let bounds = LayerContentBounds {
            min: [20.0, 10.0],
            max: [80.0, 90.0],
        };
        let drag = |handle, start_pointer| LayerTransformDrag {
            handle,
            start_transform: start,
            start_pointer,
        };

        assert_eq!(
            layer_transform_from_drag(
                drag(LayerTransformHandle::Move, [20.0, 30.0]),
                [35.0, 5.0],
                bounds,
                false,
            )
            .translation,
            [25.0, -30.0]
        );
        assert_eq!(
            layer_transform_from_drag(
                drag(
                    LayerTransformHandle::Scale(CanvasCropHandle::TopRight),
                    [120.0, -35.0],
                ),
                [150.0, -15.0],
                bounds,
                false,
            )
            .scale,
            [3.0, 1.5]
        );
        assert_eq!(
            layer_transform_from_drag(
                drag(
                    LayerTransformHandle::Scale(CanvasCropHandle::TopRight),
                    [120.0, -35.0],
                ),
                [180.0, -115.0],
                bounds,
                true,
            )
            .scale,
            [4.0, 4.0]
        );
        let rotated = layer_transform_from_drag(
            drag(LayerTransformHandle::Rotate, [110.0, 45.0]),
            [60.0, 95.0],
            bounds,
            false,
        );
        assert!((rotated.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
    }

    #[test]
    fn crop_handles_resize_selected_edges_in_document_coordinates() {
        let start_rect = CanvasCropRect {
            min: [10.0, 20.0],
            max: [110.0, 220.0],
        };
        let drag = CanvasCropDrag {
            handle: CanvasCropHandle::TopLeft,
            start_rect,
            start_pointer: [10.0, 20.0],
        };
        assert_eq!(
            canvas_crop_rect_from_drag(drag, [-15.0, 45.0]),
            CanvasCropRect {
                min: [-15.0, 45.0],
                max: [110.0, 220.0],
            }
        );
    }

    #[test]
    fn moving_crop_preserves_its_dimensions() {
        let start_rect = CanvasCropRect {
            min: [10.0, 20.0],
            max: [110.0, 220.0],
        };
        let drag = CanvasCropDrag {
            handle: CanvasCropHandle::Move,
            start_rect,
            start_pointer: [30.0, 40.0],
        };
        assert_eq!(
            canvas_crop_rect_from_drag(drag, [45.0, 10.0]),
            CanvasCropRect {
                min: [25.0, -10.0],
                max: [125.0, 190.0],
            }
        );
    }

    #[test]
    fn crop_request_rounds_to_pixels_and_validates_renderer_limits() {
        let constraints = CanvasSizeConstraints {
            max_dimension: 8192,
            max_pixels: 32 * 1024 * 1024,
        };
        assert_eq!(
            canvas_crop_request(
                CanvasCropRect {
                    min: [-10.4, 20.4],
                    max: [90.6, 220.6],
                },
                constraints,
            ),
            Ok(CanvasCropRequest {
                size: [101, 201],
                origin: [-10, 20],
            })
        );
        assert!(
            canvas_crop_request(
                CanvasCropRect {
                    min: [0.0, 0.0],
                    max: [9000.0, 10.0],
                },
                constraints,
            )
            .is_err()
        );
    }

    #[test]
    fn background_color_round_trips_through_ui() {
        let color = [0.25, 0.5, 0.75, 1.0];
        assert_eq!(rgb(background_color(color)), [64, 128, 191]);
    }

    #[test]
    fn replaced_layer_resource_invalidates_cached_preview() {
        let cached = LayerPreviewKey {
            layer_id: LayerId(1),
            resource_id: LayerResourceId(4),
        };
        let replacement = LayerPreviewKey {
            layer_id: LayerId(1),
            resource_id: LayerResourceId(5),
        };
        assert!(!layer_preview_is_current(&[replacement], cached));
    }

    #[test]
    fn unchanged_layer_resource_keeps_cached_preview() {
        let cached = LayerPreviewKey {
            layer_id: LayerId(2),
            resource_id: LayerResourceId(7),
        };
        assert!(layer_preview_is_current(&[cached], cached));
    }

    #[test]
    fn reference_transform_uses_total_drag_delta_from_the_origin() {
        let position = [10.0, 20.0];
        let size = [100.0, 50.0];

        let (_, resized) = reference_transform_from_drag_origin(
            position,
            size,
            ReferenceDrag::Resize,
            [25.0, 20.0],
        );
        assert!((resized[0] - 128.0).abs() < 0.0001);
        assert!((resized[1] - 64.0).abs() < 0.0001);
        assert!((resized[0] / resized[1] - size[0] / size[1]).abs() < 0.0001);
        assert_eq!(
            reference_transform_from_drag_origin(position, size, ReferenceDrag::Move, [50.0, 25.0],),
            ([60.0, 45.0], size),
        );
    }

    #[test]
    fn resize_handle_overlaps_the_corner_with_a_larger_invisible_hit_target() {
        let reference = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(120.0, 130.0));
        let (center, hit_rect) = reference_resize_handle(reference);

        assert_eq!(center, egui::pos2(118.0, 128.0));
        assert_eq!(hit_rect.min, egui::pos2(104.0, 114.0));
        assert_eq!(hit_rect.max, egui::pos2(132.0, 142.0));
    }

    #[test]
    fn physical_window_points_hit_cached_reference_rects() {
        let rects = [egui::Rect::from_min_max(
            egui::pos2(20.0, 30.0),
            egui::pos2(120.0, 130.0),
        )];

        assert!(window_point_over_rects([100.0, 120.0], 2.0, &rects));
        assert!(!window_point_over_rects([10.0, 10.0], 2.0, &rects));
    }

    #[test]
    fn reference_hover_is_limited_to_the_visible_workspace() {
        let reference = egui::Rect::from_min_max(egui::pos2(80.0, 20.0), egui::pos2(140.0, 80.0));
        let workspace = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0));

        assert!(pointer_over_visible_reference(
            Some(egui::pos2(90.0, 50.0)),
            reference,
            workspace,
        ));
        assert!(!pointer_over_visible_reference(
            Some(egui::pos2(120.0, 50.0)),
            reference,
            workspace,
        ));
    }

    #[test]
    fn canvas_press_clears_reference_selection_without_affecting_ui_presses() {
        assert!(should_clear_reference_selection(true, true, false, false));
        assert!(!should_clear_reference_selection(true, true, true, false));
        assert!(!should_clear_reference_selection(true, true, false, true));
        assert!(!should_clear_reference_selection(false, true, false, false));
    }
}
