use super::*;

use crate::config::DEFAULT_ACCENT_COLOR;

const ACCENT_COLORS: [(&str, egui::Color32); 7] = [
    (
        "Blue",
        egui::Color32::from_rgb(
            DEFAULT_ACCENT_COLOR[0],
            DEFAULT_ACCENT_COLOR[1],
            DEFAULT_ACCENT_COLOR[2],
        ),
    ),
    ("Teal", egui::Color32::from_rgb(0, 151, 167)),
    ("Green", egui::Color32::from_rgb(46, 155, 98)),
    ("Orange", egui::Color32::from_rgb(217, 119, 6)),
    ("Red", egui::Color32::from_rgb(220, 76, 76)),
    ("Pink", egui::Color32::from_rgb(216, 79, 139)),
    ("Purple", egui::Color32::from_rgb(142, 93, 231)),
];

const SETTINGS_SIDEBAR_WIDTH: f32 = 150.0;
const SETTINGS_CONTENT_WIDTH: f32 = 520.0;
const SETTINGS_LABEL_WIDTH: f32 = 240.0;
const SETTINGS_MIN_HEIGHT: f32 = 280.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsPage {
    Appearance,
}

impl SettingsPage {
    const ALL: [Self; 1] = [Self::Appearance];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
        }
    }
}

/// Returns whether any appearance setting changed.
fn appearance_settings(
    ui: &mut egui::Ui,
    accent_color: &mut egui::Color32,
    workspace_background: &mut WorkspaceBackground,
    surface_style: &mut SurfaceStyle,
) -> bool {
    let mut changed = false;
    setting_row(
        ui,
        "Accent color",
        "Highlights selections and active controls.",
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (name, color) in ACCENT_COLORS.into_iter().rev() {
                changed |= accent_swatch(ui, name, color, accent_color);
            }
        },
    );
    setting_row(
        ui,
        "Workspace background",
        "Color of the area around the canvas.",
        |ui| {
            changed |= ui
                .selectable_value(
                    workspace_background,
                    WorkspaceBackground::NeutralGray,
                    "Neutral Gray",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    workspace_background,
                    WorkspaceBackground::Standard,
                    "Standard",
                )
                .changed();
        },
    );
    setting_row(
        ui,
        "Interface backgrounds",
        "Use solid fills or blur the canvas behind panels.",
        |ui| {
            changed |= ui
                .selectable_value(surface_style, SurfaceStyle::Frosted, "Frosted")
                .changed();
            changed |= ui
                .selectable_value(surface_style, SurfaceStyle::Opaque, "Opaque")
                .changed();
        },
    );
    changed
}

/// Lays out a title and description on the left and a right-aligned control.
/// The control is added right to left, so it must add its widgets in reverse.
fn setting_row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        let text = ui.vertical(|ui| {
            ui.set_width(SETTINGS_LABEL_WIDTH);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(egui::RichText::new(title).color(ui.visuals().strong_text_color()));
            ui.label(egui::RichText::new(description).text_style(egui::TextStyle::Small));
        });
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), text.response.rect.height()),
            egui::Layout::right_to_left(egui::Align::Center),
            add_control,
        );
    });
    ui.add_space(20.0);
}

fn settings_nav_item(ui: &mut egui::Ui, title: &str, selected: bool) -> bool {
    ui.add_sized(
        egui::vec2(ui.available_width(), 30.0),
        egui::Button::selectable(selected, title),
    )
    .clicked()
}

/// Lays out a label on the left and a number field with its unit on the right.
fn dimension_row(ui: &mut egui::Ui, label: &str, value: &mut u32, max: u32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak("px");
            ui.add(egui::DragValue::new(value).range(1..=max).speed(1));
        });
    });
}

fn accent_swatch(
    ui: &mut egui::Ui,
    name: &str,
    color: egui::Color32,
    accent_color: &mut egui::Color32,
) -> bool {
    let selected = color == *accent_color;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(30.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, name)
    });
    ui.painter().circle_filled(rect.center(), 10.0, color);
    if selected || response.hovered() {
        ui.painter().circle_stroke(
            rect.center(),
            13.5,
            egui::Stroke::new(
                if selected { 2.0_f32 } else { 1.0_f32 },
                ui.visuals().text_color(),
            ),
        );
    }
    let clicked = response.on_hover_text(name).clicked();
    if clicked {
        *accent_color = color;
    }
    clicked && !selected
}

impl GuiLayer {
    pub(crate) fn open_settings_dialog(&mut self) {
        self.settings_dialog_open = true;
        self.context.request_repaint();
    }

    pub(crate) fn open_shortcuts_dialog(&mut self) {
        self.shortcuts_dialog_open = true;
        self.context.request_repaint();
    }

    pub(crate) fn open_new_artwork_dialog(&mut self) {
        self.close_canvas_crop();
        self.new_artwork_dialog = Some(NewArtworkDialog {
            width: DEFAULT_CANVAS_SIZE[0],
            height: DEFAULT_CANVAS_SIZE[1],
        });
        self.context.request_repaint();
    }

    pub(crate) fn close_new_artwork_dialog(&mut self) {
        self.new_artwork_dialog = None;
    }

    pub(super) fn show_new_artwork_dialog(&mut self, context: &egui::Context) {
        let Some(dialog) = self.new_artwork_dialog.as_mut() else {
            return;
        };
        let mut close = false;
        let mut create = None;
        let frame = egui::Frame::popup(&context.global_style()).inner_margin(24);
        let response = egui::Modal::new(egui::Id::new("new artwork dialog"))
            .frame(frame)
            .show(context, |ui| {
                ui.set_width(260.0);
                ui.heading("New Artwork");
                ui.add_space(16.0);
                let max_dimension = self.canvas_size_constraints.max_dimension;
                dimension_row(ui, "Width", &mut dialog.width, max_dimension);
                ui.add_space(10.0);
                dimension_row(ui, "Height", &mut dialog.height, max_dimension);
                let validation = self
                    .canvas_size_constraints
                    .validate([dialog.width, dialog.height]);
                if let Err(error) = &validation {
                    ui.add_space(8.0);
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let submit = ui.add_enabled(validation.is_ok(), egui::Button::new("Create"));
                    if submit.clicked()
                        || (validation.is_ok()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    {
                        create = Some((dialog.width, dialog.height));
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        close |= response.should_close();
        if let Some((width, height)) = create {
            self.commands
                .push(AppCommand::Navigation(NavigationCommand::CreateArtwork {
                    width,
                    height,
                }));
            close = true;
        }
        if close {
            self.new_artwork_dialog = None;
        }
    }

    pub(crate) fn open_error_dialog(
        &mut self,
        message: impl Into<String>,
        details: impl std::fmt::Display,
    ) {
        self.message_dialog = Some(MessageDialog::error(message, details));
        self.context.request_repaint();
    }

    pub(crate) fn open_success_dialog(&mut self, message: impl Into<String>) {
        self.message_dialog = Some(MessageDialog::success(message));
        self.context.request_repaint();
    }

    pub(super) fn show_message_dialog(&mut self, context: &egui::Context) {
        let Some(dialog) = self.message_dialog.as_ref() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("message dialog")).show(context, |ui| {
            ui.set_width(320.0);
            ui.heading(dialog.title);
            ui.add_space(8.0);
            ui.label(&dialog.message);
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button("OK").clicked()
            })
            .inner
        });
        if response.inner || context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.message_dialog = None;
        }
    }

    pub(super) fn show_settings_dialog(&mut self, context: &egui::Context) {
        if !self.settings_dialog_open {
            return;
        }
        let mut page = self.settings_page;
        let mut accent_color = self.accent_color;
        let mut surface_style = self.surface_style;
        let mut workspace_background = self.workspace_background;
        let mut changed = false;
        let mut close = false;
        let frame = egui::Frame::popup(&context.global_style()).inner_margin(0);
        let response = egui::Modal::new(egui::Id::new("settings dialog"))
            .frame(frame)
            .show(context, |ui| {
                let visuals = ui.visuals_mut();
                visuals.selection.bg_fill = visuals.text_color().gamma_multiply(0.08);
                visuals.selection.stroke = egui::Stroke::new(1.0, visuals.strong_text_color());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: 20,
                            right: 12,
                            top: 20,
                            bottom: 20,
                        })
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(SETTINGS_SIDEBAR_WIDTH);
                                ui.set_min_height(SETTINGS_MIN_HEIGHT);
                                ui.heading("Settings");
                                ui.add_space(12.0);
                                for candidate in SettingsPage::ALL {
                                    if settings_nav_item(ui, candidate.title(), page == candidate) {
                                        page = candidate;
                                    }
                                }
                            });
                        });
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(24, 20))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(SETTINGS_CONTENT_WIDTH);
                                ui.heading(page.title());
                                ui.add_space(12.0);
                                match page {
                                    SettingsPage::Appearance => {
                                        changed |= appearance_settings(
                                            ui,
                                            &mut accent_color,
                                            &mut workspace_background,
                                            &mut surface_style,
                                        );
                                    }
                                }
                                let button_height = ui.spacing().interact_size.y;
                                let used_height = ui.min_rect().height();
                                ui.add_space(
                                    (SETTINGS_MIN_HEIGHT - used_height - button_height).max(16.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        close = ui.button("Done").clicked();
                                    },
                                );
                            });
                        });
                });
            });
        close |= response.should_close();
        self.settings_page = page;
        if changed {
            self.set_accent_color(accent_color);
            self.set_surface_style(surface_style);
            self.workspace_background = workspace_background;
        }
        if close {
            self.settings_dialog_open = false;
            self.commands
                .push(AppCommand::Settings(SettingsCommand::Save));
        }
    }

    pub(super) fn set_surface_style(&mut self, surface_style: SurfaceStyle) {
        self.surface_style = surface_style;
        self.context.all_styles_mut(|style| {
            style.visuals.window_fill = surface_fill(style.visuals.dark_mode, surface_style);
        });
        self.context.request_repaint();
    }

    pub(super) fn set_accent_color(&mut self, accent_color: egui::Color32) {
        self.accent_color = accent_color;
        self.context
            .all_styles_mut(|style| apply_accent_color_to_style(style, accent_color));
        self.context.request_repaint();
    }

    pub(super) fn show_shortcuts_dialog(&mut self, context: &egui::Context) {
        if !self.shortcuts_dialog_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("keyboard shortcuts")).show(context, |ui| {
            ui.set_width(700.0);
            ui.add_space(16.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 0))
                .show(ui, |ui| {
                    ui.heading("Keyboard Shortcuts");
                });
            ui.add_space(16.0);
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(520.0)
                        .show(ui, |ui| {
                            shortcut_section(
                                ui,
                                "Tools",
                                &[
                                    ("Brush / Eraser / Smudge", "D/B, E, S"),
                                    ("Transform", "T"),
                                    ("Cycle paint tools", "Shift-Tab"),
                                    ("Show or hide sidebar", "Tab"),
                                    ("Show or hide artwork tabs", RAIL_SHORTCUT),
                                    ("Resize brush", "Shift-drag"),
                                    ("Eyedropper", "Alt/Option"),
                                ],
                            );
                            ui.add_space(14.0);
                            ui.add_space(14.0);
                            shortcut_section(
                                ui,
                                "Canvas",
                                &[
                                    ("Pan", "Space-drag or middle/right-drag"),
                                    ("Zoom", "Wheel"),
                                    ("Rotate freely", "R-drag"),
                                    ("Reset rotation", "Shift-R"),
                                    ("Rotate left / right", ROTATE_SHORTCUT),
                                    ("Flip horizontal / vertical", FLIP_SHORTCUT),
                                    ("Crop or resize", CROP_SHORTCUT),
                                ],
                            );
                            ui.add_space(14.0);
                            ui.add_space(14.0);
                            shortcut_section(
                                ui,
                                "Document",
                                &[
                                    ("Settings", SETTINGS_SHORTCUT),
                                    ("Save", SAVE_SHORTCUT),
                                    ("Export PNG", EXPORT_SHORTCUT),
                                    ("Undo", UNDO_SHORTCUT),
                                    ("Redo", REDO_SHORTCUT),
                                    ("Apply transform or crop", "Enter"),
                                    ("Cancel transform or crop", "Escape"),
                                ],
                            );
                        });
                });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button("Close").clicked()
            })
            .inner
        });
        if response.inner || context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.shortcuts_dialog_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_colors_include_original_blue() {
        let original = egui::Color32::from_rgb(
            DEFAULT_ACCENT_COLOR[0],
            DEFAULT_ACCENT_COLOR[1],
            DEFAULT_ACCENT_COLOR[2],
        );
        assert!(ACCENT_COLORS.iter().any(|(_, color)| *color == original));
    }
}
