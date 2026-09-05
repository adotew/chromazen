mod brush_panel;
mod brush_preview;
mod canvas_crop;
mod color_picker;
mod dialogs;
mod editor;
mod gallery;
mod interaction_geometry;
mod layer_transform;
mod layers_panel;
mod menu;
mod reference_panel;
mod toolbar;

use interaction_geometry::*;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    artwork::ArtworkSummary,
    config::{AppConfig, BrushCatalog, CurrentBrushConfig, LoadedBrushPreset, PanelLayout},
    paint::{BrushSettings, BrushSpacing, PaintTool, PressureSettings},
    renderer::{
        CanvasSizeConstraints, DEFAULT_CANVAS_SIZE, DropEdge, LayerContentBounds, LayerId,
        LayerResourceId, LayerSnapshot, LayerTransform, PaintRenderer, PaintViewSnapshot,
        merge_down_target_index,
    },
};

#[cfg(not(target_os = "macos"))]
use super::command::UiCommand;
use super::{
    autosave::SaveStatus,
    command::{AppCommand, EditorCommand, NavigationCommand, SettingsCommand},
    input::EditorTool,
    references::{ReferenceId, ReferenceImage},
};

const SIDEBAR_WIDTH: f32 = 300.0;
const TOOL_RAIL_THICKNESS: f32 = 42.0;
const BRUSH_PANEL_WIDTH: f32 = 240.0;
const BRUSH_PRESET_LIST_MAX_HEIGHT: f32 = 320.0;
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
const FLIP_SHORTCUT: &str = "F / Shift-F";
#[cfg(target_os = "macos")]
const CROP_SHORTCUT: &str = "⌘-Option-C";
#[cfg(not(target_os = "macos"))]
const CROP_SHORTCUT: &str = "Ctrl-Alt-C";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApplicationMenuState {
    pub(crate) document_enabled: bool,
    pub(crate) can_undo: bool,
    pub(crate) can_redo: bool,
    pub(crate) canvas_enabled: bool,
    pub(crate) export_enabled: bool,
}

impl ApplicationMenuState {
    pub(crate) fn new(
        in_editor: bool,
        canvas_crop_active: bool,
        can_undo: bool,
        can_redo: bool,
        export_in_progress: bool,
    ) -> Self {
        Self {
            document_enabled: in_editor,
            can_undo: in_editor && !canvas_crop_active && can_undo,
            can_redo: in_editor && !canvas_crop_active && can_redo,
            canvas_enabled: in_editor && !canvas_crop_active,
            export_enabled: in_editor && !export_in_progress,
        }
    }
}

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
    pub(crate) menu: ApplicationMenuState,
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
    tool_opacities: [f32; 3],
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
    sidebar_visible: bool,
    brush_window_open: bool,
    color_window_open: bool,
    layers_window_open: bool,
    panel_layout: PanelLayout,
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
    restore_sidebar: bool,
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
            tool_opacities: [
                config.opacity_for_tool(PaintTool::Brush),
                config.opacity_for_tool(PaintTool::Eraser),
                config.opacity_for_tool(PaintTool::Smudge),
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
            sidebar_visible: true,
            brush_window_open: false,
            color_window_open: false,
            layers_window_open: false,
            panel_layout: config.panel_layout,
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

    pub(crate) fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
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

    pub(crate) fn settings_for_save(
        &self,
    ) -> (
        CurrentBrushConfig,
        [String; 3],
        [f32; 3],
        [f32; 3],
        PanelLayout,
    ) {
        let mut brush = self.current_brush_config();
        brush.size = self.tool_sizes[tool_index(PaintTool::Brush)];
        brush.opacity = self.tool_opacities[tool_index(PaintTool::Brush)];
        (
            brush,
            self.tool_brushes.clone(),
            self.tool_sizes,
            self.tool_opacities,
            self.panel_layout,
        )
    }

    pub(crate) fn brush_for_tool(&self, tool: PaintTool) -> &str {
        &self.tool_brushes[tool_index(tool)]
    }

    pub(crate) fn store_current_brush_settings_for_tool(&mut self, tool: PaintTool) {
        let index = tool_index(tool);
        self.tool_sizes[index] = self.brush.size;
        self.tool_opacities[index] = self.brush.opacity;
    }

    pub(crate) fn set_brush_color(&mut self, color: [u8; 4]) {
        self.brush.color =
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    }

    pub(crate) fn reset_active_brush_settings(&mut self) {
        self.brush.size = self.default_size;
        self.brush.opacity = 1.0;
        self.brush.color = brush_color(&CurrentBrushConfig::default());
        self.context.request_repaint();
    }

    pub fn current_brush_config(&self) -> CurrentBrushConfig {
        CurrentBrushConfig {
            size: self.brush.size,
            opacity: self.brush.opacity,
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
        self.brush.opacity = self.tool_opacities[index];
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

    pub(crate) fn apply_reloaded_settings(&mut self, config: &AppConfig, active_tool: PaintTool) {
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
        self.tool_opacities = [
            config.opacity_for_tool(PaintTool::Brush),
            config.opacity_for_tool(PaintTool::Eraser),
            config.opacity_for_tool(PaintTool::Smudge),
        ];
        let index = tool_index(active_tool);
        self.brush.color = brush_color(&config.brush);
        self.brush.size =
            self.tool_sizes[index].clamp(*self.size_range.start(), *self.size_range.end());
        self.brush.opacity = self.tool_opacities[index];
        self.panel_layout = config.panel_layout;
        self.context.request_repaint();
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
                    commands.push(AppCommand::Editor(EditorCommand::SaveArtwork));
                }
                if ui.button("Cancel").clicked() {
                    commands.push(AppCommand::Navigation(NavigationCommand::CancelPending));
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

fn show_brush_resize_label(
    ui: &egui::Ui,
    overlay: BrushResizeLabel,
    brush_size: f32,
    brush_opacity: f32,
) {
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
    let text = format!("{brush_size:.0} px · {:.0}%", brush_opacity * 100.0);
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
            "D / B",
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

fn reference_resize_handle_geometry(reference_rect: egui::Rect) -> (egui::Pos2, egui::Rect) {
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
        let (center, _) = reference_resize_handle_geometry(reference_rect);
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
        apply_button_padding(style);
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

fn apply_button_padding(style: &mut egui::Style) {
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
}

fn apply_menu_item_padding(ui: &mut egui::Ui) {
    apply_button_padding(ui.style_mut());
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
        opacity: config.opacity,
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
    fn application_menu_disables_document_actions_in_gallery() {
        let state = ApplicationMenuState::new(false, false, true, true, false);

        assert!(!state.document_enabled);
        assert!(!state.can_undo);
        assert!(!state.can_redo);
        assert!(!state.canvas_enabled);
        assert!(!state.export_enabled);
    }

    #[test]
    fn application_menu_respects_transient_editor_restrictions() {
        let cropping = ApplicationMenuState::new(true, true, true, true, false);
        assert!(cropping.document_enabled);
        assert!(!cropping.can_undo);
        assert!(!cropping.can_redo);
        assert!(!cropping.canvas_enabled);
        assert!(cropping.export_enabled);

        let exporting = ApplicationMenuState::new(true, false, true, false, true);
        assert!(exporting.can_undo);
        assert!(!exporting.can_redo);
        assert!(exporting.canvas_enabled);
        assert!(!exporting.export_enabled);
    }

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
        let (center, hit_rect) = reference_resize_handle_geometry(reference);

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
