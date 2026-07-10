mod color_picker;

use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    paint::{BrushSettings, MAX_BRUSH_SIZE, MIN_BRUSH_SIZE, StrokeSmoothingOptions},
    renderer::PaintRenderer,
};

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
}

impl GuiLayer {
    pub fn new(window: &Window, paint: &PaintRenderer) -> Self {
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

        Self {
            context,
            state,
            renderer,
            brush: BrushSettings::default(),
            stroke_smoothing: StrokeSmoothingOptions::default(),
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
                });

            color_picker::show(ui.ctx(), &mut self.brush.color);
        })
    }
}

pub fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |viewport| viewport.repaint_delay)
}
