use super::*;

const DASH_LENGTH: f32 = 5.0;

impl GuiLayer {
    pub(super) fn show_selection_outline(
        &self,
        ui: &egui::Ui,
        view: PaintViewSnapshot,
        path: &[[f32; 2]],
        closed: bool,
        workspace_rect: egui::Rect,
    ) {
        if path.len() < 2 {
            return;
        }
        let pixels_per_point = ui.ctx().pixels_per_point();
        let mut points: Vec<_> = path
            .iter()
            .map(|&point| {
                let point = view.document_to_window(point);
                egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point)
            })
            .collect();
        if closed {
            points.push(points[0]);
        }
        let painter = ui.painter().with_clip_rect(workspace_rect);
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
        ));
        painter.extend(egui::Shape::dashed_line(
            &points,
            egui::Stroke::new(1.5_f32, egui::Color32::BLACK),
            DASH_LENGTH,
            DASH_LENGTH,
        ));
    }
}
