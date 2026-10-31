use super::*;

impl GuiLayer {
    pub(super) fn show_brush_panel_controls(&mut self, ui: &mut egui::Ui, tool: PaintTool) {
        egui::Frame::NONE.inner_margin(10.0).show(ui, |ui| {
            ui.add_enabled_ui(!self.canvas_crop_active(), |ui| {
                let spacing = ui.spacing().item_spacing.x;
                ui.spacing_mut().slider_width = (ui.available_width() - 52.0 - spacing).max(24.0);
                egui::Grid::new("brush panel controls")
                    .spacing(egui::vec2(spacing, 12.0))
                    .show(ui, |ui| {
                        let size_response = panel_slider(
                            ui,
                            Some("Size"),
                            &mut self.brush.size,
                            self.size_range.clone(),
                            true,
                            true,
                        );
                        let opacity_response = panel_slider(
                            ui,
                            Some("Opacity"),
                            &mut self.brush.opacity,
                            0.01..=1.0,
                            false,
                            true,
                        );
                        if size_response.changed() || opacity_response.changed() {
                            self.store_current_brush_settings_for_tool(tool);
                        }
                        self.track_brush_sliders(&size_response, &opacity_response);
                    });
            });
        });
    }

    fn track_brush_sliders(&mut self, size: &egui::Response, opacity: &egui::Response) {
        self.brush_slider_active |= slider_preview_active(size) | slider_preview_active(opacity);
        self.brush_slider_focus = [size, opacity]
            .into_iter()
            .find(|response| response.has_focus())
            .map(|response| response.id)
            .or(self.brush_slider_focus);
    }

    pub(super) fn show_brush_adjustment_preview(
        &mut self,
        ui: &egui::Ui,
        resize_position: Option<[f32; 2]>,
        outline_half_width: f32,
        workspace: egui::Rect,
        tool: EditorTool,
    ) {
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
            input.key_pressed(egui::Key::ArrowUp)
                || input.key_pressed(egui::Key::ArrowDown)
                || input.key_pressed(egui::Key::ArrowLeft)
                || input.key_pressed(egui::Key::ArrowRight)
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

pub(super) fn panel_slider(
    ui: &mut egui::Ui,
    label: Option<&str>,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: bool,
    value_bubble: bool,
) -> egui::Response {
    if let Some(label) = label {
        ui.allocate_ui_with_layout(
            egui::vec2(52.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(52.0, 20.0));
                ui.label(label);
            },
        );
    }
    let mut fraction = slider_fraction(*value, &range, size);
    let response = ui
        .scope(|ui| {
            ui.set_opacity(0.0);
            ui.add(
                egui::Slider::new(&mut fraction, 0.0..=1.0)
                    .show_value(false)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
        })
        .inner;
    let rect = response.rect;
    // Match egui's handle travel while retaining the full-height interaction area.
    let travel = rect.x_range().shrink(rect.height() / 2.5);
    let center = egui::pos2(egui::lerp(travel, fraction), rect.center().y);
    let dark_mode = ui.visuals().dark_mode;
    let track = egui::Color32::from_gray(if dark_mode { 72 } else { 190 });
    let fill = egui::Color32::from_gray(if dark_mode { 170 } else { 105 });
    let thumb = egui::Color32::from_gray(if dark_mode { 238 } else { 65 });
    let mut painter = ui.painter().clone();
    painter.set_opacity(if ui.is_enabled() { 1.0 } else { 0.5 });
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(2.0, track),
    );
    painter.line_segment([rect.left_center(), center], egui::Stroke::new(2.0, fill));
    painter.circle_filled(center, 5.0, thumb);
    if response.has_focus() {
        painter.circle_stroke(center, 7.0, ui.visuals().selection.stroke);
    }
    if response.changed() {
        *value = slider_value(fraction, &range, size);
    }
    if value_bubble
        && response.enabled()
        && ui.input(|input| input.focused)
        && (response.hovered() || response.is_pointer_button_down_on() || response.has_focus())
    {
        let text = if size {
            format!("{value:.0} px")
        } else {
            format!("{:.0}%", *value * 100.0)
        };
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            response.id.with("value bubble"),
        ));
        let galley = painter.layout_no_wrap(
            text,
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        let bounds =
            egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.ctx().content_rect().y_range())
                .intersect(ui.ctx().content_rect());
        let padding = egui::vec2(6.0, 3.0);
        let bubble = slider_value_bubble_rect(center, galley.size() + 2.0 * padding, bounds);
        painter.rect(
            bubble,
            6,
            ui.visuals().extreme_bg_color,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        painter.galley(bubble.min + padding, galley, ui.visuals().text_color());
    }
    ui.end_row();
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            ui.is_enabled(),
            f64::from(*value),
            label.unwrap_or_default(),
        )
    });
    response
}

fn slider_value_bubble_rect(
    center: egui::Pos2,
    size: egui::Vec2,
    bounds: egui::Rect,
) -> egui::Rect {
    let left = (center.x - size.x / 2.0)
        .clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left()));
    let top = (center.y - 10.0 - size.y).max(bounds.top());
    egui::Rect::from_min_size(egui::pos2(left, top), size)
}

// Like Procreate, size eases in quadratically: fine control for small brushes,
// fast travel to large ones, without log scaling's cramped top end.
fn slider_fraction(value: f32, range: &std::ops::RangeInclusive<f32>, curved: bool) -> f32 {
    let linear = egui::remap_clamp(value, range.clone(), 0.0..=1.0);
    if curved { linear.sqrt() } else { linear }
}

fn slider_value(fraction: f32, range: &std::ops::RangeInclusive<f32>, curved: bool) -> f32 {
    let linear = if curved {
        fraction * fraction
    } else {
        fraction
    };
    egui::remap_clamp(linear, 0.0..=1.0, range.clone())
}

#[cfg(test)]
mod tests;
