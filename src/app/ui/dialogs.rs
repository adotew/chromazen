use super::*;

use crate::config::DEFAULT_ACCENT_COLOR;

const ACCENT_COLORS: [(&str, egui::Color32); 8] = [
    (
        "Monochrome",
        egui::Color32::from_rgb(
            DEFAULT_ACCENT_COLOR[0],
            DEFAULT_ACCENT_COLOR[1],
            DEFAULT_ACCENT_COLOR[2],
        ),
    ),
    ("Blue", egui::Color32::from_rgb(10, 132, 255)),
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

    fn icon(self) -> egui::ImageSource<'static> {
        match self {
            Self::Appearance => egui::include_image!("../../../assets/icons/palette.svg"),
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
        |ui| changed |= accent_color_dropdown(ui, accent_color),
    );
    setting_row(
        ui,
        "Workspace background",
        "Color of the area around the canvas.",
        |ui| {
            changed |= segmented_control(
                ui,
                workspace_background,
                [
                    (WorkspaceBackground::Standard, "Standard"),
                    (WorkspaceBackground::NeutralGray, "Neutral Gray"),
                ],
            );
        },
    );
    setting_row(
        ui,
        "Interface backgrounds",
        "Use solid fills or blur the canvas behind panels.",
        |ui| {
            changed |= segmented_control(
                ui,
                surface_style,
                [
                    (SurfaceStyle::Opaque, "Opaque"),
                    (SurfaceStyle::Frosted, "Frosted"),
                ],
            );
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
            ui.label(egui::RichText::new(description).size(12.0));
        });
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), text.response.rect.height()),
            egui::Layout::right_to_left(egui::Align::Center),
            add_control,
        );
    });
    ui.add_space(20.0);
}

/// A pill-shaped two-option switch whose highlight slides to the selected option.
fn segmented_control<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: [(T, &str); 2],
) -> bool {
    const PADDING: f32 = 3.0;
    const SEGMENT_HEIGHT: f32 = 26.0;
    const SEGMENT_MARGIN: f32 = 14.0;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galleys = options.map(|(_, label)| {
        ui.painter()
            .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
    });
    let segment_size = egui::vec2(
        galleys[0].size().x.max(galleys[1].size().x) + 2.0 * SEGMENT_MARGIN,
        SEGMENT_HEIGHT,
    );
    let size = egui::vec2(2.0 * segment_size.x, segment_size.y) + egui::Vec2::splat(2.0 * PADDING);
    let (track, track_response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let segment = |offset: f32| {
        egui::Rect::from_min_size(
            track.min + egui::vec2(PADDING + offset * segment_size.x, PADDING),
            segment_size,
        )
    };
    let mut changed = false;
    let hovered = [0, 1].map(|index| {
        let (option, label) = options[index];
        let response = ui
            .interact(
                segment(index as f32),
                track_response.id.with(index),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() && *value != option {
            *value = option;
            changed = true;
        }
        let selected = *value == option;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
        });
        response.hovered()
    });
    let position = ui.ctx().animate_bool_with_time_and_easing(
        track_response.id,
        *value == options[1].0,
        0.1,
        egui::emath::easing::cubic_in_out,
    );
    let visuals = ui.visuals();
    let painter = ui.painter();
    painter.rect_filled(
        track,
        size.y / 2.0,
        visuals.text_color().gamma_multiply(0.06),
    );
    painter.rect_filled(
        segment(position),
        SEGMENT_HEIGHT / 2.0,
        visuals.text_color().gamma_multiply(0.16),
    );
    for (index, galley) in galleys.into_iter().enumerate() {
        let base = if hovered[index] {
            visuals.text_color()
        } else {
            visuals.weak_text_color()
        };
        let highlight = 1.0 - (position - index as f32).abs();
        painter.galley(
            segment(index as f32).center() - galley.size() / 2.0,
            galley,
            base.lerp_to_gamma(visuals.strong_text_color(), highlight),
        );
    }
    changed
}

fn settings_nav_item(ui: &mut egui::Ui, page: SettingsPage, selected: bool) -> bool {
    let icon = egui::Image::new(page.icon())
        .fit_to_exact_size(egui::Vec2::splat(16.0))
        .tint(ui.visuals().text_color());
    ui.add(
        egui::Button::selectable(selected, (icon, page.title(), egui::Atom::grow()))
            .min_size(egui::vec2(ui.available_width(), 30.0))
            .gap(8.0),
    )
    .clicked()
}

const SWAP_BUTTON_SIZE: f32 = 24.0;
const NEW_ARTWORK_FORM_WIDTH: f32 = 260.0;
const SIZE_PREVIEW_WIDTH: f32 = 96.0;

/// Fits a canvas of `size` pixels inside `bounds`, preserving its aspect ratio.
fn fit_size_preview(size: [u32; 2], bounds: egui::Vec2) -> egui::Vec2 {
    let size = egui::vec2(size[0].max(1) as f32, size[1].max(1) as f32);
    let scale = (bounds.x / size.x).min(bounds.y / size.y);
    (size * scale).max(egui::Vec2::splat(2.0))
}

fn paint_size_preview(ui: &egui::Ui, rect: egui::Rect, size: [u32; 2], color: egui::Color32) {
    let preview = egui::Rect::from_center_size(rect.center(), fit_size_preview(size, rect.size()));
    let painter = ui.painter();
    painter.rect_filled(preview, 4, color);
    painter.rect_stroke(
        preview,
        4,
        egui::Stroke::new(1.0_f32, ui.visuals().text_color().gamma_multiply(0.3)),
        egui::StrokeKind::Inside,
    );
}

fn dimension_row(ui: &mut egui::Ui, label: &str, value: &mut u32, max: u32) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak("px");
            let response = ui.add(egui::DragValue::new(value).range(1..=max).speed(1));
            // Leaves room for the swap button that spans both size rows.
            ui.add_space(SWAP_BUTTON_SIZE);
            response
        })
        .inner
    })
    .inner
}

fn background_row(ui: &mut egui::Ui, color: &mut egui::Color32) {
    ui.horizontal(|ui| {
        ui.label("Background");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(56.0, 28.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, "Background color")
            });
            let painter = ui.painter();
            painter.rect_filled(rect, 14, *color);
            // Keeps white and near-surface colors visible against the dialog.
            painter.rect_stroke(
                rect,
                14,
                egui::Stroke::new(1.0_f32, ui.visuals().text_color().gamma_multiply(0.3)),
                egui::StrokeKind::Inside,
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            egui::Popup::from_toggle_button_response(&response)
                .width(220.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    color_picker::show(ui, color);
                });
        });
    });
}

fn accent_color_dropdown(ui: &mut egui::Ui, accent_color: &mut egui::Color32) -> bool {
    let previous = *accent_color;
    let selected_name = ACCENT_COLORS
        .iter()
        .find(|(_, color)| *color == previous)
        .map_or("Custom", |(name, _)| name);
    const DOT_RADIUS: f32 = 6.0;
    let gap = ui.spacing().button_padding.x;
    // Leaves room before the label for the dot painted over the button below.
    let mut selected_text = egui::text::LayoutJob::default();
    selected_text.append(
        selected_name,
        2.0 * DOT_RADIUS + gap,
        egui::TextFormat::simple(
            egui::TextStyle::Button.resolve(ui.style()),
            egui::Color32::PLACEHOLDER,
        ),
    );
    let combo = egui::ComboBox::from_id_salt("accent color")
        .selected_text(selected_text)
        .width(140.0)
        .height(f32::INFINITY)
        .icon(|ui, rect, visuals, open| {
            let turn = ui
                .ctx()
                .animate_bool_responsive(egui::Id::new("accent color chevron"), open);
            egui::Image::new(egui::include_image!(
                "../../../assets/icons/chevron-down.svg"
            ))
            .tint(visuals.fg_stroke.color)
            .rotate(turn * std::f32::consts::PI, egui::Vec2::splat(0.5))
            .paint_at(ui, rect);
        })
        .show_ui(ui, |ui| {
            // Popups don't inherit the dialog's neutral selection style.
            let visuals = ui.visuals_mut();
            visuals.selection.bg_fill = visuals.text_color().gamma_multiply(0.08);
            visuals.selection.stroke = egui::Stroke::new(1.0_f32, visuals.strong_text_color());
            for (name, color) in ACCENT_COLORS {
                let dot = ui.id().with(name);
                let response = egui::Button::selectable(
                    *accent_color == color,
                    (
                        egui::Atom::custom(dot, egui::Vec2::splat(2.0 * DOT_RADIUS)),
                        name,
                        egui::Atom::grow(),
                    ),
                )
                .min_size(egui::vec2(ui.available_width(), 0.0))
                .atom_ui(ui);
                if let Some(rect) = response.rect(dot) {
                    ui.painter().circle_filled(
                        rect.center(),
                        DOT_RADIUS,
                        resolve_accent_color(color, ui.visuals().dark_mode),
                    );
                }
                if response.response.clicked() {
                    *accent_color = color;
                }
            }
        });
    let button = combo.response.rect;
    ui.painter().circle_filled(
        egui::pos2(button.left() + gap + DOT_RADIUS, button.center().y),
        DOT_RADIUS,
        resolve_accent_color(*accent_color, ui.visuals().dark_mode),
    );
    *accent_color != previous
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
            background: egui::Color32::WHITE,
            focus_width: true,
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
        egui::Modal::new(egui::Id::new("new artwork dialog"))
            .frame(dialog_frame(context))
            .show(context, |ui| {
                ui.set_width(NEW_ARTWORK_FORM_WIDTH + 16.0 + SIZE_PREVIEW_WIDTH);
                ui.heading("New Artwork");
                ui.add_space(16.0);
                ui.horizontal_top(|ui| {
                    let form = ui.vertical(|ui| {
                        ui.set_width(NEW_ARTWORK_FORM_WIDTH);
                        let max_dimension = self.canvas_size_constraints.max_dimension;
                        let width = dimension_row(ui, "Width", &mut dialog.width, max_dimension);
                        if std::mem::take(&mut dialog.focus_width) {
                            width.request_focus();
                        }
                        ui.add_space(10.0);
                        let height = dimension_row(ui, "Height", &mut dialog.height, max_dimension);
                        let swap_rect = egui::Rect::from_center_size(
                            egui::pos2(
                                width.rect.left()
                                    - ui.spacing().item_spacing.x
                                    - SWAP_BUTTON_SIZE / 2.0,
                                (width.rect.center().y + height.rect.center().y) / 2.0,
                            ),
                            egui::Vec2::splat(SWAP_BUTTON_SIZE),
                        );
                        // Interacts without allocating, so the button doesn't push later rows.
                        let swap = ui
                            .interact(swap_rect, ui.id().with("swap size"), egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Swap width and height");
                        swap.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Swap width and height",
                            )
                        });
                        let tint = if swap.hovered() {
                            ui.visuals().strong_text_color()
                        } else {
                            ui.visuals().text_color()
                        };
                        egui::Image::new(egui::include_image!(
                            "../../../assets/icons/arrow-down-up.svg"
                        ))
                        .tint(tint)
                        .paint_at(
                            ui,
                            egui::Rect::from_center_size(
                                swap_rect.center(),
                                egui::Vec2::splat(14.0),
                            ),
                        );
                        if swap.clicked() {
                            std::mem::swap(&mut dialog.width, &mut dialog.height);
                        }
                        ui.add_space(10.0);
                        background_row(ui, &mut dialog.background);
                    });
                    ui.add_space(16.0);
                    let (preview_rect, _) = ui.allocate_exact_size(
                        egui::vec2(SIZE_PREVIEW_WIDTH, form.response.rect.height()),
                        egui::Sense::hover(),
                    );
                    paint_size_preview(
                        ui,
                        preview_rect,
                        [dialog.width, dialog.height],
                        dialog.background,
                    );
                });
                let validation = self
                    .canvas_size_constraints
                    .validate([dialog.width, dialog.height]);
                if let Err(error) = &validation {
                    ui.add_space(8.0);
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                ui.add_space(20.0);
                let clicked = dialog_buttons(
                    ui,
                    &[
                        DialogButton::new("Create", DialogButtonKind::Primary)
                            .enabled(validation.is_ok()),
                        DialogButton::new("Cancel", DialogButtonKind::Secondary),
                    ],
                );
                close = clicked == Some(1);
                if clicked == Some(0)
                    || (validation.is_ok() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    create = Some((dialog.width, dialog.height, rgb(dialog.background)));
                }
            });
        // Unlike other dialogs, a backdrop click doesn't dismiss this one.
        close |= context.input(|input| input.key_pressed(egui::Key::Escape));
        if let Some((width, height, background)) = create {
            self.commands
                .push(AppCommand::Navigation(NavigationCommand::CreateArtwork {
                    width,
                    height,
                    background,
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
        let response = egui::Modal::new(egui::Id::new("message dialog"))
            .frame(dialog_frame(context))
            .show(context, |ui| {
                ui.set_width(DIALOG_WIDTH);
                ui.heading(dialog.title);
                ui.add_space(16.0);
                ui.label(&dialog.message);
                ui.add_space(20.0);
                dialog_buttons(ui, &[DialogButton::new("OK", DialogButtonKind::Primary)]).is_some()
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
        let frame = dialog_frame(context).inner_margin(0);
        let response = egui::Modal::new(egui::Id::new("settings dialog"))
            .frame(frame)
            .show(context, |ui| {
                let visuals = ui.visuals_mut();
                visuals.selection.bg_fill = visuals.text_color().gamma_multiply(0.08);
                visuals.selection.stroke = egui::Stroke::new(1.0_f32, visuals.strong_text_color());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let section_background = ui.painter().add(egui::Shape::Noop);
                    let sidebar = egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: 24,
                            right: 12,
                            top: 24,
                            bottom: 24,
                        })
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(SETTINGS_SIDEBAR_WIDTH);
                                ui.set_min_height(SETTINGS_MIN_HEIGHT);
                                ui.heading("Settings");
                                ui.add_space(12.0);
                                for candidate in SettingsPage::ALL {
                                    if settings_nav_item(ui, candidate, page == candidate) {
                                        page = candidate;
                                    }
                                }
                            });
                        })
                        .response
                        .rect;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(24))
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
                            });
                        });
                    // Spans the dialog's full height, which the content column may set.
                    let dialog = ui.min_rect();
                    let opaque = surface_style == SurfaceStyle::Opaque;
                    let dark_mode = ui.visuals().dark_mode;
                    // The opaque dark surface is already near black, so lift the content instead.
                    let shape = if opaque && dark_mode {
                        egui::epaint::RectShape::filled(
                            dialog.with_min_x(sidebar.right()),
                            egui::CornerRadius {
                                ne: 28,
                                se: 28,
                                ..Default::default()
                            },
                            ui.visuals().text_color().gamma_multiply(0.09),
                        )
                    } else {
                        egui::epaint::RectShape::filled(
                            sidebar.with_max_y(dialog.bottom()),
                            egui::CornerRadius {
                                nw: 28,
                                sw: 28,
                                ..Default::default()
                            },
                            if opaque || !dark_mode {
                                ui.visuals().text_color().gamma_multiply(0.09)
                            } else {
                                egui::Color32::from_black_alpha(90)
                            },
                        )
                    };
                    ui.painter().set(section_background, shape);
                });
            });
        let close = response.should_close();
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
        let frame = dialog_frame(context).inner_margin(12);
        let response = egui::Modal::new(egui::Id::new("keyboard shortcuts"))
            .frame(frame)
            .show(context, |ui| {
                const COLUMN_GAP: f32 = 32.0;
                let (action_width, shortcut_width) = shortcut_column_widths(ui, SHORTCUT_SECTIONS);
                let shortcut_x = action_width + COLUMN_GAP;
                ui.set_width(
                    shortcut_x + shortcut_width + COLUMN_GAP + ui.spacing().scroll.bar_width + 24.0,
                );
                ui.add_space(12.0);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(12, 0))
                    .show(ui, |ui| ui.heading("Keyboard Shortcuts"));
                ui.add_space(16.0);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(520.0)
                            .show(ui, |ui| {
                                for (index, &(title, shortcuts)) in
                                    SHORTCUT_SECTIONS.iter().enumerate()
                                {
                                    if index > 0 {
                                        ui.add_space(28.0);
                                    }
                                    shortcut_section(ui, title, shortcuts, shortcut_x);
                                }
                            });
                    });
                ui.add_space(12.0);
            });
        if response.should_close() {
            self.shortcuts_dialog_open = false;
        }
    }
}

const SHORTCUT_SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Tools",
        &[
            ("Brush / Eraser / Smudge", "D/B, E, S"),
            ("Transform", "T"),
            ("Freehand selection", "L"),
            ("Cycle paint tools", CYCLE_TOOLS_SHORTCUT),
            ("Show or hide sidebar", "Tab"),
            ("Resize brush", RESIZE_BRUSH_SHORTCUT),
            ("Eyedropper", EYEDROPPER_SHORTCUT),
            ("Quick menu", "Hold Q"),
        ],
    ),
    (
        "Canvas",
        &[
            ("Pan", "Space drag"),
            ("Zoom", "Wheel"),
            ("Rotate freely", "R drag"),
            ("Reset rotation", RESET_ROTATION_SHORTCUT),
            ("Rotate left / right", ROTATE_SHORTCUT),
            ("Flip horizontal / vertical", FLIP_SHORTCUT),
            ("Crop or resize", CROP_SHORTCUT),
        ],
    ),
    (
        "Document",
        &[
            ("Settings", SETTINGS_SHORTCUT),
            ("Save", SAVE_SHORTCUT),
            ("Export PNG", EXPORT_SHORTCUT),
            ("Return to gallery", GALLERY_SHORTCUT),
            ("Undo", UNDO_SHORTCUT),
            ("Redo", REDO_SHORTCUT),
            ("Apply transform or crop", "Enter"),
            ("Cancel transform or crop, or deselect", "Escape"),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_preview_keeps_aspect_ratio_within_bounds() {
        let bounds = egui::vec2(96.0, 120.0);
        for (size, expected) in [
            ([4000, 2000], egui::vec2(96.0, 48.0)),
            ([1000, 4000], egui::vec2(30.0, 120.0)),
            ([20000, 1], egui::vec2(96.0, 2.0)),
            ([0, 0], egui::vec2(96.0, 96.0)),
        ] {
            let fitted = fit_size_preview(size, bounds);
            assert!(
                (fitted - expected).length() < 1e-3,
                "{size:?} -> {fitted:?}"
            );
        }
    }

    #[test]
    fn accent_colors_include_the_default() {
        let original = egui::Color32::from_rgb(
            DEFAULT_ACCENT_COLOR[0],
            DEFAULT_ACCENT_COLOR[1],
            DEFAULT_ACCENT_COLOR[2],
        );
        assert!(ACCENT_COLORS.iter().any(|(_, color)| *color == original));
    }
}
