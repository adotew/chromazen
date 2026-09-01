use super::*;

impl GuiLayer {
    pub(super) fn brush_preview_texture(&self, brush_id: &str) -> Option<egui::TextureId> {
        self.brush_previews
            .iter()
            .find(|(id, _)| id == brush_id)
            .map(|(_, texture)| texture.id())
    }

    pub(super) fn ensure_brush_preview_cached(&mut self, brush_id: &str) {
        if self.brush_preview_texture(brush_id).is_some()
            || self.failed_brush_previews.iter().any(|id| id == brush_id)
        {
            return;
        }
        let Some(brush) = self
            .brushes
            .iter()
            .find(|brush| brush.id == brush_id)
            .cloned()
        else {
            return;
        };

        match brush_preview::generate(&brush) {
            Ok(image) => {
                let texture = self.context.load_texture(
                    format!("brush preview {}", brush.id),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                self.brush_previews.push((brush.id, texture));
            }
            Err(error) => {
                log::warn!(
                    "failed to generate preview for brush '{}': {error}",
                    brush.id
                );
                self.failed_brush_previews.push(brush.id);
            }
        }
    }

    pub(super) fn cache_next_missing_brush_preview(&mut self) {
        let next_id = self
            .brushes
            .iter()
            .find(|brush| {
                self.brush_preview_texture(&brush.id).is_none()
                    && !self.failed_brush_previews.iter().any(|id| id == &brush.id)
            })
            .map(|brush| brush.id.clone());
        if let Some(id) = next_id {
            self.ensure_brush_preview_cached(&id);
            self.context.request_repaint();
        }
    }

    pub(super) fn show_brush_color_picker(&mut self, ui: &mut egui::Ui) {
        if color_picker::show(ui, &mut self.brush.color) {
            self.commands
                .push(AppCommand::Editor(EditorCommand::SetBrushColor(
                    self.brush.color.to_array(),
                )));
        }
    }

    pub(super) fn show_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        active_tool: EditorTool,
    ) -> Option<EditorTool> {
        const TOOL_WIDTH: f32 = 40.0;
        const TOOL_COUNT: usize = 3;
        const HORIZONTAL_PADDING: f32 = 6.0;

        let tools = [PaintTool::Brush, PaintTool::Eraser, PaintTool::Smudge];
        let button_count = TOOL_COUNT + 2;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                TOOL_WIDTH * button_count as f32 + 2.0 * HORIZONTAL_PADDING,
                TOOL_RAIL_THICKNESS,
            ),
            egui::Sense::hover(),
        );
        paint_rounded_panel(ui, rect, egui::CornerRadius::same(16));
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.left() + HORIZONTAL_PADDING, rect.top()),
            egui::pos2(rect.right() - HORIZONTAL_PADDING, rect.bottom()),
        );

        let mut selected_tool = None;
        for (index, paint_tool) in tools.into_iter().enumerate() {
            let tool = EditorTool::from(paint_tool);
            let tool_rect = egui::Rect::from_min_size(
                egui::pos2(body.left() + index as f32 * TOOL_WIDTH, body.top()),
                egui::vec2(TOOL_WIDTH, TOOL_RAIL_THICKNESS),
            );
            let response = show_tool_button(ui, tool_rect, paint_tool, tool == active_tool);
            if response.clicked() {
                egui::Popup::close_all(ui.ctx());
                if tool == active_tool {
                    self.brush_window_open = !self.brush_window_open;
                } else {
                    selected_tool = Some(tool);
                }
            }
        }

        {
            let separator_x = body.left() + TOOL_COUNT as f32 * TOOL_WIDTH;
            let layers_rect = egui::Rect::from_min_size(
                egui::pos2(separator_x, body.top()),
                egui::vec2(TOOL_WIDTH, TOOL_RAIL_THICKNESS),
            );
            let layers_response = ui
                .interact(layers_rect, ui.id().with("Layers"), egui::Sense::click())
                .on_hover_text("Layers");
            let layers_color = if self.layers_window_open || layers_response.hovered() {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            egui::Image::new(egui::include_image!("../../../assets/icons/layers.svg"))
                .fit_to_exact_size(egui::Vec2::splat(20.0))
                .tint(layers_color)
                .alt_text("Layers")
                .paint_at(
                    ui,
                    egui::Rect::from_center_size(layers_rect.center(), egui::Vec2::splat(20.0)),
                );
            if layers_response.clicked() {
                self.layers_window_open = !self.layers_window_open;
            }

            let color_rect = layers_rect.translate(egui::vec2(TOOL_WIDTH, 0.0));
            let color_response = ui
                .interact(color_rect, ui.id().with("Color"), egui::Sense::click())
                .on_hover_text("Color");
            ui.painter()
                .circle_filled(color_rect.center(), 9.0, self.brush.color);
            if color_response.clicked() {
                self.color_window_open = !self.color_window_open;
            }
        }
        selected_tool
    }
}
