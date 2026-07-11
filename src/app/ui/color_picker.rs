use egui::{Color32, Context};

pub(super) fn show(context: &Context, color: &mut Color32) {
    egui::Window::new("Color picker")
        .title_bar(false)
        .anchor(egui::Align2::RIGHT_TOP, [0.0, 0.0])
        .resizable(false)
        .show(context, |ui| {
            ui.spacing_mut().slider_width = 275.0;
            egui::color_picker::color_picker_color32(ui, color, egui::color_picker::Alpha::Opaque);
        });
}
