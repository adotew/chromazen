use std::time::Duration;

use egui::ViewportId;
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions};
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::{
    brush::BrushSettings,
    constants::{MAX_BRUSH_SIZE, MIN_BRUSH_SIZE},
    renderer::{PaintRenderer, PaintStats},
    stroke_smoothing::StrokeSmoothingOptions,
};

pub struct GuiLayer {
    pub context: egui::Context,
    pub state: EguiWinitState,
    pub renderer: EguiRenderer,
    pub brush: BrushSettings,
    pub stroke_smoothing: StrokeSmoothingOptions,
}

#[derive(Clone, Copy)]
pub struct PanelSnapshot {
    pub document_size: [u32; 2],
    pub zoom: f32,
    pub offset: [f32; 2],
    pub pressure: f32,
    pub pen_active: bool,
    pub frame_ms: f32,
    pub fps: f32,
    pub stats: PaintStats,
}

#[derive(Default)]
pub struct PanelActions {
    pub clear: bool,
    pub fit: bool,
    pub zoom_100: bool,
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

    pub fn run_panel(
        &mut self,
        window: &Window,
        snapshot: PanelSnapshot,
    ) -> (egui::FullOutput, PanelActions) {
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();
        let mut actions = PanelActions::default();

        let full_output = context.run_ui(raw_input, |ui| {
            egui::Window::new("minipaint-rs")
                .default_pos([12.0, 12.0])
                .default_width(260.0)
                .show(ui.ctx(), |ui| {
                    ui.label("Minimal wgpu brush performance port");
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut self.brush.size, MIN_BRUSH_SIZE..=MAX_BRUSH_SIZE)
                            .text("Brush size")
                            .suffix(" px"),
                    );
                    egui::color_picker::color_picker_color32(
                        ui,
                        &mut self.brush.color,
                        egui::color_picker::Alpha::Opaque,
                    );
                    ui.separator();
                    ui.checkbox(&mut self.stroke_smoothing.enabled, "Stroke smoothing");
                    ui.add_enabled(
                        self.stroke_smoothing.enabled,
                        egui::Slider::new(&mut self.stroke_smoothing.strength, 0.0..=1.0)
                            .text("Smoothing strength")
                            .fixed_decimals(2),
                    )
                    .on_hover_text("0 is linear; 1 is full Catmull–Rom smoothing");
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            actions.clear = true;
                        }
                        if ui.button("Fit").clicked() {
                            actions.fit = true;
                        }
                        if ui.button("100%").clicked() {
                            actions.zoom_100 = true;
                        }
                    });
                    ui.separator();
                    ui.label(format!(
                        "Canvas: {} × {}",
                        snapshot.document_size[0], snapshot.document_size[1]
                    ));
                    ui.label(format!("Zoom: {:.1}%", snapshot.zoom * 100.0));
                    ui.label(format!(
                        "Offset: {:.0}, {:.0}",
                        snapshot.offset[0], snapshot.offset[1]
                    ));
                    ui.label(format!(
                        "Pressure: {} {:.0}%",
                        if snapshot.pen_active {
                            "pen"
                        } else {
                            "mouse/fallback"
                        },
                        snapshot.pressure * 100.0,
                    ));
                    ui.label(format!(
                        "Frame: {:.2} ms ({:.0} FPS)",
                        snapshot.frame_ms, snapshot.fps
                    ));
                    ui.label(format!(
                        "Stamps/frame: {}",
                        snapshot.stats.stamps_last_frame
                    ));
                    ui.label(format!("Pending stamps: {}", snapshot.stats.pending_stamps));
                    ui.label(format!("Total stamps: {}", snapshot.stats.total_stamps));
                    ui.separator();
                    ui.small(
                        "Paint: left drag · Pan: middle/right drag or Space+left · Zoom: wheel",
                    );
                });
        });

        (full_output, actions)
    }
}

pub fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |viewport| viewport.repaint_delay)
}
