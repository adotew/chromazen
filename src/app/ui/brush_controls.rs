use super::*;

pub(super) const CONTROLS_HEIGHT: f32 = 348.0;
const CONTROL_CHROME: f32 = 14.0;

impl GuiLayer {
    pub(super) fn show_brush_controls(
        &mut self,
        ui: &mut egui::Ui,
        active_tool: EditorTool,
        height: f32,
    ) {
        let enabled = active_tool.paint_tool().is_some() && !self.canvas_crop_active();
        let track_height = (height * 0.5 - CONTROL_CHROME).clamp(64.0, 160.0);
        egui::ScrollArea::vertical()
            .id_salt("brush controls scroll")
            .max_height(height)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.add_space(10.0);
                    let size_response = brush_slider(
                        ui,
                        "Size",
                        &mut self.brush.size,
                        self.size_range.clone(),
                        track_height,
                        true,
                    );
                    ui.add_space(5.0);

                    let opacity_response = brush_slider(
                        ui,
                        "Opacity",
                        &mut self.brush.opacity,
                        0.01..=1.0,
                        track_height,
                        false,
                    );
                    if (size_response.changed() || opacity_response.changed())
                        && let Some(tool) = active_tool.paint_tool()
                    {
                        self.store_current_brush_settings_for_tool(tool);
                    }
                    let size_active = slider_preview_active(&size_response);
                    let opacity_active = slider_preview_active(&opacity_response);
                    self.brush_slider_active = size_active || opacity_active;
                    self.brush_slider_focus = [&size_response, &opacity_response]
                        .into_iter()
                        .find(|response| response.has_focus())
                        .map(|response| response.id);
                });
            });
    }

    pub(super) fn show_brush_adjustment_preview(
        &mut self,
        ui: &egui::Ui,
        resize_position: Option<[f32; 2]>,
        outline_half_width: f32,
        workspace: egui::Rect,
        tool: EditorTool,
    ) {
        let workspace =
            workspace.with_max_x((workspace.right() - TOOL_RAIL_THICKNESS).max(workspace.left()));
        let enabled = ui.ctx().input(|input| input.focused)
            && tool.paint_tool().is_some()
            && !self.canvas_crop_active();
        let center = adjustment_preview_center(
            enabled,
            resize_position,
            self.brush_slider_active,
            workspace,
            ui.ctx().pixels_per_point(),
        );
        self.brush_adjustment_preview = center.map(|center| chromazen_canvas::BrushCursor {
            center,
            diameter: self.brush.size,
        });
        if let Some(center) = center {
            show_brush_resize_label(
                ui,
                center,
                outline_half_width,
                workspace,
                self.brush.size,
                self.brush.opacity,
            );
        }
    }

    pub(crate) fn brush_slider_active(&self) -> bool {
        self.brush_slider_active
    }
}

fn adjustment_preview_center(
    enabled: bool,
    resize_position: Option<[f32; 2]>,
    slider_active: bool,
    workspace: egui::Rect,
    pixels_per_point: f32,
) -> Option<[f32; 2]> {
    if enabled && workspace.is_positive() {
        // The shortcut is already in physical pixels; egui's workspace is in points.
        resize_position.or(slider_active.then(|| {
            let center = workspace.center() * pixels_per_point;
            [center.x, center.y]
        }))
    } else {
        None
    }
}

fn slider_preview_active(response: &egui::Response) -> bool {
    let keyboard_id = response.id.with("keyboard preview");
    if !response.ctx.input(|input| input.focused) || !response.enabled() {
        response.surrender_focus();
        response
            .ctx
            .data_mut(|data| data.remove::<bool>(keyboard_id));
        return false;
    }
    if response.is_pointer_button_down_on() {
        response.request_focus();
    }
    // Retain focus after a pointer drag so arrow keys can fine-tune the value.
    // Releasing the pointer hides the preview until keyboard adjustment resumes.
    let keyboard_input = response.has_focus()
        && response.ctx.input(|input| {
            input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::ArrowDown)
        });
    let clear_keyboard = response.drag_stopped() || !response.has_focus();
    let start_keyboard =
        keyboard_input || (response.gained_focus() && !response.is_pointer_button_down_on());
    let keyboard_active = response.ctx.data_mut(|data| {
        if clear_keyboard {
            data.remove::<bool>(keyboard_id);
        } else if start_keyboard {
            data.insert_temp(keyboard_id, true);
        }
        data.get_temp::<bool>(keyboard_id).unwrap_or(false)
    });
    response.is_pointer_button_down_on() || keyboard_active
}

fn brush_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    height: f32,
    logarithmic: bool,
) -> egui::Response {
    ui.push_id(label, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let dark = ui.visuals().dark_mode;
        let response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(4.0);
                let style = ui.style_mut();
                style.spacing.slider_width = height;
                style.spacing.interact_size.y = 34.0;
                style.visuals.slider_trailing_fill = false;
                for widget in [
                    &mut style.visuals.widgets.inactive,
                    &mut style.visuals.widgets.hovered,
                    &mut style.visuals.widgets.active,
                ] {
                    widget.bg_fill = egui::Color32::TRANSPARENT;
                    widget.fg_stroke = egui::Stroke::NONE;
                }
                ui.add(
                    egui::Slider::new(value, range.clone())
                        .vertical()
                        .logarithmic(logarithmic)
                        .show_value(false)
                        .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
                )
            })
            .inner;
        let fraction = slider_fraction(*value, &range, logarithmic);
        let track = egui::Rect::from_center_size(
            response.rect.center(),
            egui::vec2(26.0, response.rect.height()),
        );
        let thumb_center = egui::pos2(
            track.center().x,
            egui::lerp((track.top() + 7.0)..=(track.bottom() - 7.0), 1.0 - fraction),
        );
        let thumb = egui::Rect::from_center_size(thumb_center, egui::vec2(24.0, 14.0));
        let (track_shade, idle, hovered, active, disabled) = if dark {
            (48, 105, 135, 155, 60)
        } else {
            (230, 65, 45, 30, 130)
        };
        ui.painter().rect_filled(
            track,
            egui::CornerRadius::same(4),
            egui::Color32::from_gray(track_shade),
        );
        let thumb_shade = if !response.enabled() {
            disabled
        } else if response.is_pointer_button_down_on() {
            active
        } else if response.hovered() {
            hovered
        } else {
            idle
        };
        ui.painter().rect_filled(
            thumb,
            egui::CornerRadius::same(4),
            egui::Color32::from_gray(thumb_shade),
        );
        response
            .widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*value), label));
        response.on_hover_text(label)
    })
    .inner
}

fn slider_fraction(value: f32, range: &std::ops::RangeInclusive<f32>, logarithmic: bool) -> f32 {
    if logarithmic {
        egui::remap_clamp(value.ln(), range.start().ln()..=range.end().ln(), 0.0..=1.0)
    } else {
        egui::remap_clamp(value, range.clone(), 0.0..=1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider_frame(
        context: &egui::Context,
        value: &mut f32,
        events: impl IntoIterator<Item = egui::Event>,
        focused: bool,
    ) -> (egui::Rect, bool) {
        let mut result = (egui::Rect::NOTHING, false);
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 400.0),
                )),
                events: events.into_iter().collect(),
                focused,
                ..Default::default()
            },
            |ui| {
                let response = brush_slider(ui, "Opacity", value, 0.01..=1.0, 140.0, false);
                result = (response.rect, slider_preview_active(&response));
            },
        );
        result
    }

    fn slider_event(context: &egui::Context, value: &mut f32, event: egui::Event) -> bool {
        slider_frame(context, value, [event], true).1
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn slider_thumb_position_matches_linear_and_logarithmic_values() {
        assert_eq!(slider_fraction(0.5, &(0.0..=1.0), false), 0.5);
        assert!((slider_fraction(10.0, &(1.0..=100.0), true) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn slider_drag_keyboard_and_focus_behavior_is_preserved() {
        let context = egui::Context::default();
        let mut value = 0.5;
        let rect = slider_frame(&context, &mut value, [], true).0;
        slider_event(
            &context,
            &mut value,
            egui::Event::PointerMoved(rect.center()),
        );
        assert!(slider_event(
            &context,
            &mut value,
            pointer_button(rect.center(), true)
        ));
        let outside = egui::pos2(rect.right() + 80.0, rect.top() - 20.0);
        assert!(slider_event(
            &context,
            &mut value,
            egui::Event::PointerMoved(outside)
        ));
        assert_eq!(value, 1.0);
        assert!(!slider_event(
            &context,
            &mut value,
            pointer_button(outside, false)
        ));
        assert!(!slider_frame(&context, &mut value, [], true).1);

        value = 0.5;
        let before = value;
        assert!(slider_event(
            &context,
            &mut value,
            egui::Event::Key {
                key: egui::Key::ArrowUp,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }
        ));
        assert!(value > before);
        assert!(!slider_frame(&context, &mut value, [], false).1);
    }

    #[test]
    fn adjustment_preview_uses_correct_position_and_lifetime() {
        let workspace = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(810.0, 620.0));
        assert_eq!(
            adjustment_preview_center(true, None, true, workspace, 2.0),
            Some([820.0, 640.0])
        );
        assert_eq!(
            adjustment_preview_center(true, Some([73.0, 91.0]), true, workspace, 2.0),
            Some([73.0, 91.0])
        );
        assert_eq!(
            adjustment_preview_center(true, None, false, workspace, 1.0),
            None
        );
        assert_eq!(
            adjustment_preview_center(false, Some([20.0, 30.0]), true, workspace, 1.0),
            None
        );
    }
}
