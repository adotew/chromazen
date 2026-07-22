use egui::{Color32, Ui};

pub(super) fn show(ui: &mut Ui, color: &mut Color32) -> bool {
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width;
        egui::color_picker::color_picker_color32(ui, color, egui::color_picker::Alpha::Opaque)
    })
    .inner
}
