use super::*;

impl GuiLayer {
    pub fn run_editor(&mut self, window: &Window, state: EditorUiState<'_>) -> egui::FullOutput {
        let EditorUiState {
            layers,
            tool,
            layer_transform,
            layer_content_bounds,
            brush_resize_label,
            eyedropper_indicator,
            save_status,
            pending_navigation,
            brush_import_dialog_delay,
            reference_import_dialog_delay,
            reference_load_dialog_delay,
            references,
            workspace_view,
        } = state;
        if let Some(paint_tool) = tool.paint_tool() {
            let index = tool_index(paint_tool);
            self.tool_sizes[index] = self.brush.size;
            self.tool_opacities[index] = self.brush.opacity;
            self.ensure_brush_preview_cached(&self.tool_brushes[tool_index(paint_tool)].clone());
        }
        self.sync_reference_textures(references);
        let raw_input = self.state.take_egui_input(window);
        let context = self.context.clone();

        context.run_ui(raw_input, |ui| {
            if !ui.ctx().egui_wants_keyboard_input()
                && ui
                    .ctx()
                    .input(|input| input.key_pressed(egui::Key::Questionmark))
            {
                self.shortcuts_dialog_open = true;
            }
            let background = background_color(layers.background_color);
            if let Some(id) = self.selected_reference
                && !ui.ctx().egui_wants_keyboard_input()
                && ui.ctx().input(|input| input.key_pressed(egui::Key::Delete))
            {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::DeleteReference(id)));
            }

            // Keep the full-width contents clipped to the animated panel so the sidebar slides
            // away instead of flashing empty while egui resizes it.
            let sidebar_progress = ui.ctx().animate_bool_with_time_and_easing(
                egui::Id::new("sidebar animation"),
                self.sidebar_visible,
                0.18,
                egui::emath::easing::cubic_in_out,
            );
            if sidebar_progress > 0.0 {
                let sidebar_frame = egui::Frame::side_top_panel(ui.style())
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE);
                egui::Panel::right("tools")
                    .frame(sidebar_frame)
                    .show_separator_line(false)
                    .exact_size(SIDEBAR_WIDTH * sidebar_progress)
                    .resizable(false)
                    .show_inside(ui, |panel_ui| {
                        let component_rect = egui::Frame::side_top_panel(panel_ui.style())
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .widget_rect(panel_ui.max_rect());
                        paint_rounded_panel(panel_ui, component_rect, egui::CornerRadius::ZERO);
                        let inner_width = SIDEBAR_WIDTH
                            - egui::Frame::side_top_panel(panel_ui.style())
                                .inner_margin
                                .sum()
                                .x;
                        let content_rect = egui::Rect::from_min_size(
                            panel_ui.min_rect().min,
                            egui::vec2(inner_width, panel_ui.available_height()),
                        );
                        let mut content_ui = panel_ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("sidebar contents")
                                .max_rect(content_rect),
                        );
                        content_ui.set_clip_rect(panel_ui.clip_rect());

                        self.show_brush_color_picker(&mut content_ui);
                        content_ui.add_space(8.0);
                        content_ui.separator();
                        if add_layer_button(&mut content_ui) {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::AddLayer));
                        }
                        self.show_layers_panel(&mut content_ui, layers, background);
                    });
            }

            if !self.sidebar_visible
                && self.color_window_open
                && let Some(response) = egui::Window::new("Color")
                    .id(egui::Id::new("floating color picker"))
                    .default_pos(egui::pos2(
                        self.panel_layout.color_panel_pos[0],
                        self.panel_layout.color_panel_pos[1],
                    ))
                    .default_width(280.0)
                    .resizable(false)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| self.show_brush_color_picker(ui))
            {
                let min = response.response.rect.min;
                self.panel_layout.color_panel_pos = [min.x, min.y];
            }

            if self.brush_window_open
                && let Some(paint_tool) = tool.paint_tool()
                && let Some(response) = egui::Window::new("Brush")
                    .id(egui::Id::new("floating brush"))
                    .default_pos(egui::pos2(
                        self.panel_layout.brush_panel_pos[0],
                        self.panel_layout.brush_panel_pos[1],
                    ))
                    .auto_sized()
                    .default_width(BRUSH_PANEL_WIDTH)
                    .min_width(BRUSH_PANEL_WIDTH)
                    .max_width(BRUSH_PANEL_WIDTH)
                    .resizable(false)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| self.show_brush_panel(ui, paint_tool))
            {
                let min = response.response.rect.min;
                self.panel_layout.brush_panel_pos = [min.x, min.y];
            }

            if !self.sidebar_visible && self.layers_window_open {
                let layers_response = egui::Window::new("Layers")
                    .id(egui::Id::new("floating layers"))
                    .default_pos(egui::pos2(
                        self.panel_layout.layers_panel_pos[0],
                        self.panel_layout.layers_panel_pos[1],
                    ))
                    .auto_sized()
                    .default_width(LAYER_PANEL_WIDTH)
                    .min_width(LAYER_PANEL_WIDTH)
                    .max_width(LAYER_PANEL_WIDTH)
                    .max_height(LAYER_LIST_MAX_HEIGHT + 48.0)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| {
                        self.show_layers_panel(ui, layers, background)
                    });
                if let Some(response) = layers_response {
                    let min = response.response.rect.min;
                    self.panel_layout.layers_panel_pos = [min.x, min.y];
                    let button_rect = egui::Rect::from_min_size(
                        response.response.rect.left_top() + egui::vec2(8.0, 6.0),
                        egui::Vec2::splat(28.0),
                    );
                    let mut button_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id(egui::Id::new("floating layer add button"))
                            .layer_id(response.response.layer_id)
                            .max_rect(button_rect),
                    );
                    button_ui.set_clip_rect(response.response.rect);
                    if add_layer_button(&mut button_ui) {
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::AddLayer));
                    }
                }
            }

            if !ui.ctx().egui_wants_keyboard_input() {
                if ui.ctx().input(|input| input.key_pressed(egui::Key::Enter))
                    && tool == EditorTool::Transform
                {
                    self.layer_transform_drag = None;
                    self.commands
                        .push(AppCommand::Editor(EditorCommand::ApplyLayerTransform));
                }
                if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
                    if tool == EditorTool::Transform {
                        self.layer_transform_drag = None;
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::CancelLayerTransform));
                    } else {
                        self.brush_window_open = false;
                        self.color_window_open = false;
                        self.layers_window_open = false;
                    }
                }
            }

            let workspace_rect = ui.available_rect_before_wrap();
            self.show_workspace_references(ui.ctx(), references, workspace_view, workspace_rect);
            if tool == EditorTool::Transform {
                self.show_layer_transform_overlay(
                    ui,
                    workspace_view,
                    layer_content_bounds,
                    layer_transform,
                    workspace_rect,
                );
            } else {
                self.layer_transform_drag = None;
            }

            let selected_tool = egui::Area::new(egui::Id::new("tool rail"))
                .anchor(
                    egui::Align2::RIGHT_TOP,
                    egui::vec2(-SIDEBAR_WIDTH * sidebar_progress, 12.0),
                )
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| self.show_toolbar(ui, tool))
                .inner;
            if let Some(tool) = selected_tool {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::SelectTool(tool)));
            }
            if let Some(label) = brush_resize_label {
                show_brush_resize_label(ui, label, self.brush.size, self.brush.opacity);
            }
            if let Some(indicator) = eyedropper_indicator {
                show_eyedropper_indicator(ui, indicator);
            }
            if let Some(action) = pending_navigation {
                show_save_blocker(ui.ctx(), action, &save_status, &mut self.commands);
            }
            if let Some(delay) = brush_import_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(
                        ui.ctx(),
                        "brush import dialog",
                        "Importing Photoshop brushes…",
                    );
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            if let Some(delay) = reference_import_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(
                        ui.ctx(),
                        "reference import dialog",
                        "Importing reference…",
                    );
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            if let Some(delay) = reference_load_dialog_delay {
                if delay.is_zero() {
                    show_loading_dialog(ui.ctx(), "reference load dialog", "Loading references…");
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
            }
            self.show_new_artwork_dialog(ui.ctx());
            self.show_canvas_crop_overlay(ui.ctx(), workspace_view, workspace_rect);
            self.clear_reference_selection_on_outside_press(ui.ctx());
            self.show_message_dialog(ui.ctx());
            self.show_shortcuts_dialog(ui.ctx());
        })
    }
}
