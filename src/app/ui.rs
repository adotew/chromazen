mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    config::{AppConfig, BrushCatalog, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PaintTool, PressureSettings, StrokeSmoothingOptions},
    renderer::{LayerSelection, LayerSnapshot, PaintRenderer},
};

use super::command::AppCommand;

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
    active_brush: String,
    brushes: Vec<crate::config::BrushSummary>,
    size_range: std::ops::RangeInclusive<f32>,
    default_size: f32,
    commands: Vec<AppCommand>,
    settings_message: Option<SettingsMessage>,
    background_edit_start: Option<[u8; 3]>,
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
            active_brush: brush_preset.id.clone(),
            brushes: catalog.brushes,
            size_range: preset.size.min..=preset.size.max,
            default_size: preset.size.default,
            commands: Vec::new(),
            settings_message: load_error.map(|text| SettingsMessage {
                text,
                is_error: true,
            }),
            background_edit_start: None,
        }
    }

    pub fn run(
        &mut self,
        window: &Window,
        layers: &LayerSnapshot,
        tool: PaintTool,
    ) -> egui::FullOutput {
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        context.run_ui(raw_input, |ui| {
            let background = background_color(layers.background_color);

            egui::Panel::right("tools")
                .default_size(300.0)
                .resizable(false)
                .show_inside(ui, |ui| {
                    match layers.selection {
                        LayerSelection::Background => {
                            let mut color = background;
                            if color_picker::show(ui, &mut color) {
                                self.background_edit_start.get_or_insert(rgb(background));
                                self.commands
                                    .push(AppCommand::SetBackgroundColor(rgb(color)));
                            }
                            if !ui.ctx().input(|input| input.pointer.primary_down())
                                && let Some(before) = self.background_edit_start.take()
                            {
                                self.commands.push(AppCommand::CommitBackgroundColor {
                                    before,
                                    after: rgb(color),
                                });
                            }
                        }
                        LayerSelection::Paint(_) => {
                            if let Some(before) = self.background_edit_start.take() {
                                self.commands.push(AppCommand::CommitBackgroundColor {
                                    before,
                                    after: rgb(background),
                                });
                            }
                            color_picker::show(ui, &mut self.brush.color);
                        }
                    }

                    ui.separator();
                    let selected_name = self
                        .brushes
                        .iter()
                        .find(|brush| brush.id == self.active_brush)
                        .map_or(self.active_brush.as_str(), |brush| brush.name.as_str());
                    egui::ComboBox::from_label("")
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for brush in &self.brushes {
                                if ui
                                    .selectable_label(brush.id == self.active_brush, &brush.name)
                                    .clicked()
                                    && brush.id != self.active_brush
                                {
                                    self.commands
                                        .push(AppCommand::SwitchBrush(brush.id.clone()));
                                }
                            }
                        });
                    ui.add(
                        egui::Slider::new(&mut self.brush.size, self.size_range.clone())
                            .suffix(" px"),
                    );
                    if let Some(message) = &self.settings_message {
                        let color = if message.is_error {
                            egui::Color32::LIGHT_RED
                        } else {
                            egui::Color32::LIGHT_GREEN
                        };
                        ui.colored_label(color, &message.text);
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            self.commands.push(AppCommand::AddLayer);
                        }
                        let can_delete = layers.layers.len() > 1
                            && matches!(layers.selection, LayerSelection::Paint(_));
                        if ui
                            .add_enabled(can_delete, egui::Button::new("Delete"))
                            .clicked()
                        {
                            self.commands.push(AppCommand::DeleteSelectedLayer);
                        }
                    });
                    for layer in layers.layers.iter().rev() {
                        let selected = layers.selection == LayerSelection::Paint(layer.id);
                        if ui.selectable_label(selected, &layer.name).clicked() && !selected {
                            self.commands.push(AppCommand::SelectLayer(layer.id));
                        }
                    }
                    ui.horizontal(|ui| {
                        let selected = layers.selection == LayerSelection::Background;
                        if ui.selectable_label(selected, "Background").clicked() && !selected {
                            self.commands.push(AppCommand::SelectBackground);
                        }
                    });
                });

            egui::Area::new(egui::Id::new("tool mode"))
                .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
                .interactable(false)
                .show(ui.ctx(), |ui| show_tool_badge(ui, tool));
        })
    }

    pub(crate) fn take_commands(&mut self) -> Vec<AppCommand> {
        std::mem::take(&mut self.commands)
    }

    pub(crate) fn settings_snapshot(&self) -> (CurrentBrushConfig, String) {
        (self.current_brush_config(), self.active_brush.clone())
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
    }

    pub(crate) fn settings_reloaded(&mut self, config: &AppConfig) {
        self.brush.color = brush_color(&config.brush);
        self.brush.size = config
            .brush
            .size
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

fn show_tool_badge(ui: &mut egui::Ui, tool: PaintTool) {
    let (label, fill) = match tool {
        PaintTool::Brush => ("BRUSH", egui::Color32::from_rgb(169, 186, 200)),
        PaintTool::Eraser => ("ERASER", egui::Color32::from_rgb(213, 170, 109)),
        PaintTool::Smudge => ("SMUDGE", egui::Color32::from_rgb(177, 159, 204)),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .color(egui::Color32::from_rgb(35, 35, 40))
                    .strong(),
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
}
