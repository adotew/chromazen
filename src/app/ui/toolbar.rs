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

    pub(super) fn show_brush_color_swatch(&mut self, ui: &mut egui::Ui) {
        if color_picker::show_swatch(ui, self.brush.color, self.previous_brush_color) {
            self.brush.color = self.previous_brush_color;
            self.commands
                .push(AppCommand::Editor(EditorCommand::SetBrushColor(
                    self.brush.color.to_array(),
                )));
        }
    }

    pub(super) fn show_brush_color_picker(&mut self, ui: &mut egui::Ui) {
        if color_picker::show(ui, &mut self.brush.color) {
            self.commands
                .push(AppCommand::Editor(EditorCommand::SetBrushColor(
                    self.brush.color.to_array(),
                )));
        }
        // Wait until after the release frame's pick to avoid saving intermediate drag colors.
        if !ui.ctx().input(|input| input.pointer.any_down())
            && self.brush.color != self.settled_brush_color
        {
            self.previous_brush_color =
                std::mem::replace(&mut self.settled_brush_color, self.brush.color);
            // The sidebar swatch was already painted this frame.
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn show_editor_controls(
        &mut self,
        ui: &mut egui::Ui,
        menu: ApplicationMenuState,
        active_tool: EditorTool,
        has_selection: bool,
        has_titlebar: bool,
    ) -> Option<EditorTool> {
        const TOOL_SIZE: f32 = 40.0;
        const PADDING: f32 = 6.0;

        let left_width = if has_titlebar {
            80.0
        } else if cfg!(target_os = "macos") {
            0.0
        } else {
            36.0
        };
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                left_width + 3.0 * TOOL_SIZE + 2.0 * PADDING,
                TOOL_RAIL_THICKNESS,
            ),
            egui::Sense::hover(),
        );
        paint_rounded_panel(ui, rect, egui::CornerRadius::same(16));
        let body = rect.shrink2(egui::vec2(PADDING, 0.0));
        let mut menu_ui =
            ui.new_child(egui::UiBuilder::new().id_salt("application menu").max_rect(
                egui::Rect::from_min_size(body.min, egui::vec2(left_width, body.height())),
            ));
        self.show_application_menu(&mut menu_ui, menu);
        let gallery_rect = egui::Rect::from_min_size(
            body.min + egui::vec2(left_width, 0.0),
            egui::vec2(TOOL_SIZE, body.height()),
        );
        let gallery_response = ui
            .interact(gallery_rect, ui.id().with("Gallery"), egui::Sense::click())
            .on_hover_text("Gallery");
        let gallery_tint = if gallery_response.hovered() {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        egui::Image::new(egui::include_image!(
            "../../../assets/icons/layout-grid.svg"
        ))
        .tint(gallery_tint)
        .alt_text("Gallery")
        .paint_at(
            ui,
            egui::Rect::from_center_size(gallery_rect.center(), egui::Vec2::splat(20.0)),
        );
        if gallery_response.clicked() {
            egui::Popup::close_all(ui.ctx());
            self.commands
                .push(AppCommand::Navigation(NavigationCommand::ShowGallery));
        }
        let tools_rect = egui::Rect::from_min_size(
            gallery_rect.right_top(),
            egui::vec2(2.0 * TOOL_SIZE, body.height()),
        );
        show_selection_tools(
            ui,
            tools_rect,
            active_tool,
            has_selection,
            &mut self.commands,
        )
    }

    pub(super) fn show_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        active_tool: EditorTool,
    ) -> Option<EditorTool> {
        const TOOL_SIZE: f32 = 40.0;
        const PADDING: f32 = 6.0;

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(5.0 * TOOL_SIZE + 2.0 * PADDING, TOOL_RAIL_THICKNESS),
            egui::Sense::hover(),
        );
        paint_rounded_panel(ui, rect, egui::CornerRadius::same(16));
        let body = rect.shrink2(egui::vec2(PADDING, 0.0));

        let mut selected_tool = None;
        for (index, paint_tool) in [PaintTool::Brush, PaintTool::Eraser, PaintTool::Smudge]
            .into_iter()
            .enumerate()
        {
            let tool = EditorTool::from(paint_tool);
            let tool_rect = egui::Rect::from_min_size(
                egui::pos2(body.left() + index as f32 * TOOL_SIZE, body.top()),
                egui::vec2(TOOL_SIZE, TOOL_RAIL_THICKNESS),
            );
            let response = show_tool_button(ui, tool_rect, tool, tool == active_tool);
            if response.clicked() {
                egui::Popup::close_all(ui.ctx());
                if tool == active_tool {
                    if self.sidebar_visible {
                        self.toggle_sidebar();
                    } else {
                        self.brush_window_open = !self.brush_window_open;
                    }
                } else {
                    selected_tool = Some(tool);
                }
            }
        }

        let layers_rect = egui::Rect::from_min_size(
            egui::pos2(body.left() + 3.0 * TOOL_SIZE, body.top()),
            egui::vec2(TOOL_SIZE, TOOL_RAIL_THICKNESS),
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

        let color_rect = layers_rect.translate(egui::vec2(TOOL_SIZE, 0.0));
        let color_response = ui
            .interact(color_rect, ui.id().with("Color"), egui::Sense::click())
            .on_hover_text("Color");
        ui.painter()
            .circle_filled(color_rect.center(), 9.0, self.brush.color);
        if self.sidebar_visible || self.color_window_open {
            ui.painter().circle_stroke(
                color_rect.center(),
                11.0,
                egui::Stroke::new(2.0_f32, ui.visuals().text_color()),
            );
        }
        if color_response.clicked() {
            if self.sidebar_visible {
                self.toggle_sidebar();
            } else {
                self.color_window_open = !self.color_window_open;
            }
        }
        selected_tool
    }
}

fn show_selection_tools(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    active_tool: EditorTool,
    has_selection: bool,
    commands: &mut Vec<AppCommand>,
) -> Option<EditorTool> {
    for (index, tool) in [EditorTool::Select, EditorTool::Transform]
        .into_iter()
        .enumerate()
    {
        let tool_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(index as f32 * 40.0, 0.0),
            egui::vec2(40.0, rect.height()),
        );
        let selected = active_tool == tool || (tool == EditorTool::Select && has_selection);
        if show_tool_button(ui, tool_rect, tool, selected).clicked() {
            egui::Popup::close_all(ui.ctx());
            if selected {
                commands.push(AppCommand::Editor(match tool {
                    EditorTool::Select => EditorCommand::CancelSelection,
                    _ => EditorCommand::ApplyLayerTransform,
                }));
            } else {
                return Some(tool);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_controls_select_cancel_and_apply_tools() {
        for (x, active_tool, has_selection, expected_tool, expected_command) in [
            (
                20.0,
                EditorTool::Paint(PaintTool::Brush),
                false,
                Some(EditorTool::Select),
                None,
            ),
            (
                60.0,
                EditorTool::Paint(PaintTool::Brush),
                false,
                Some(EditorTool::Transform),
                None,
            ),
            (
                20.0,
                EditorTool::Select,
                false,
                None,
                Some(EditorCommand::CancelSelection),
            ),
            (
                20.0,
                EditorTool::Paint(PaintTool::Brush),
                true,
                None,
                Some(EditorCommand::CancelSelection),
            ),
            (
                60.0,
                EditorTool::Transform,
                false,
                None,
                Some(EditorCommand::ApplyLayerTransform),
            ),
        ] {
            let context = egui::Context::default();
            let mut commands = Vec::new();
            let mut selected_tool = None;
            let position = egui::pos2(x, 16.0);
            for pressed in [None, Some(true), Some(false)] {
                let mut events = vec![egui::Event::PointerMoved(position)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        selected_tool = show_selection_tools(
                            ui,
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(80.0, 32.0)),
                            active_tool,
                            has_selection,
                            &mut commands,
                        );
                    },
                );
            }
            assert_eq!(selected_tool, expected_tool);
            assert_eq!(commands.len(), usize::from(expected_command.is_some()));
            if let Some(expected) = expected_command {
                assert_eq!(commands, vec![AppCommand::Editor(expected)]);
            }
        }
    }
}
