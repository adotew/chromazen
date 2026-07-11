mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    config::{AppConfig, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PressureSettings, StrokeSmoothingOptions},
    renderer::PaintRenderer,
};

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
    saved_brush: CurrentBrushConfig,
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    default_smoothing: StrokeSmoothingOptions,
    save_requested: bool,
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
            size_range: preset.size.min..=preset.size.max,
            default_size: preset.size.default,
            default_smoothing,
            save_requested: false,
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
            egui::Window::new("Brush size")
                .default_pos([12.0, 12.0])
                .default_width(260.0)
                .resizable(false)
                .show(ui.ctx(), |ui| {
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

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save settings").clicked() {
                            self.save_requested = true;
                        }
                        if ui.button("Reset").clicked() {
                            self.brush.size = self.default_size;
                            self.brush.color = brush_color(&CurrentBrushConfig::default());
                            self.stroke_smoothing = self.default_smoothing;
                            self.settings_message = None;
                        }
                    });

                    if self.current_brush_config() != self.saved_brush {
                        ui.label(egui::RichText::new("Unsaved changes").italics());
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

    pub fn current_brush_config(&self) -> CurrentBrushConfig {
        CurrentBrushConfig {
            size: self.brush.size,
            color: self.brush.color.to_array(),
        }
    }

    pub fn settings_saved(&mut self, path: &std::path::Path) {
        self.saved_brush = self.current_brush_config();
        self.settings_message = Some(SettingsMessage {
            text: format!("Saved to {}", path.display()),
            is_error: false,
        });
        self.context.request_repaint();
    }

    pub fn settings_save_failed(&mut self, error: impl Into<String>) {
        self.settings_message = Some(SettingsMessage {
            text: error.into(),
            is_error: true,
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
