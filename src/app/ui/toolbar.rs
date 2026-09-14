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
        const TOOL_SIZE: f32 = 40.0;
        const PAINT_TOOL_COUNT: usize = 3;
        const VERTICAL_PADDING: f32 = 6.0;
        const EDGE_MARGIN: f32 = 12.0;

        let tools = [PaintTool::Brush, PaintTool::Eraser, PaintTool::Smudge];
        let button_count = PAINT_TOOL_COUNT + 3;
        let buttons_height = TOOL_SIZE * button_count as f32;
        let controls_height = (ui.ctx().content_rect().height()
            - buttons_height
            - 2.0 * VERTICAL_PADDING
            - 2.0 * EDGE_MARGIN)
            .clamp(0.0, brush_controls::CONTROLS_HEIGHT);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                TOOL_RAIL_THICKNESS,
                buttons_height + controls_height + 2.0 * VERTICAL_PADDING,
            ),
            egui::Sense::hover(),
        );
        paint_rounded_panel(
            ui,
            rect,
            egui::CornerRadius {
                nw: 16,
                ne: 0,
                sw: 16,
                se: 0,
            },
        );
        let body = rect.shrink2(egui::vec2(0.0, VERTICAL_PADDING));

        let mut selected_tool = None;
        for (index, paint_tool) in tools.into_iter().enumerate() {
            let tool = EditorTool::from(paint_tool);
            let tool_rect = egui::Rect::from_min_size(
                egui::pos2(body.left(), body.top() + index as f32 * TOOL_SIZE),
                egui::vec2(TOOL_RAIL_THICKNESS, TOOL_SIZE),
            );
            let response = show_tool_button(ui, tool_rect, tool, tool == active_tool);
            if response.clicked() {
                egui::Popup::close_all(ui.ctx());
                if tool == active_tool {
                    self.brush_window_open = !self.brush_window_open;
                } else {
                    selected_tool = Some(tool);
                }
            }
        }

        let controls_top = body.top() + (PAINT_TOOL_COUNT + 2) as f32 * TOOL_SIZE;
        let layers_rect = egui::Rect::from_min_size(
            egui::pos2(
                body.left(),
                body.top() + PAINT_TOOL_COUNT as f32 * TOOL_SIZE,
            ),
            egui::vec2(TOOL_RAIL_THICKNESS, TOOL_SIZE),
        );
        let layers_response = ui
            .interact(layers_rect, ui.id().with("Layers"), egui::Sense::click())
            .on_hover_text("Layers");
        let layers_color =
            if self.sidebar_visible || self.layers_window_open || layers_response.hovered() {
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
            if self.sidebar_visible {
                self.toggle_sidebar();
            } else {
                self.layers_window_open = !self.layers_window_open;
            }
        }

        let color_rect = layers_rect.translate(egui::vec2(0.0, TOOL_SIZE));
        let color_response = ui
            .interact(color_rect, ui.id().with("Color"), egui::Sense::click())
            .on_hover_text("Color");
        ui.painter()
            .circle_filled(color_rect.center(), 9.0, self.brush.color);
        if self.sidebar_visible || self.color_window_open {
            ui.painter().circle_stroke(
                color_rect.center(),
                11.0,
                egui::Stroke::new(2.0, ui.visuals().text_color()),
            );
        }
        if color_response.clicked() {
            if self.sidebar_visible {
                self.toggle_sidebar();
            } else {
                self.color_window_open = !self.color_window_open;
            }
        }

        self.brush_slider_active = false;
        self.brush_slider_focus = None;
        let controls_rect = egui::Rect::from_min_size(
            egui::pos2(body.left(), controls_top),
            egui::vec2(TOOL_RAIL_THICKNESS, controls_height),
        );
        if controls_height > 0.0 {
            let mut controls_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("brush controls")
                    .max_rect(controls_rect),
            );
            controls_ui.set_clip_rect(controls_rect.intersect(ui.clip_rect()));
            self.show_brush_controls(&mut controls_ui, active_tool, controls_height);
        }

        let transform_rect = egui::Rect::from_min_size(
            controls_rect.left_bottom(),
            egui::vec2(TOOL_RAIL_THICKNESS, TOOL_SIZE),
        );
        let transform_response = show_tool_button(
            ui,
            transform_rect,
            EditorTool::Transform,
            active_tool == EditorTool::Transform,
        );
        if transform_response.clicked() {
            egui::Popup::close_all(ui.ctx());
            if active_tool == EditorTool::Transform {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::ApplyLayerTransform));
            } else {
                selected_tool = Some(EditorTool::Transform);
            }
        }
        selected_tool
    }
}
