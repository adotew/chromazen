use super::*;

#[cfg(not(target_os = "macos"))]
use egui::containers::menu::SubMenuButton;

#[cfg(not(target_os = "macos"))]
impl GuiLayer {
    pub(super) fn show_application_menu(
        &mut self,
        context: &egui::Context,
        state: ApplicationMenuState,
    ) {
        egui::Area::new(egui::Id::new("application menu"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                let response = ui.allocate_response(egui::Vec2::splat(36.0), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Application menu")
                });
                egui::Image::new(egui::include_image!("../../../assets/icons/menu.svg"))
                    .fit_to_exact_size(egui::Vec2::splat(18.0))
                    .tint(egui::Color32::from_white_alpha(170))
                    .alt_text("Application menu")
                    .paint_at(
                        ui,
                        egui::Rect::from_center_size(
                            response.rect.center(),
                            egui::Vec2::splat(18.0),
                        ),
                    );
                let _ = egui::Popup::menu(&response).show(|ui| {
                    apply_menu_item_padding(ui);
                    ui.set_min_width(210.0);
                    SubMenuButton::new("File").ui(ui, |ui| {
                        ui.set_min_width(260.0);
                        menu_item(
                            ui,
                            "New Artwork",
                            Some("Ctrl-N"),
                            true,
                            AppCommand::Navigation(NavigationCommand::NewArtwork),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Save",
                            Some(SAVE_SHORTCUT),
                            state.document_enabled,
                            AppCommand::Editor(EditorCommand::SaveArtwork),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Export PNG…",
                            Some(EXPORT_SHORTCUT),
                            state.export_enabled,
                            AppCommand::Editor(EditorCommand::ExportPng),
                            &mut self.commands,
                        );
                        ui.separator();
                        menu_item(
                            ui,
                            "Add Reference…",
                            None,
                            state.document_enabled,
                            AppCommand::Editor(EditorCommand::AddReferences),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Import Photoshop Brushes…",
                            None,
                            state.document_enabled,
                            AppCommand::Settings(SettingsCommand::ImportBrushes),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Return to Gallery",
                            Some(GALLERY_SHORTCUT),
                            state.document_enabled,
                            AppCommand::Navigation(NavigationCommand::ShowGallery),
                            &mut self.commands,
                        );
                        ui.separator();
                        menu_item(
                            ui,
                            "Quit Chromazen",
                            None,
                            true,
                            AppCommand::Navigation(NavigationCommand::Quit),
                            &mut self.commands,
                        );
                    });
                    SubMenuButton::new("Edit").ui(ui, |ui| {
                        ui.set_min_width(220.0);
                        menu_item(
                            ui,
                            "Undo",
                            Some(UNDO_SHORTCUT),
                            state.can_undo,
                            AppCommand::Editor(EditorCommand::Undo),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Redo",
                            Some(REDO_SHORTCUT),
                            state.can_redo,
                            AppCommand::Editor(EditorCommand::Redo),
                            &mut self.commands,
                        );
                    });
                    SubMenuButton::new("Canvas").ui(ui, |ui| {
                        ui.set_min_width(260.0);
                        menu_item(
                            ui,
                            "Rotate Left 90°",
                            Some("Ctrl-Alt-←"),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::RotateCanvasLeft),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Rotate Right 90°",
                            Some("Ctrl-Alt-→"),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::RotateCanvasRight),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Reset Rotation",
                            Some("Shift-R"),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::ResetCanvasRotation),
                            &mut self.commands,
                        );
                        ui.separator();
                        menu_item(
                            ui,
                            "Flip Horizontally",
                            Some("F"),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::ToggleCanvasFlipHorizontal),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Flip Vertically",
                            Some("Shift-F"),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::ToggleCanvasFlipVertical),
                            &mut self.commands,
                        );
                        ui.separator();
                        menu_item(
                            ui,
                            "Crop / Resize Canvas",
                            Some(CROP_SHORTCUT),
                            state.canvas_enabled,
                            AppCommand::Editor(EditorCommand::RequestCanvasResize),
                            &mut self.commands,
                        );
                    });
                    SubMenuButton::new("Settings").ui(ui, |ui| {
                        ui.set_min_width(240.0);
                        menu_item(
                            ui,
                            "Save Settings",
                            None,
                            true,
                            AppCommand::Settings(SettingsCommand::Save),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Reload Configuration",
                            None,
                            true,
                            AppCommand::Settings(SettingsCommand::ReloadConfiguration),
                            &mut self.commands,
                        );
                        menu_item(
                            ui,
                            "Reset Brush to Defaults",
                            None,
                            true,
                            AppCommand::Settings(SettingsCommand::ResetBrush),
                            &mut self.commands,
                        );
                        ui.separator();
                        menu_item(
                            ui,
                            "Open Configuration Folder",
                            None,
                            true,
                            AppCommand::Settings(SettingsCommand::OpenConfigDirectory),
                            &mut self.commands,
                        );
                    });
                    SubMenuButton::new("Help").ui(ui, |ui| {
                        ui.set_min_width(220.0);
                        menu_item(
                            ui,
                            "Keyboard Shortcuts",
                            Some("Shift-?"),
                            true,
                            AppCommand::Ui(UiCommand::ShowShortcuts),
                            &mut self.commands,
                        );
                    });
                });
                response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Application menu");
            });
    }
}

#[cfg(not(target_os = "macos"))]
fn menu_item(
    ui: &mut egui::Ui,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
    command: AppCommand,
    commands: &mut Vec<AppCommand>,
) {
    apply_menu_item_padding(ui);
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        let shortcut =
            egui::RichText::new(shortcut).color(ui.visuals().text_color().gamma_multiply(0.6));
        button = button.right_text(shortcut);
    }
    if ui.add_enabled(enabled, button).clicked() {
        commands.push(command);
        ui.close();
    }
}

#[cfg(target_os = "macos")]
impl GuiLayer {
    pub(super) fn show_application_menu(
        &mut self,
        _context: &egui::Context,
        _state: ApplicationMenuState,
    ) {
    }
}
