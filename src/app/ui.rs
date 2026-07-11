mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    config::{AppConfig, CurrentBrushConfig},
    paint::{BrushSettings, MAX_BRUSH_SIZE, MIN_BRUSH_SIZE, StrokeSmoothingOptions},
    renderer::PaintRenderer,
};

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
    saved_brush: CurrentBrushConfig,
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

        let brush = brush_settings_from_config(&config.brush);
        Self {
            context,
            state,
            renderer,
            brush,
            stroke_smoothing: StrokeSmoothingOptions::default(),
            saved_brush: config.brush.clone(),
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
                        egui::Slider::new(&mut self.brush.size, MIN_BRUSH_SIZE..=MAX_BRUSH_SIZE)
                            .suffix(" px"),
                    );

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save settings").clicked() {
                            self.save_requested = true;
                        }
                        if ui.button("Reset").clicked() {
                            self.brush = brush_settings_from_config(&CurrentBrushConfig::default());
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

fn brush_settings_from_config(config: &CurrentBrushConfig) -> BrushSettings {
    BrushSettings {
        color: egui::Color32::from_rgba_unmultiplied(
            config.color[0],
            config.color[1],
            config.color[2],
            config.color[3],
        ),
        size: config.size,
    }
}

pub fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |viewport| viewport.repaint_delay)
}
