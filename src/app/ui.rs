mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    config::{AppConfig, BrushCatalog, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PressureSettings, StrokeSmoothingOptions},
    renderer::PaintRenderer,
};

pub(crate) enum GuiAction {
    SwitchBrush(String),
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
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    save_requested: bool,
    action: Option<GuiAction>,
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
        let preset = &brush_preset.preset;

        Self {
            context,
            state,
            renderer,
            brush: brush_settings_from_config(&config.brush, brush_preset),
            stroke_smoothing: StrokeSmoothingOptions {
                strength: config.smoothing.strength,
            },
            saved_brush: config.brush.clone(),
            saved_active_brush: config.active_brush.clone(),
            active_brush: brush_preset.id.clone(),
            brushes: catalog.brushes,
            size_range: preset.size.min..=preset.size.max,
            default_size: preset.size.default,
            save_requested: false,
            action: None,
            settings_message: load_error.map(|text| SettingsMessage {
                text,
                is_error: true,
            }),
        }
    }

    pub fn run(&mut self, window: &Window) -> egui::FullOutput {
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        context.run_ui(raw_input, |ui| {
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
        })
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

    pub(crate) fn apply_brush_preset(&mut self, loaded: &LoadedBrushPreset, catalog: BrushCatalog) {
        let preset = &loaded.preset;
        self.active_brush.clone_from(&loaded.id);
        self.brushes = catalog.brushes;
        self.size_range = preset.size.min..=preset.size.max;
        self.default_size = preset.size.default;
        self.brush.size = self.default_size;
        self.brush.pressure = PressureSettings {
            min_size: preset.pressure.min_size,
            min_opacity: preset.pressure.min_opacity,
            opacity_gamma: preset.pressure.opacity_gamma,
        };
        self.brush.spacing = BrushSpacing {
            ratio: preset.spacing.ratio,
            minimum: preset.spacing.minimum,
        };
        self.show_message(format!("Selected {}", preset.name), false);
    }

    pub(crate) fn settings_reloaded(&mut self, config: &AppConfig) {
        self.brush.color = brush_color(&config.brush);
        self.brush.size = config
            .brush
            .size
            .clamp(*self.size_range.start(), *self.size_range.end());
        self.stroke_smoothing.strength = config.smoothing.strength;
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
