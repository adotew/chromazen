use super::*;

impl GuiLayer {
    pub(super) fn show_layer_transform_overlay(
        &mut self,
        ui: &mut egui::Ui,
        view: PaintViewSnapshot,
        bounds: Option<LayerContentBounds>,
        active: Option<LayerTransform>,
        workspace_rect: egui::Rect,
    ) {
        let Some(bounds) = bounds else {
            self.layer_transform_drag = None;
            return;
        };
        let context = ui.ctx().clone();
        let transform = active.unwrap_or_default();
        let pixels_per_point = context.pixels_per_point();
        let corners = layer_transform_screen_corners(bounds, transform, view, pixels_per_point);
        let handles = canvas_crop_handle_positions(corners);
        let rotation_handle = layer_rotation_handle(corners);
        let pointer = context.pointer_latest_pos();
        let hovered_handle = pointer.and_then(|pointer| {
            layer_transform_handle_at(pointer, corners, &handles, rotation_handle)
        });
        let cursor_handle = self
            .layer_transform_drag
            .map(|drag| drag.handle)
            .or(hovered_handle);
        let (panning, preserve_aspect) =
            context.input(|input| (input.key_down(egui::Key::Space), input.modifiers.shift));
        let interaction_rect = if panning {
            egui::Rect::NOTHING
        } else if self.layer_transform_drag.is_some() {
            workspace_rect
        } else if let (Some(_), Some(pointer)) = (hovered_handle, pointer) {
            egui::Rect::from_center_size(pointer, egui::Vec2::splat(2.0))
        } else {
            egui::Rect::NOTHING
        };
        let response = ui.interact(
            interaction_rect,
            ui.id().with("layer transform interaction"),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter().with_clip_rect(workspace_rect);
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            egui::Stroke::new(2.0, egui::Color32::from_gray(96)),
        ));
        let top_center = corners[0] + (corners[1] - corners[0]) * 0.5;
        painter.line_segment(
            [top_center, rotation_handle],
            egui::Stroke::new(1.5, egui::Color32::from_gray(160)),
        );
        paint_resize_handle_markers(&painter, &handles);
        painter.circle_filled(rotation_handle, 7.0, egui::Color32::from_gray(232));
        painter.circle_stroke(
            rotation_handle,
            7.0,
            egui::Stroke::new(1.5, egui::Color32::from_gray(72)),
        );
        painter.text(
            egui::pos2(workspace_rect.center().x, workspace_rect.top() + 16.0),
            egui::Align2::CENTER_TOP,
            "Enter to apply  •  Esc to cancel",
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
        let response = if let Some(handle) = cursor_handle {
            response.on_hover_cursor(layer_transform_cursor(handle))
        } else {
            response
        };

        if response.drag_started()
            && let (Some(pointer), Some(handle)) = (pointer, hovered_handle)
        {
            self.layer_transform_drag = Some(LayerTransformDrag {
                handle,
                start_transform: transform,
                start_pointer: pointer_document_position(pointer, view, pixels_per_point),
            });
        }
        if response.dragged()
            && let (Some(pointer), Some(drag)) = (pointer, self.layer_transform_drag)
        {
            let pointer = pointer_document_position(pointer, view, pixels_per_point);
            self.commands
                .push(AppCommand::Editor(EditorCommand::SetLayerTransform(
                    layer_transform_from_drag(drag, pointer, bounds, preserve_aspect),
                )));
        }
        if response.drag_stopped() {
            self.layer_transform_drag = None;
        }
    }
}
