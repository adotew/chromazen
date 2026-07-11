mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    config::{AppConfig, BrushCatalog, BrushPreset, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PressureSettings, StrokeSmoothingOptions},
    renderer::PaintRenderer,
};

pub(crate) enum GuiAction {
    SwitchBrush(String),
    SavePreset(BrushPreset),
    SavePresetAs { id: String, preset: BrushPreset },
    DeletePreset,
    ReloadFromDisk,
    OpenConfigDirectory,
}

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
    saved_brush: CurrentBrushConfig,
    saved_active_brush: String,
    active_brush: String,
    brushes: Vec<crate::config::BrushSummary>,
    preset: BrushPreset,
    saved_preset: BrushPreset,
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    default_smoothing: StrokeSmoothingOptions,
    save_requested: bool,
    action: Option<GuiAction>,
    show_save_as: bool,
    save_as_id: String,
    confirm_delete: bool,
    settings_message: Option<SettingsMessage>,
}

struct SettingsMessage {
    text: String,
    is_error: bool,
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

        let brush = brush_settings_from_config(&config.brush, brush_preset);
        let preset = &brush_preset.preset;
        let default_smoothing = StrokeSmoothingOptions {
            enabled: preset.smoothing.enabled,
            strength: preset.smoothing.strength,
        };
        Self {
            context,
            state,
            renderer,
            brush,
            stroke_smoothing: default_smoothing,
            saved_brush: config.brush.clone(),
            saved_active_brush: config.active_brush.clone(),
            active_brush: brush_preset.id.clone(),
            brushes: catalog.brushes,
            preset: preset.clone(),
            saved_preset: preset.clone(),
            size_range: preset.size.min..=preset.size.max,
            default_size: preset.size.default,
            default_smoothing,
            save_requested: false,
            action: None,
            show_save_as: false,
            save_as_id: String::new(),
            confirm_delete: false,
            settings_message: load_error.map(|text| SettingsMessage {
                text,
                is_error: true,
            }),
        }
    }

    pub fn run(&mut self, window: &Window) -> egui::FullOutput {
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        let output = context.run_ui(raw_input, |ui| {
            egui::Window::new("Brush")
                .default_pos([12.0, 12.0])
                .default_width(280.0)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    let selected_name = self
                        .brushes
                        .iter()
                        .find(|brush| brush.id == self.active_brush)
                        .map_or(self.active_brush.as_str(), |brush| brush.name.as_str());
                    egui::ComboBox::from_label("Preset")
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for brush in &self.brushes {
                                if ui
                                    .selectable_label(brush.id == self.active_brush, &brush.name)
                                    .clicked()
                                    && brush.id != self.active_brush
                                {
                                    self.action = Some(GuiAction::SwitchBrush(brush.id.clone()));
                                }
                            }
                        });

                    ui.add(
                        egui::Slider::new(&mut self.brush.size, self.size_range.clone())
                            .suffix(" px"),
                    );
                    ui.checkbox(&mut self.stroke_smoothing.enabled, "Stroke smoothing");
                    ui.add_enabled(
                        self.stroke_smoothing.enabled,
                        egui::Slider::new(&mut self.stroke_smoothing.strength, 0.0..=1.0)
                            .text("Strength"),
                    );

                    ui.collapsing("Preset behavior", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.text_edit_singleline(&mut self.preset.name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Default size");
                            ui.add(egui::DragValue::new(&mut self.preset.size.default).speed(1.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Size range");
                            ui.add(egui::DragValue::new(&mut self.preset.size.min).speed(1.0));
                            ui.add(egui::DragValue::new(&mut self.preset.size.max).speed(1.0));
                        });
                        ui.add(
                            egui::Slider::new(&mut self.preset.pressure.min_size, 0.0..=1.0)
                                .text("Min pressure size"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.preset.pressure.min_opacity, 0.0..=1.0)
                                .text("Min pressure opacity"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.preset.pressure.opacity_gamma)
                                .range(0.01..=10.0)
                                .speed(0.05)
                                .prefix("Opacity gamma "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.preset.spacing.ratio)
                                .range(0.0..=10.0)
                                .speed(0.01)
                                .prefix("Spacing ratio "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.preset.spacing.minimum)
                                .range(0.01..=100.0)
                                .speed(0.1)
                                .prefix("Minimum spacing "),
                        );

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    self.active_brush != "charcoal",
                                    egui::Button::new("Save preset"),
                                )
                                .clicked()
                            {
                                self.preset.smoothing.enabled = self.stroke_smoothing.enabled;
                                self.preset.smoothing.strength = self.stroke_smoothing.strength;
                                self.action = Some(GuiAction::SavePreset(self.preset.clone()));
                            }
                            if ui.button("New / Save As").clicked() {
                                self.show_save_as = !self.show_save_as;
                                self.confirm_delete = false;
                            }
                            if ui
                                .add_enabled(
                                    self.active_brush != "charcoal",
                                    egui::Button::new("Delete"),
                                )
                                .clicked()
                            {
                                self.confirm_delete = true;
                                self.show_save_as = false;
                            }
                        });

                        if self.show_save_as {
                            ui.horizontal(|ui| {
                                ui.label("New ID");
                                ui.text_edit_singleline(&mut self.save_as_id);
                                if ui.button("Create").clicked() {
                                    self.preset.smoothing.enabled = self.stroke_smoothing.enabled;
                                    self.preset.smoothing.strength = self.stroke_smoothing.strength;
                                    self.action = Some(GuiAction::SavePresetAs {
                                        id: self.save_as_id.trim().to_owned(),
                                        preset: self.preset.clone(),
                                    });
                                }
                            });
                        }
                        if self.confirm_delete {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::LIGHT_RED, "Delete this preset?");
                                if ui.button("Confirm").clicked() {
                                    self.action = Some(GuiAction::DeletePreset);
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm_delete = false;
                                }
                            });
                        }

                        if self.preset != self.saved_preset {
                            ui.label(egui::RichText::new("Unsaved preset changes").italics());
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save settings").clicked() {
                            self.save_requested = true;
                        }
                        if ui.button("Reload").clicked() {
                            self.action = Some(GuiAction::ReloadFromDisk);
                        }
                        if ui.button("Reset").clicked() {
                            self.brush.size = self.default_size;
                            self.brush.color = brush_color(&CurrentBrushConfig::default());
                            self.stroke_smoothing = self.default_smoothing;
                            self.settings_message = None;
                        }
                    });
                    if ui.button("Open config folder").clicked() {
                        self.action = Some(GuiAction::OpenConfigDirectory);
                    }

                    if self.current_brush_config() != self.saved_brush
                        || self.active_brush != self.saved_active_brush
                    {
                        ui.label(egui::RichText::new("Unsaved settings changes").italics());
                    }
                    if let Some(message) = &self.settings_message {
                        let color = if message.is_error {
                            egui::Color32::LIGHT_RED
                        } else {
                            egui::Color32::LIGHT_GREEN
                        };
                        ui.colored_label(color, &message.text);
                    }
                });

            color_picker::show(ui.ctx(), &mut self.brush.color);
        });

        self.sync_runtime_behavior();
        output
    }

    pub fn take_save_requested(&mut self) -> bool {
        std::mem::take(&mut self.save_requested)
    }

    pub(crate) fn take_action(&mut self) -> Option<GuiAction> {
        self.action.take()
    }

    pub(crate) fn active_brush(&self) -> &str {
        &self.active_brush
    }

    pub fn current_brush_config(&self) -> CurrentBrushConfig {
        CurrentBrushConfig {
            size: self.brush.size,
            color: self.brush.color.to_array(),
        }
    }

    pub fn settings_saved(&mut self, path: &std::path::Path) {
        self.saved_brush = self.current_brush_config();
        self.saved_active_brush.clone_from(&self.active_brush);
        self.show_message(format!("Saved to {}", path.display()), false);
    }

    pub fn settings_save_failed(&mut self, error: impl Into<String>) {
        self.show_message(error, true);
    }

    pub(crate) fn preset_saved(&mut self, path: &std::path::Path) {
        self.saved_preset = self.preset.clone();
        self.show_message(format!("Saved preset to {}", path.display()), false);
    }

    pub(crate) fn apply_brush_preset(&mut self, loaded: &LoadedBrushPreset, catalog: BrushCatalog) {
        self.active_brush.clone_from(&loaded.id);
        self.brushes = catalog.brushes;
        self.preset = loaded.preset.clone();
        self.saved_preset = loaded.preset.clone();
        self.size_range = loaded.preset.size.min..=loaded.preset.size.max;
        self.default_size = loaded.preset.size.default;
        self.brush.size = self.default_size;
        self.default_smoothing = StrokeSmoothingOptions {
            enabled: loaded.preset.smoothing.enabled,
            strength: loaded.preset.smoothing.strength,
        };
        self.stroke_smoothing = self.default_smoothing;
        self.show_save_as = false;
        self.save_as_id.clear();
        self.confirm_delete = false;
        self.sync_runtime_behavior();
        self.show_message(format!("Selected {}", loaded.preset.name), false);
    }

    pub(crate) fn settings_reloaded(&mut self, config: &AppConfig) {
        self.brush.color = brush_color(&config.brush);
        self.brush.size = config
            .brush
            .size
            .clamp(*self.size_range.start(), *self.size_range.end());
        self.saved_brush = config.brush.clone();
        self.saved_active_brush.clone_from(&config.active_brush);
        self.show_message("Reloaded settings and brushes from disk", false);
    }

    pub(crate) fn show_error(&mut self, error: impl Into<String>) {
        self.show_message(error, true);
    }

    pub(crate) fn show_success(&mut self, message: impl Into<String>) {
        self.show_message(message, false);
    }

    fn sync_runtime_behavior(&mut self) {
        self.preset.smoothing.enabled = self.stroke_smoothing.enabled;
        self.preset.smoothing.strength = self.stroke_smoothing.strength;
        self.brush.pressure = PressureSettings {
            min_size: self.preset.pressure.min_size,
            min_opacity: self.preset.pressure.min_opacity,
            opacity_gamma: self.preset.pressure.opacity_gamma,
        };
        self.brush.spacing = BrushSpacing {
            ratio: self.preset.spacing.ratio,
            minimum: self.preset.spacing.minimum,
        };
        if self.preset.size.min.is_finite()
            && self.preset.size.max.is_finite()
            && self.preset.size.min > 0.0
            && self.preset.size.max >= self.preset.size.min
        {
            self.size_range = self.preset.size.min..=self.preset.size.max;
            self.brush.size = self
                .brush
                .size
                .clamp(self.preset.size.min, self.preset.size.max);
        }
        self.default_size = self.preset.size.default;
    }

    fn show_message(&mut self, text: impl Into<String>, is_error: bool) {
        self.settings_message = Some(SettingsMessage {
            text: text.into(),
            is_error,
        });
        self.context.request_repaint();
    }
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

pub fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |viewport| viewport.repaint_delay)
}
