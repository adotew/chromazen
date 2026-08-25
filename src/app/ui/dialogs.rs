use super::*;

impl GuiLayer {
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
        let response = egui::Modal::new(egui::Id::new("new artwork dialog")).show(context, |ui| {
            ui.heading("New Artwork");
            ui.add_space(8.0);
            egui::Grid::new("new artwork dimensions")
                .num_columns(3)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut dialog.width)
                            .range(1..=self.canvas_size_constraints.max_dimension)
                            .speed(1),
                    );
                    ui.label("px");
                    ui.end_row();

                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut dialog.height)
                            .range(1..=self.canvas_size_constraints.max_dimension)
                            .speed(1),
                    );
                    ui.label("px");
                    ui.end_row();
                });
            let validation = self
                .canvas_size_constraints
                .validate([dialog.width, dialog.height]);
            if let Err(error) = &validation {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                let submit = ui.add_enabled(validation.is_ok(), egui::Button::new("Create"));
                if submit.clicked()
                    || (validation.is_ok() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    create = Some((dialog.width, dialog.height));
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
                                    ("Show or hide panels", "Tab"),
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
                                    ("Save", SAVE_SHORTCUT),
                                    ("Export PNG", EXPORT_SHORTCUT),
                                    ("Return to gallery", GALLERY_SHORTCUT),
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
