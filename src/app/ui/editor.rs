use super::*;

impl GuiLayer {
    pub fn run_editor(&mut self, window: &Window, state: EditorUiState<'_>) -> egui::FullOutput {
        let EditorUiState {
            menu,
            layers,
            tool,
            layer_transform,
            layer_content_bounds,
            selection,
            lasso,
            brush_resize_position,
            brush_outline_half_size,
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
        self.brush_slider_active = false;
        self.brush_slider_focus = None;
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
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(0.0);
                egui::Panel::right("tools")
                    .frame(sidebar_frame)
                    .show_separator_line(false)
                    .exact_size(SIDEBAR_WIDTH * sidebar_progress)
                    .resizable(false)
                    .show_inside(ui, |panel_ui| {
                        let layer_id =
                            egui::LayerId::new(egui::Order::Middle, egui::Id::new("tools sidebar"));
                        let panel_rect = panel_ui.max_rect();
                        egui::Area::new(layer_id.id)
                            .order(layer_id.order)
                            .fixed_pos(panel_rect.min)
                            .default_size(panel_rect.size())
                            .show(panel_ui.ctx(), |ui| ui.set_min_size(panel_rect.size()));
                        panel_ui.ctx().move_to_top(layer_id);
                        let mut panel_ui = panel_ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("foreground")
                                .layer_id(layer_id),
                        );
                        show_sidebar_frame(&mut panel_ui, |content_ui| {
                            content_ui.add_space(8.0);
                            let header_width = content_ui.available_width();
                            content_ui.allocate_ui(egui::vec2(header_width, 28.0), |ui| {
                                ui.columns(3, |columns| {
                                    columns[1].vertical_centered(|ui| {
                                        ui.heading("Color");
                                    });
                                    columns[0].with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| self.show_brush_color_swatch(ui),
                                    );
                                });
                            });
                            content_ui.add_space(8.0);
                            self.show_brush_color_picker(content_ui);
                            content_ui.add_space(8.0);
                            let stroke = content_ui.visuals().widgets.noninteractive.bg_stroke;
                            content_ui
                                .visuals_mut()
                                .widgets
                                .noninteractive
                                .bg_stroke
                                .color = stroke.color.gamma_multiply(0.5);
                            content_ui.add(egui::Separator::default().grow(8.0));
                            content_ui.visuals_mut().widgets.noninteractive.bg_stroke = stroke;
                            if let Some(paint_tool) = tool.paint_tool() {
                                content_ui.add_space(8.0);
                                content_ui.vertical_centered(|ui| {
                                    ui.heading("Brush");
                                });
                                content_ui.add_space(4.0);
                                self.show_brush_panel(content_ui, paint_tool);
                                content_ui.add_space(8.0);
                                content_ui
                                    .visuals_mut()
                                    .widgets
                                    .noninteractive
                                    .bg_stroke
                                    .color = stroke.color.gamma_multiply(0.5);
                                content_ui.add(egui::Separator::default().grow(8.0));
                                content_ui.visuals_mut().widgets.noninteractive.bg_stroke = stroke;
                            }
                            content_ui.add_space(8.0);
                            let header_width = content_ui.available_width();
                            content_ui.allocate_ui(egui::vec2(header_width, 28.0), |ui| {
                                ui.columns(3, |columns| {
                                    columns[1].vertical_centered(|ui| {
                                        ui.heading("Layers");
                                    });
                                    columns[0].with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if add_layer_button(ui) {
                                                self.commands.push(AppCommand::Editor(
                                                    EditorCommand::AddLayer,
                                                ));
                                            }
                                        },
                                    );
                                });
                            });
                            content_ui.add_space(8.0);
                            self.show_layers_panel(content_ui, layers, background);
                        });
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
                let swatch_rect = egui::Rect::from_min_size(
                    response.response.rect.left_top() + egui::vec2(8.0, 8.0),
                    egui::vec2(72.0, 24.0),
                );
                let mut swatch_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id(egui::Id::new("floating color swatch"))
                        .layer_id(response.response.layer_id)
                        .max_rect(swatch_rect),
                );
                swatch_ui.set_clip_rect(response.response.rect);
                self.show_brush_color_swatch(&mut swatch_ui);
            }

            if !self.sidebar_visible
                && self.brush_window_open
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
                if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape))
                    && !self.close_quick_menu()
                {
                    if tool == EditorTool::Transform
                        && (layer_transform.is_some() || selection.is_none())
                    {
                        self.layer_transform_drag = None;
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::CancelLayerTransform));
                    } else if selection.is_some() {
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::ClearSelection));
                    } else {
                        self.brush_window_open = false;
                        self.color_window_open = false;
                        self.layers_window_open = false;
                    }
                }
            }

            let workspace_rect = ui.available_rect_before_wrap();
            self.show_workspace_references(ui.ctx(), references, workspace_view, workspace_rect);
            if let Some(selection) = selection
                && layer_transform.is_none()
            {
                self.show_selection_outline(ui, workspace_view, selection, true, workspace_rect);
            }
            if let Some(lasso) = lasso {
                self.show_selection_outline(ui, workspace_view, lasso, false, workspace_rect);
            }
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

            let selected_tool = egui::Area::new(egui::Id::new("editor controls"))
                .anchor(
                    egui::Align2::LEFT_TOP,
                    egui::Vec2::splat(crate::platform::WINDOW_CONTROLS_MARGIN),
                )
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    self.show_editor_controls(
                        ui,
                        menu,
                        tool,
                        selection.is_some(),
                        has_macos_titlebar(window.fullscreen().is_some()),
                    )
                })
                .inner;
            if let Some(tool) = selected_tool {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::SelectTool(tool)));
            }
            let selected_tool = egui::Area::new(egui::Id::new("toolbar"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| self.show_toolbar(ui, tool))
                .inner;
            if let Some(tool) = selected_tool {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::SelectTool(tool)));
            }
            let half_size = brush_outline_half_size(self.brush.size);
            let (axis_x, axis_y) = workspace_view.document_axes_in_window();
            let outline_half_width =
                half_size[0] * axis_x[0].abs() + half_size[1] * axis_y[0].abs();
            self.show_brush_adjustment_preview(
                ui,
                brush_resize_position,
                outline_half_width,
                workspace_rect,
                tool,
            );
            if let Some(indicator) = eyedropper_indicator {
                show_eyedropper_indicator(ui, indicator);
            }
            self.show_quick_menu(ui.ctx(), menu, tool, layers);
            if let Some((action, delay)) = pending_navigation {
                if delay.is_zero() || matches!(save_status, SaveStatus::Failed(_)) {
                    show_save_blocker(ui.ctx(), action, &save_status, &mut self.commands);
                } else {
                    ui.ctx().request_repaint_after(delay);
                }
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
            self.show_settings_dialog(ui.ctx());
            self.show_shortcuts_dialog(ui.ctx());
        })
    }
}

fn show_sidebar_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let bounds = egui::Rect::from_min_size(
        ui.max_rect().min,
        egui::vec2(SIDEBAR_WIDTH, ui.max_rect().height()),
    )
    .shrink(12.0);
    let mut frame_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("sidebar contents")
            .max_rect(bounds),
    );
    frame_ui.set_clip_rect(ui.clip_rect().intersect(bounds));
    egui::Frame::new()
        .fill(ui.visuals().window_fill())
        .stroke(ui.visuals().window_stroke)
        .corner_radius(28)
        .inner_margin(16.0)
        .show(&mut frame_ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_frame_tracks_content_instead_of_window_height() {
        let context = egui::Context::default();
        for (window_height, content_height) in [(1600.0, 600.0), (2000.0, 300.0), (1200.0, 500.0)] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, window_height),
                )),
                ..Default::default()
            };
            let output = context.run_ui(input, |ui| {
                show_sidebar_frame(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), content_height),
                            egui::Sense::hover(),
                        );
                    });
                });
            });
            let panel = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.corner_radius == egui::CornerRadius::same(28) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("sidebar background");
            assert!((panel.width() - (SIDEBAR_WIDTH - 24.0)).abs() < 1.0);
            assert!((panel.height() - (content_height + 32.0)).abs() < 3.0);
        }
    }
}
