use super::*;

impl App {
    pub(super) fn dispatch_pending_commands(&mut self) -> bool {
        if self.gui.is_none() || self.pending_commands.is_empty() {
            return false;
        }

        for command in std::mem::take(&mut self.pending_commands) {
            match command {
                AppCommand::Editor(command) => self.handle_editor_command(command),
                AppCommand::Gallery(command) => self.handle_gallery_command(command),
                AppCommand::Navigation(command) => self.handle_navigation_command(command),
                AppCommand::Settings(command) => self.handle_app_settings_command(command),
                AppCommand::Ui(command) => self.handle_ui_command(command),
            }
        }
        self.sync_history_menu();
        true
    }

    fn handle_editor_command(&mut self, command: EditorCommand) {
        let canvas_crop_active = self.gui.as_ref().is_some_and(GuiLayer::canvas_crop_active);
        if canvas_crop_active
            && matches!(
                command,
                EditorCommand::Undo
                    | EditorCommand::Redo
                    | EditorCommand::RotateCanvasLeft
                    | EditorCommand::RotateCanvasRight
                    | EditorCommand::ResetCanvasRotation
                    | EditorCommand::ToggleCanvasFlipHorizontal
                    | EditorCommand::ToggleCanvasFlipVertical
                    | EditorCommand::RequestCanvasResize
            )
        {
            return;
        }
        if matches!(
            command,
            EditorCommand::SelectTool(_)
                | EditorCommand::SelectLayer(_)
                | EditorCommand::AddLayer
                | EditorCommand::DuplicateSelectedLayer
                | EditorCommand::ClearLayer
                | EditorCommand::DeleteSelectedLayer
                | EditorCommand::RenameLayer { .. }
                | EditorCommand::MergeLayerDown(_)
                | EditorCommand::SetLayerClipped { .. }
                | EditorCommand::SetLayerVisibility { .. }
                | EditorCommand::SetLayerOpacity { .. }
                | EditorCommand::CommitLayerOpacity { .. }
                | EditorCommand::MoveLayer { .. }
                | EditorCommand::SetBackgroundColor(_)
                | EditorCommand::CommitBackgroundColor { .. }
                | EditorCommand::ResizeCanvas { .. }
                | EditorCommand::SaveArtwork
                | EditorCommand::ExportPng
        ) {
            self.finish_editor_interaction();
        }

        match command {
            EditorCommand::Undo => self.undo(),
            EditorCommand::Redo => self.redo(),
            EditorCommand::RotateCanvasLeft => {
                self.finish_editor_interaction();
                if let Some(paint) = self.paint.as_mut() {
                    paint.rotate_canvas_view(-std::f32::consts::FRAC_PI_2);
                }
            }
            EditorCommand::RotateCanvasRight => {
                self.finish_editor_interaction();
                if let Some(paint) = self.paint.as_mut() {
                    paint.rotate_canvas_view(std::f32::consts::FRAC_PI_2);
                }
            }
            EditorCommand::ResetCanvasRotation => {
                self.finish_editor_interaction();
                if let Some(paint) = self.paint.as_mut() {
                    paint.reset_canvas_rotation();
                }
            }
            EditorCommand::ToggleCanvasFlipHorizontal => {
                self.finish_editor_interaction();
                if let Some(paint) = self.paint.as_mut() {
                    paint.toggle_canvas_flip_horizontal();
                }
            }
            EditorCommand::ToggleCanvasFlipVertical => {
                self.finish_editor_interaction();
                if let Some(paint) = self.paint.as_mut() {
                    paint.toggle_canvas_flip_vertical();
                }
            }
            EditorCommand::RequestCanvasResize => {
                if self.screen != AppScreen::Editor {
                    return;
                }
                self.finish_editor_interaction();
                let size = self.paint.as_mut().map(|paint| {
                    paint.prepare_canvas_crop_view();
                    paint.document_size()
                });
                if let (Some(size), Some(gui)) = (size, self.gui.as_mut()) {
                    gui.open_canvas_crop(size);
                }
            }
            EditorCommand::ResizeCanvas {
                width,
                height,
                origin,
            } => {
                let result = self
                    .paint
                    .as_mut()
                    .ok_or_else(|| "the paint renderer is unavailable".to_owned())
                    .and_then(|paint| paint.resize_canvas([width, height], origin));
                if let Err(error) = result
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_error_dialog("Chromazen couldn’t resize the canvas.", error);
                }
            }
            EditorCommand::SetLayerTransform(transform) => {
                if self.input.tool() == EditorTool::Transform
                    && let Some(paint) = self.paint.as_mut()
                {
                    paint.update_layer_transform(transform);
                }
            }
            EditorCommand::ApplyLayerTransform => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.commit_layer_transform();
                }
                self.restore_previous_paint_tool();
            }
            EditorCommand::CancelLayerTransform => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.cancel_layer_transform();
                }
                self.restore_previous_paint_tool();
            }
            EditorCommand::SelectTool(tool) => self.select_editor_tool(tool),
            EditorCommand::SelectLayer(id) => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.select_layer(id);
                }
            }
            EditorCommand::AddLayer => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.add_layer();
                }
            }
            EditorCommand::DuplicateSelectedLayer => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.duplicate_selected_layer();
                }
            }
            EditorCommand::ClearLayer => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.clear_selected_layer();
                }
            }
            EditorCommand::DeleteSelectedLayer => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.delete_selected_layer();
                }
            }
            EditorCommand::RenameLayer { id, name } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.rename_layer(id, &name);
                }
            }
            EditorCommand::MergeLayerDown(id) => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.merge_layer_down(id);
                }
            }
            EditorCommand::SetLayerClipped { id, clipped } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.set_layer_clipped(id, clipped);
                }
            }
            EditorCommand::SetLayerVisibility { id, visible } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.set_layer_visibility(id, visible);
                }
            }
            EditorCommand::SetLayerOpacity { id, opacity } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.set_layer_opacity(id, opacity);
                }
            }
            EditorCommand::CommitLayerOpacity { id, before, after } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.commit_layer_opacity(id, before, after);
                }
            }
            EditorCommand::MoveLayer {
                dragged,
                target,
                edge,
            } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.move_layer_relative(dragged, target, edge);
                }
            }
            EditorCommand::AddReferences => {
                if self.screen != AppScreen::Editor || self.reference_load.is_loading() {
                    return;
                }
                let paths = choose_reference_paths();
                if let Some(artwork_id) = self.autosave.artwork_id().cloned() {
                    self.reference_import.start(artwork_id, paths, None);
                }
            }
            EditorCommand::SetReferenceTransform { id, position, size } => {
                self.references.set_transform(id, position, size);
            }
            EditorCommand::ToggleReferenceLocked(id) => {
                self.references.toggle_locked(id);
            }
            EditorCommand::DeleteReference(id) => {
                self.references.remove(id);
            }
            EditorCommand::SetBrushColor(color) => {
                self.autosave.set_brush_color(color);
            }
            EditorCommand::SetBackgroundColor(color) => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.set_background_color(color);
                }
            }
            EditorCommand::CommitBackgroundColor { before, after } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.commit_background_color(before, after);
                }
            }
            EditorCommand::SaveArtwork => {
                if self.screen == AppScreen::Editor {
                    self.autosave.request_save();
                }
            }
            EditorCommand::ExportPng => self.export_png(),
        }
    }

    fn handle_app_settings_command(&mut self, command: AppSettingsCommand) {
        match command {
            AppSettingsCommand::SwitchBrush { tool, id } => {
                self.handle_settings_commands(vec![SettingsCommand::SwitchBrush {
                    tool,
                    id,
                    reset_size: true,
                }]);
            }
            AppSettingsCommand::ImportBrushes => {
                if self.screen != AppScreen::Editor {
                    return;
                }
                let paths = choose_abr_paths();
                let tool = self.input.tool().paint_tool().unwrap_or(PaintTool::Brush);
                self.brush_import.start(tool, paths);
            }
            AppSettingsCommand::Save => {
                let Some((brush, tool_brushes, tool_sizes, tool_opacities, panel_layout)) =
                    self.gui.as_ref().map(GuiLayer::settings_for_save)
                else {
                    return;
                };
                self.handle_settings_commands(vec![SettingsCommand::Save {
                    brush,
                    tool_brushes,
                    tool_sizes,
                    tool_opacities,
                    panel_layout,
                }]);
            }
            AppSettingsCommand::ReloadConfiguration => {
                let tool = self.input.tool().paint_tool().unwrap_or(PaintTool::Brush);
                self.handle_settings_commands(vec![SettingsCommand::ReloadFromDisk(tool)]);
            }
            AppSettingsCommand::ResetBrush => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.reset_active_brush_settings();
                }
            }
            AppSettingsCommand::OpenConfigDirectory => {
                self.handle_settings_commands(vec![SettingsCommand::OpenConfigDirectory]);
            }
        }
    }

    fn handle_navigation_command(&mut self, command: NavigationCommand) {
        if matches!(
            command,
            NavigationCommand::CreateArtwork { .. }
                | NavigationCommand::OpenArtwork(_)
                | NavigationCommand::ShowGallery
        ) {
            self.finish_editor_interaction();
        }
        match command {
            NavigationCommand::NewArtwork => {
                if !self.has_pending_navigation()
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_new_artwork_dialog();
                }
            }
            NavigationCommand::CreateArtwork { width, height } => {
                if !self.has_pending_navigation() {
                    self.create_artwork([width, height]);
                }
            }
            NavigationCommand::OpenArtwork(id) => self.open_artwork(&id),
            NavigationCommand::ShowGallery => {
                if self.screen == AppScreen::Editor {
                    if let Some(gui) = self.gui.as_mut() {
                        gui.close_new_artwork_dialog();
                        gui.close_canvas_crop();
                    }
                    self.pending_gallery = true;
                    self.pending_new_artwork = None;
                    self.autosave.request_save();
                }
            }
            NavigationCommand::CancelPending => {
                self.pending_gallery = false;
                self.pending_new_artwork = None;
                self.pending_exit = false;
            }
            NavigationCommand::Quit => self.request_exit(),
        }
    }

    fn handle_gallery_command(&mut self, command: GalleryCommand) {
        match command {
            GalleryCommand::Rename { id, title } => {
                if let Err(error) = self.gallery.rename(&id, &title)
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_error_dialog("Chromazen couldn’t rename the artwork.", error);
                }
            }
            GalleryCommand::Duplicate(id) => {
                if let Err(error) = self.gallery.start_duplicate(id)
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_error_dialog("Chromazen couldn’t duplicate the artwork.", error);
                }
            }
            GalleryCommand::Delete(id) => {
                if let Err(error) = self.gallery.delete(&id)
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_error_dialog("Chromazen couldn’t delete the artwork.", error);
                }
            }
        }
    }

    fn handle_ui_command(&mut self, command: UiCommand) {
        match command {
            UiCommand::ShowShortcuts => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.open_shortcuts_dialog();
                }
            }
        }
    }

    fn export_png(&mut self) {
        if self.screen != AppScreen::Editor || self.export.is_exporting() {
            return;
        }
        let title = self.autosave.artwork_title().unwrap_or("Untitled");
        let Some(path) = choose_export_path(title) else {
            return;
        };
        let result = self
            .paint
            .as_ref()
            .ok_or_else(|| "the paint renderer is unavailable".to_owned())
            .and_then(|paint| self.export.start(path, paint));
        if let Err(error) = result
            && let Some(gui) = self.gui.as_mut()
        {
            gui.open_error_dialog("Chromazen couldn’t export the PNG.", error);
        }
    }

    fn restore_previous_paint_tool(&mut self) {
        self.select_editor_tool(self.input.previous_paint_tool());
    }

    fn select_editor_tool(&mut self, tool: EditorTool) {
        self.input.select_tool(tool);
        if self.input.tool() != tool {
            return;
        }
        let Some(paint_tool) = tool.paint_tool() else {
            return;
        };
        let id = self
            .gui
            .as_ref()
            .map(|gui| gui.brush_for_tool(paint_tool).to_owned());
        if let Some(id) = id {
            self.handle_settings_commands(vec![SettingsCommand::SwitchBrush {
                tool: paint_tool,
                id,
                reset_size: false,
            }]);
        }
    }

    pub(super) fn finish_editor_interaction(&mut self) {
        if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
            self.input.finish_document_interaction(paint, gui.brush);
        }
    }

    fn undo(&mut self) {
        if let Some(paint) = self.paint.as_mut()
            && !paint.cancel_layer_transform()
        {
            paint.undo();
        }
    }

    fn redo(&mut self) {
        if let Some(paint) = self.paint.as_mut()
            && !paint.cancel_layer_transform()
        {
            paint.redo();
        }
    }

    pub(super) fn sync_history_menu(&self) {
        let in_editor = self.screen == AppScreen::Editor;
        let canvas_crop_active =
            in_editor && self.gui.as_ref().is_some_and(GuiLayer::canvas_crop_active);
        let (can_undo, can_redo) = (in_editor && !canvas_crop_active)
            .then_some(self.paint.as_ref())
            .flatten()
            .map_or((false, false), |paint| (paint.can_undo(), paint.can_redo()));
        self.native_menu.set_history_enabled(can_undo, can_redo);
        self.native_menu.set_document_enabled(in_editor);
        self.native_menu
            .set_canvas_enabled(in_editor && !canvas_crop_active);
        self.native_menu
            .set_export_enabled(in_editor && !self.export.is_exporting());
    }

    pub(super) fn handle_settings_commands(&mut self, commands: Vec<SettingsCommand>) {
        for command in commands {
            let Some(effect) = self.settings.handle_command(command) else {
                continue;
            };
            let Some(gui) = self.gui.as_mut() else {
                continue;
            };
            match effect {
                SettingsEffect::Success(message) => gui.open_success_dialog(message),
                SettingsEffect::Error(error) => {
                    gui.open_error_dialog("Chromazen couldn’t update the settings.", error)
                }
            }
        }
    }
}
