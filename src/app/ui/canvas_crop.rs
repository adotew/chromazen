use super::*;

impl GuiLayer {
    pub(crate) fn open_canvas_crop(&mut self, size: [u32; 2]) {
        self.new_artwork_dialog = None;
        let (restore_sidebar, restore_color_window, restore_layers_window) =
            self.canvas_crop.as_ref().map_or(
                (
                    self.sidebar_visible,
                    self.color_window_open,
                    self.layers_window_open,
                ),
                |crop| {
                    (
                        crop.restore_sidebar,
                        crop.restore_color_window,
                        crop.restore_layers_window,
                    )
                },
            );
        self.canvas_crop = Some(CanvasCrop {
            rect: CanvasCropRect::from_size(size),
            drag: None,
            restore_sidebar,
            restore_color_window,
            restore_layers_window,
        });
        self.sidebar_visible = false;
        self.color_window_open = false;
        self.layers_window_open = false;
        self.selected_reference = None;
        self.reference_transform_edit = None;
        self.context.request_repaint();
    }

    pub(crate) fn close_canvas_crop(&mut self) {
        if let Some(crop) = self.canvas_crop.take() {
            self.sidebar_visible = crop.restore_sidebar;
            self.color_window_open = crop.restore_color_window;
            self.layers_window_open = crop.restore_layers_window;
            self.context.request_repaint();
        }
    }

    pub(crate) fn canvas_crop_active(&self) -> bool {
        self.canvas_crop.is_some()
    }

    pub(super) fn show_canvas_crop_overlay(
        &mut self,
        context: &egui::Context,
        view: PaintViewSnapshot,
        workspace_rect: egui::Rect,
    ) {
        let Some(rect) = self.canvas_crop.as_ref().map(|crop| crop.rect) else {
            return;
        };
        let pixels_per_point = context.pixels_per_point();
        let screen_corners = canvas_crop_screen_corners(rect, view, pixels_per_point);
        let handle_positions = canvas_crop_handle_positions(screen_corners);
        let pointer = context.pointer_latest_pos();
        let hovered_handle = pointer.and_then(|pointer| {
            let pointer_document = pointer_document_position(pointer, view, pixels_per_point);
            canvas_crop_handle_at(pointer, pointer_document, rect, &handle_positions)
        });
        let cursor_handle = self
            .canvas_crop
            .as_ref()
            .and_then(|crop| crop.drag.map(|drag| drag.handle))
            .or(hovered_handle);
        let validation = canvas_crop_request(rect, self.canvas_size_constraints);
        let status = validation
            .as_ref()
            .map(|request| {
                format!(
                    "{} × {} px  •  Enter to apply  •  Esc to cancel",
                    request.size[0], request.size[1]
                )
            })
            .unwrap_or_else(|error| format!("{error}  •  Esc to cancel"));
        let status_color = if validation.is_ok() {
            egui::Color32::WHITE
        } else {
            egui::Color32::LIGHT_RED
        };

        let response = egui::Area::new(egui::Id::new("canvas crop overlay"))
            .fixed_pos(workspace_rect.min)
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                let (overlay_rect, response) =
                    ui.allocate_exact_size(workspace_rect.size(), egui::Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(overlay_rect);
                painter.add(egui::Shape::convex_polygon(
                    screen_corners.to_vec(),
                    egui::Color32::from_white_alpha(8),
                    egui::Stroke::new(2.0, egui::Color32::from_gray(96)),
                ));
                paint_resize_handle_markers(&painter, &handle_positions);
                painter.text(
                    egui::pos2(overlay_rect.center().x, overlay_rect.top() + 16.0),
                    egui::Align2::CENTER_TOP,
                    status,
                    egui::FontId::proportional(14.0),
                    status_color,
                );
                response
            })
            .inner;
        let response = if let Some(handle) = cursor_handle {
            response.on_hover_cursor(canvas_crop_cursor(handle))
        } else {
            response
        };

        if response.drag_started()
            && let Some(pointer) = pointer
            && let Some(handle) = hovered_handle
            && let Some(crop) = self.canvas_crop.as_mut()
        {
            let pointer_document = pointer_document_position(pointer, view, pixels_per_point);
            crop.drag = Some(CanvasCropDrag {
                handle,
                start_rect: crop.rect,
                start_pointer: pointer_document,
            });
        }
        if response.dragged()
            && let Some(pointer) = pointer
            && let Some(crop) = self.canvas_crop.as_mut()
            && let Some(drag) = crop.drag
        {
            let pointer_document = pointer_document_position(pointer, view, pixels_per_point);
            crop.rect = canvas_crop_rect_from_drag(drag, pointer_document);
        }
        if response.drag_stopped()
            && let Some(crop) = self.canvas_crop.as_mut()
        {
            crop.drag = None;
        }

        let cancel = context.input(|input| input.key_pressed(egui::Key::Escape));
        let apply = context.input(|input| input.key_pressed(egui::Key::Enter));
        if cancel {
            self.close_canvas_crop();
        } else if apply
            && let Some(rect) = self.canvas_crop.as_ref().map(|crop| crop.rect)
            && let Ok(request) = canvas_crop_request(rect, self.canvas_size_constraints)
        {
            self.commands
                .push(AppCommand::Editor(EditorCommand::ResizeCanvas {
                    width: request.size[0],
                    height: request.size[1],
                    origin: request.origin,
                }));
            self.close_canvas_crop();
        }
    }
}
