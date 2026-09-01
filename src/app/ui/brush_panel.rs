use super::*;

const BRUSH_ROW_HEIGHT: f32 = 48.0;
const STAMP_SIZE: f32 = 40.0;

impl GuiLayer {
    pub(super) fn show_brush_panel(&mut self, ui: &mut egui::Ui, tool: PaintTool) {
        let selected_id = self.tool_brushes[tool_index(tool)].clone();
        let brushes = self.brushes.clone();
        let mut switch_to = None;

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("brush preset list")
            .max_height(BRUSH_PRESET_LIST_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for brush in &brushes {
                    let selected = brush.id == selected_id;
                    let response = show_brush_row(
                        ui,
                        &brush.name,
                        self.brush_preview_texture(&brush.id),
                        selected,
                    );
                    if response.clicked() && !selected {
                        switch_to = Some(brush.id.clone());
                    }
                    ui.add_space(2.0);
                }
            });

        if let Some(id) = switch_to {
            self.commands
                .push(AppCommand::Settings(SettingsCommand::SwitchBrush {
                    tool,
                    id,
                }));
        }
        self.cache_next_missing_brush_preview();
    }
}

fn show_brush_row(
    ui: &mut egui::Ui,
    name: &str,
    texture_id: Option<egui::TextureId>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = selectable_row(ui, BRUSH_ROW_HEIGHT, egui::Sense::click(), selected);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
    let visuals = ui.style().interact(&response);
    let stamp_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - STAMP_SIZE * 0.5 - 6.0, rect.center().y),
        egui::Vec2::splat(STAMP_SIZE),
    );
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, rect.top()),
        egui::pos2(stamp_rect.left() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(name_rect).text(
        name_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        name,
        egui::TextStyle::Button.resolve(ui.style()),
        visuals.text_color(),
    );
    paint_stamp(ui, stamp_rect, texture_id, visuals.text_color());

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(name)
}

fn paint_stamp(
    ui: &egui::Ui,
    rect: egui::Rect,
    texture_id: Option<egui::TextureId>,
    tint: egui::Color32,
) {
    if let Some(texture_id) = texture_id {
        egui::Image::new((texture_id, rect.size()))
            .tint(tint)
            .alt_text("Brush stamp")
            .paint_at(ui, rect);
    } else {
        ui.painter().circle_filled(
            rect.center(),
            rect.width() * 0.18,
            ui.visuals().weak_text_color(),
        );
    }
}
