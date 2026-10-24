use super::*;

impl App {
    pub(super) fn has_pending_navigation(&self) -> bool {
        self.pending_artwork.is_some()
            || self.pending_exit
            || self.gallery.is_loading()
            || self.reference_load.is_loading()
    }

    pub(super) fn request_exit(&mut self) {
        if self.screen == AppScreen::Editor {
            if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
                self.input.finish_document_interaction(paint, gui.brush);
            }
            let clean = self
                .paint
                .as_ref()
                .is_some_and(|paint| self.autosave.is_clean(paint, &self.references));
            if !clean {
                self.autosave.request_save();
            }
        }
        if let Some(gui) = self.gui.as_ref() {
            let UiSettingsSnapshot {
                brush,
                tool_brushes,
                tool_sizes,
                tool_opacities,
                panel_layout,
                accent_color,
                surface_style,
                workspace_background,
            } = gui.settings_for_save();
            if let Some(effect) = self.settings.handle_command(SettingsCommand::Save {
                brush,
                tool_brushes,
                tool_sizes,
                tool_opacities,
                panel_layout,
                accent_color,
                surface_style,
                workspace_background,
            }) {
                match effect {
                    SettingsEffect::Success(_) => {}
                    SettingsEffect::Error(error) => {
                        log::error!("failed to save settings on exit: {error}")
                    }
                }
            }
        }
        if let Some(gui) = self.gui.as_mut() {
            gui.close_new_artwork_dialog();
        }
        self.pending_exit = true;
        self.pending_artwork = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(super) fn create_artwork(&mut self, size: [u32; 2], background: [u8; 3]) {
        if self.screen == AppScreen::Editor {
            self.pending_artwork = Some(PendingArtwork::Create(size, background));
            self.autosave.request_save();
            return;
        }
        self.finish_create_artwork(size, background);
    }

    fn finish_create_artwork(&mut self, size: [u32; 2], background: [u8; 3]) {
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.reset_document(size) {
            if let Some(gui) = self.gui.as_mut() {
                gui.open_error_dialog("Chromazen couldn’t create the artwork.", error);
            }
            return;
        }
        paint.set_background_color(background);
        let id = crate::artwork::ArtworkId::new();
        self.references.clear();
        self.pending_reference_load = None;
        let brush_color = self
            .gui
            .as_ref()
            .map(|gui| gui.brush.color.to_array())
            .unwrap_or([170, 187, 204, 255]);
        self.autosave
            .begin_new_session(id, "Untitled".to_owned(), brush_color);
        self.screen = AppScreen::Editor;
        self.pending_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title("Untitled • Chromazen");
        }
    }

    pub(super) fn open_artwork(&mut self, id: &crate::artwork::ArtworkId) {
        if self.screen != AppScreen::Gallery || self.has_pending_navigation() {
            return;
        }
        let Some(constraints) = self.paint.as_ref().map(Canvas::canvas_size_constraints) else {
            return;
        };
        if let Err(error) = self.gallery.start_load_artwork(id.clone(), constraints)
            && let Some(gui) = self.gui.as_mut()
        {
            gui.open_error_dialog("Chromazen couldn’t open the artwork.", error);
        }
    }

    pub(super) fn apply_pending_artwork(&mut self) -> bool {
        if self.pending_artwork.is_none()
            || self.reference_load.is_loading()
            || !self
                .paint
                .as_ref()
                .is_some_and(|paint| self.autosave.is_clean(paint, &self.references))
        {
            return false;
        }
        let pending = self.pending_artwork.take();
        self.enter_gallery();
        if let Some(PendingArtwork::Create(size, background)) = pending {
            self.finish_create_artwork(size, background);
        }
        true
    }

    pub(super) fn enter_gallery(&mut self) {
        self.gallery.refresh();
        self.autosave.clear_session();
        self.references.clear();
        self.pending_reference_load = None;
        self.screen = AppScreen::Gallery;
        self.pending_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(WINDOW_TITLE);
        }
        self.sync_history_menu();
    }

    pub(super) fn finish_open_artwork(&mut self, opened: gallery::OpenedArtwork) {
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        let canvas_document = crate::artwork::canvas_document(&opened.document);
        if let Err(error) = paint.load_document(&canvas_document, opened.layers) {
            if let Some(gui) = self.gui.as_mut() {
                gui.open_error_dialog("Chromazen couldn’t open the artwork.", error);
            }
            return;
        }
        let versions = paint.document_versions();
        if let Some(gui) = self.gui.as_mut() {
            gui.set_brush_color(opened.document.brush_color);
        }
        self.references.clear();
        self.autosave.clear_session();
        self.screen = AppScreen::Editor;
        self.pending_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(&format!("{} • Chromazen", opened.title));
        }
        if opened.reference_sources.is_empty() {
            self.autosave.begin_loaded_session(
                opened.id,
                opened.title,
                versions,
                self.references.versions(),
                opened.document.brush_color,
            );
        } else {
            self.pending_reference_load = Some(PendingReferenceLoad {
                id: opened.id.clone(),
                title: opened.title,
                paint_versions: versions,
                brush_color: opened.document.brush_color,
            });
            self.reference_load
                .start(opened.id, opened.reference_sources);
        }
    }
}
