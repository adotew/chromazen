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
    paint::{BrushSettings, BrushSpacing, PaintTool, PressureSettings, StrokeSmoothingOptions},
    renderer::{
        CanvasSizeConstraints, DEFAULT_CANVAS_SIZE, DropEdge, LayerId, LayerResourceId,
        LayerSnapshot, PaintRenderer,
    },
};

use super::{autosave::SaveStatus, command::AppCommand};

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
    pub(crate) tool: PaintTool,
    pub(crate) brush_resize_label: Option<BrushResizeLabel>,
    pub(crate) eyedropper_indicator: Option<EyedropperIndicator>,
    pub(crate) save_status: SaveStatus,
    pub(crate) pending_navigation: Option<&'a str>,
}

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
    tool_brushes: [String; 3],
    tool_sizes: [f32; 3],
    brushes: Vec<crate::config::BrushSummary>,
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    commands: Vec<AppCommand>,
    settings_message: Option<SettingsMessage>,
    background_edit_start: Option<[u8; 3]>,
    layer_name_edit: Option<LayerNameEdit>,
    layer_opacity_edit: Option<LayerOpacityEdit>,
    layer_thumbnails: Vec<LayerThumbnail>,
    brush_previews: Vec<(String, egui::TextureHandle)>,
    failed_brush_previews: Vec<String>,
    sidebar_visible: bool,
    canvas_size_constraints: CanvasSizeConstraints,
    new_artwork_dialog: Option<NewArtworkDialog>,
    gallery: gallery::GalleryUi,
}

struct SettingsMessage {
    text: String,
    is_error: bool,
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

        Self {
            context,
            state,
            renderer,
            brush: brush_settings_from_config(&config.brush, brush_preset),
            stroke_smoothing: StrokeSmoothingOptions {
                strength: config.smoothing.strength,
            },
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
            settings_message: load_error.map(|text| SettingsMessage {
                text,
                is_error: true,
            }),
            background_edit_start: None,
            layer_name_edit: None,
            layer_opacity_edit: None,
            layer_thumbnails: Vec::new(),
            brush_previews: Vec::new(),
            failed_brush_previews: Vec::new(),
            sidebar_visible: true,
            canvas_size_constraints: paint.canvas_size_constraints(),
            new_artwork_dialog: None,
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
        color_picker::show(ui, &mut self.brush.color);
        ui.add_space(8.0);
    }

    fn show_tool_rail(&mut self, ui: &mut egui::Ui, active_tool: PaintTool) -> Option<PaintTool> {
        const RAIL_WIDTH: f32 = 42.0;
        const TOOL_HEIGHT: f32 = 40.0;
        const VERTICAL_PADDING: f32 = 6.0;

        let tools = [PaintTool::Brush, PaintTool::Eraser, PaintTool::Smudge];
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                RAIL_WIDTH,
                TOOL_HEIGHT * tools.len() as f32 + 2.0 * VERTICAL_PADDING,
            ),
            egui::Sense::hover(),
        );
        let separator_width = ui.visuals().widgets.noninteractive.bg_stroke.width;
        let panel_rect = rect.with_max_x(rect.right() + separator_width);
        ui.painter().rect_filled(
            panel_rect,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 12,
                se: 0,
            },
            ui.visuals().panel_fill,
        );
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + VERTICAL_PADDING),
            egui::pos2(rect.right(), rect.bottom() - VERTICAL_PADDING),
        );

        let mut selected_tool = None;
        for (index, tool) in tools.into_iter().enumerate() {
            let tool_rect = egui::Rect::from_min_size(
                egui::pos2(body.left(), body.top() + index as f32 * TOOL_HEIGHT),
                egui::vec2(RAIL_WIDTH, TOOL_HEIGHT),
            );
            let response = show_tool_button(ui, tool_rect, tool, tool == active_tool);
            if tool != active_tool {
                if response.clicked() {
                    egui::Popup::close_all(ui.ctx());
                    selected_tool = Some(tool);
                }
                continue;
            }

            let popup_id = egui::Popup::default_response_id(&response);
            let selected_brush = self.tool_brushes[tool_index(tool)].clone();
            let brushes = self.brushes.clone();
            egui::Popup::menu(&response)
                .align(egui::RectAlign::LEFT_START)
                .width(300.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for brush in &brushes {
                                let selected = brush.id == selected_brush;
                                let preview = self.brush_preview_texture(&brush.id);
                                if show_brush_row(ui, &brush.name, preview, selected).clicked() {
                                    if !selected {
                                        self.commands.push(AppCommand::SwitchBrush {
                                            tool,
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
        selected_tool
    }

    pub fn run_editor(&mut self, window: &Window, state: EditorUiState<'_>) -> egui::FullOutput {
        let EditorUiState {
            layers,
            tool,
            brush_resize_label,
            eyedropper_indicator,
            save_status,
            pending_navigation,
        } = state;
        self.tool_sizes[tool_index(tool)] = self.brush.size;
        self.load_brush_preview(&self.tool_brushes[tool_index(tool)].clone());
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        context.run_ui(raw_input, |ui| {
            let background = background_color(layers.background_color);

            // egui's built-in animated panel deliberately hides its contents while resizing,
            // which makes a wide sidebar flash empty. Keep a full-width child clipped to an
            // animated panel instead, so the complete sidebar slides off the right edge.
            const SIDEBAR_WIDTH: f32 = 300.0;
            let sidebar_progress = ui.ctx().animate_bool_with_time_and_easing(
                egui::Id::new("sidebar animation"),
                self.sidebar_visible,
                0.18,
                egui::emath::easing::cubic_in_out,
            );
            if sidebar_progress > 0.0 {
                egui::Panel::right("tools")
                    .exact_size(SIDEBAR_WIDTH * sidebar_progress)
                    .resizable(false)
                    .show_inside(ui, |panel_ui| {
                        let inner_width = SIDEBAR_WIDTH
                            - egui::Frame::side_top_panel(panel_ui.style())
                                .inner_margin
                                .sum()
                                .x;
                        let content_rect = egui::Rect::from_min_size(
                            panel_ui.min_rect().min,
                            egui::vec2(inner_width, panel_ui.available_height()),
                        );
                        let mut content_ui = panel_ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("sidebar contents")
                                .max_rect(content_rect),
                        );
                        content_ui.set_clip_rect(panel_ui.clip_rect());
                        let ui = &mut content_ui;

                        self.show_brush_controls(ui);

                        if let Some(message) = &self.settings_message {
                            let color = if message.is_error {
                                egui::Color32::LIGHT_RED
                            } else {
                                egui::Color32::LIGHT_GREEN
                            };
                            ui.colored_label(color, &message.text);
                        }

                        ui.separator();
                        egui::Panel::bottom("layer controls")
                            .show_separator_line(false)
                            .show_inside(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let add_icon = egui::Image::new(egui::include_image!(
                                        "../../assets/icons/plus.svg"
                                    ))
                                    .fit_to_exact_size(egui::Vec2::splat(16.0))
                                    .alt_text("Add layer");
                                    let add_button = egui::Button::image(add_icon)
                                        .image_tint_follows_text_color(true)
                                        .min_size(egui::Vec2::splat(28.0))
                                        .corner_radius(8);
                                    if ui.add(add_button).on_hover_text("Add layer").clicked() {
                                        self.commands.push(AppCommand::AddLayer);
                                    }

                                    let can_delete = layers.layers.len() > 1;
                                    let delete_icon = egui::Image::new(egui::include_image!(
                                        "../../assets/icons/trash-2.svg"
                                    ))
                                    .fit_to_exact_size(egui::Vec2::splat(16.0))
                                    .alt_text("Delete layer");
                                    let delete_button = egui::Button::image(delete_icon)
                                        .image_tint_follows_text_color(true)
                                        .min_size(egui::Vec2::splat(28.0))
                                        .corner_radius(8);
                                    if ui
                                        .add_enabled(can_delete, delete_button)
                                        .on_hover_text("Delete layer")
                                        .clicked()
                                    {
                                        self.commands.push(AppCommand::DeleteSelectedLayer);
                                    }
                                });
                                ui.add_space(6.0);
                            });
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .id_salt("layer list")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for layer in layers.layers.iter().rev() {
                                    let selected = layers.selection == layer.id;
                                    let thumbnail = self
                                        .layer_thumbnails
                                        .iter()
                                        .find(|thumbnail| thumbnail.key.layer_id == layer.id)
                                        .map(|thumbnail| thumbnail.texture_id);
                                    let drag = ui.dnd_drag_source(
                                        egui::Id::new(("layer drag", layer.id.0)),
                                        layer.id,
                                        |ui| {
                                            show_layer_row(
                                                ui,
                                                &layer.name,
                                                selected,
                                                thumbnail,
                                                None,
                                                Some(layer.visible),
                                            )
                                        },
                                    );
                                    let row = drag.inner;
                                    if let (Some(pointer), Some(dragged)) = (
                                        ui.input(|input| input.pointer.interact_pos()),
                                        drag.response.dnd_hover_payload::<LayerId>(),
                                    ) && *dragged != layer.id
                                    {
                                        let edge = if pointer.y < drag.response.rect.center().y {
                                            DropEdge::Above
                                        } else {
                                            DropEdge::Below
                                        };
                                        let y = match edge {
                                            DropEdge::Above => drag.response.rect.top(),
                                            DropEdge::Below => drag.response.rect.bottom(),
                                        };
                                        ui.painter().hline(
                                            drag.response.rect.x_range().shrink(8.0),
                                            y,
                                            egui::Stroke::new(
                                                1.5_f32,
                                                ui.visuals().selection.stroke.color,
                                            ),
                                        );
                                        if let Some(dragged) =
                                            drag.response.dnd_release_payload::<LayerId>()
                                        {
                                            self.commands.push(AppCommand::MoveLayer {
                                                dragged: *dragged,
                                                target: layer.id,
                                                edge,
                                            });
                                        }
                                    }
                                    if row.visibility.as_ref().is_some_and(|eye| eye.clicked()) {
                                        self.commands.push(AppCommand::SetLayerVisibility {
                                            id: layer.id,
                                            visible: !layer.visible,
                                        });
                                    } else if row.row.clicked() && !selected {
                                        self.commands.push(AppCommand::SelectLayer(layer.id));
                                    }

                                    if selected {
                                        let properties_popup_id =
                                            egui::Popup::default_response_id(&row.row);
                                        if !egui::Popup::is_id_open(ui.ctx(), properties_popup_id)
                                            || self
                                                .layer_name_edit
                                                .as_ref()
                                                .is_none_or(|edit| edit.id != layer.id)
                                        {
                                            self.layer_name_edit = Some(LayerNameEdit {
                                                id: layer.id,
                                                name: layer.name.clone(),
                                            });
                                        }
                                        let mut edited_name =
                                            self.layer_name_edit.as_ref().map_or_else(
                                                || layer.name.clone(),
                                                |edit| edit.name.clone(),
                                            );
                                        let mut opacity = layer.opacity;
                                        let mut commit_name = false;
                                        let mut cancel_name = false;
                                        let mut opacity_changed = false;
                                        egui::Popup::from_toggle_button_response(&row.row)
                                            .width(240.0)
                                            .close_behavior(
                                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                            )
                                            .show(|ui| {
                                                egui::Grid::new(("layer properties", layer.id.0))
                                                    .num_columns(2)
                                                    .spacing([10.0, 8.0])
                                                    .show(ui, |ui| {
                                                        ui.label("Name");
                                                        let response = ui.add(
                                                            egui::TextEdit::singleline(
                                                                &mut edited_name,
                                                            )
                                                            .desired_width(150.0),
                                                        );
                                                        let enter = response.has_focus()
                                                            && ui.input(|input| {
                                                                input.key_pressed(egui::Key::Enter)
                                                            });
                                                        cancel_name = response.has_focus()
                                                            && ui.input(|input| {
                                                                input.key_pressed(egui::Key::Escape)
                                                            });
                                                        commit_name = !cancel_name
                                                            && (enter || response.lost_focus());
                                                        ui.end_row();

                                                        ui.label("Opacity");
                                                        opacity_changed = ui
                                                            .add(
                                                                egui::Slider::new(
                                                                    &mut opacity,
                                                                    0..=100,
                                                                )
                                                                .suffix("%")
                                                                .show_value(true),
                                                            )
                                                            .changed();
                                                        ui.end_row();
                                                    });
                                            });
                                        if cancel_name {
                                            edited_name.clone_from(&layer.name);
                                        } else if commit_name && !edited_name.trim().is_empty() {
                                            self.commands.push(AppCommand::RenameLayer {
                                                id: layer.id,
                                                name: edited_name.clone(),
                                            });
                                        }
                                        if let Some(edit) = self.layer_name_edit.as_mut() {
                                            edit.name = edited_name;
                                        }
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
                                    "Background",
                                    false,
                                    None,
                                    Some(background),
                                    None,
                                );
                                egui::Popup::from_toggle_button_response(&response.row)
                                    .width(220.0)
                                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                    .show(|ui| {
                                        if color_picker::show(ui, &mut color) {
                                            self.background_edit_start
                                                .get_or_insert(rgb(background));
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
                    });
            }

            let selected_tool = egui::Area::new(egui::Id::new("tool rail"))
                .anchor(
                    egui::Align2::RIGHT_TOP,
                    egui::vec2(-SIDEBAR_WIDTH * sidebar_progress, 0.0),
                )
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
            self.show_new_artwork_dialog(ui.ctx());
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
        let settings_message = self
            .settings_message
            .as_ref()
            .filter(|message| message.is_error)
            .map(|message| message.text.as_str());
        let warning = match (discovery_warning, settings_message) {
            (Some(discovery), Some(message)) => Some(format!("{discovery}\n{message}")),
            (Some(discovery), None) => Some(discovery.to_owned()),
            (None, Some(message)) => Some(message.to_owned()),
            (None, None) => None,
        };
        context.run_ui(raw_input, |ui| {
            self.gallery
                .show(ui, artworks, warning.as_deref(), &mut self.commands);
            self.show_new_artwork_dialog(ui.ctx());
        })
    }

    pub(crate) fn open_new_artwork_dialog(&mut self) {
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

    pub(crate) fn reset_brush(&mut self) {
        self.brush.size = self.default_size;
        self.brush.color = brush_color(&CurrentBrushConfig::default());
        self.settings_message = None;
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
        self.stroke_smoothing.strength = config.smoothing.strength;
        self.settings_message = None;
        self.context.request_repaint();
    }

    pub(crate) fn show_error(&mut self, error: impl Into<String>) {
        self.show_message(error, true);
    }

    pub(crate) fn show_success(&mut self, message: impl Into<String>) {
        self.show_message(message, false);
    }

    fn show_message(&mut self, text: impl Into<String>, is_error: bool) {
        self.settings_message = Some(SettingsMessage {
            text: text.into(),
            is_error,
        });
        self.context.request_repaint();
    }
}

fn layer_preview_is_current(current: &[LayerPreviewKey], cached: LayerPreviewKey) -> bool {
    current.contains(&cached)
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

fn show_brush_row(
    ui: &mut egui::Ui,
    name: &str,
    texture_id: Option<egui::TextureId>,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), egui::Sense::click());
    let dark_mode = ui.visuals().dark_mode;
    let fill = if selected {
        egui::Color32::from_gray(if dark_mode { 58 } else { 224 })
    } else if response.hovered() {
        egui::Color32::from_gray(if dark_mode { 42 } else { 240 })
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = selected.then(|| {
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_gray(if dark_mode { 110 } else { 155 }),
        )
    });
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        10,
        fill,
        stroke.unwrap_or(egui::Stroke::NONE),
        egui::StrokeKind::Inside,
    );

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

struct LayerRowResponse {
    row: egui::Response,
    visibility: Option<egui::Response>,
}

fn show_layer_row(
    ui: &mut egui::Ui,
    name: &str,
    selected: bool,
    texture_id: Option<egui::TextureId>,
    solid_color: Option<egui::Color32>,
    visible: Option<bool>,
) -> LayerRowResponse {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.0), egui::Sense::click());
    let visibility = visible.map(|_| {
        let eye_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            egui::Vec2::splat(24.0),
        );
        ui.interact(
            eye_rect,
            response.id.with("visibility"),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    let visuals = ui.style().interact(&response);
    let dark_mode = ui.visuals().dark_mode;
    let fill = if selected {
        egui::Color32::from_gray(if dark_mode { 58 } else { 224 })
    } else if response.hovered() {
        egui::Color32::from_gray(if dark_mode { 42 } else { 240 })
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if selected {
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_gray(if dark_mode { 110 } else { 155 }),
        )
    } else {
        egui::Stroke::NONE
    };
    let painter = ui.painter();
    painter.rect(rect, 12, fill, stroke, egui::StrokeKind::Inside);

    if let (Some(visible), Some(visibility)) = (visible, &visibility) {
        let icon = if visible {
            egui::include_image!("../../assets/icons/eye.svg")
        } else {
            egui::include_image!("../../assets/icons/eye-off.svg")
        };
        let alpha = if !visible || selected || visibility.hovered() {
            220
        } else {
            90
        };
        egui::Image::new(icon)
            .fit_to_exact_size(egui::Vec2::splat(16.0))
            .tint(egui::Color32::from_white_alpha(alpha))
            .paint_at(ui, visibility.rect.shrink(4.0));
    }

    let thumbnail =
        egui::Rect::from_min_size(rect.min + egui::vec2(34.0, 6.0), egui::Vec2::splat(48.0));
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
            let tint = if visible == Some(false) {
                egui::Color32::from_white_alpha(100)
            } else {
                egui::Color32::WHITE
            };
            egui::Image::new((texture_id, thumbnail.size()))
                .corner_radius(8)
                .tint(tint)
                .paint_at(ui, thumbnail);
        }
    }
    let text_color = if visible == Some(false) {
        ui.visuals().weak_text_color()
    } else {
        visuals.text_color()
    };
    painter.text(
        egui::pos2(thumbnail.max.x + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::TextStyle::Button.resolve(ui.style()),
        text_color,
    );

    LayerRowResponse {
        row: response.on_hover_cursor(egui::CursorIcon::PointingHand),
        visibility,
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
        "inter_medium".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Medium.ttf"
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
        egui::FontFamily::Name("inter_medium".into()),
        vec!["inter_medium".to_owned(), "inter".to_owned()],
    );
    fonts.families.insert(
        egui::FontFamily::Name("elms_sans".into()),
        vec!["elms_sans".to_owned(), "inter".to_owned()],
    );
    context.set_fonts(fonts);
    context.all_styles_mut(|style| {
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Name("inter_medium".into())),
        );
    });
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
}
