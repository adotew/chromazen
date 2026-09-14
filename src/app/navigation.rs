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
            let (brush, tool_brushes, tool_sizes, tool_opacities, panel_layout) =
                gui.settings_for_save();
            if let Some(effect) = self.settings.handle_command(SettingsCommand::Save {
                brush,
                tool_brushes,
                tool_sizes,
                tool_opacities,
                panel_layout,
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

    pub(super) fn create_artwork(&mut self, size: [u32; 2]) {
        if self.screen == AppScreen::Editor {
            self.pending_artwork = Some(PendingArtwork::Create(size));
            self.autosave.request_save();
            return;
        }
        self.finish_create_artwork(size);
    }

    fn finish_create_artwork(&mut self, size: [u32; 2]) {
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.reset_document(size) {
            if let Some(gui) = self.gui.as_mut() {
                gui.open_error_dialog("Chromazen couldn’t create the artwork.", error);
            }
            return;
        }
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
        if self.autosave.artwork_id() == Some(id) || self.has_pending_navigation() {
            return;
        }
        if self.screen == AppScreen::Editor {
            self.pending_artwork = Some(PendingArtwork::Open(id.clone()));
            self.autosave.request_save();
        } else {
            self.start_open_artwork(id.clone());
        }
    }

    fn start_open_artwork(&mut self, id: crate::artwork::ArtworkId) {
        let Some(constraints) = self.paint.as_ref().map(Canvas::canvas_size_constraints) else {
            return;
        };
        if let Err(error) = self.gallery.start_load_artwork(id, constraints)
            && let Some(gui) = self.gui.as_mut()
        {
            gui.open_error_dialog("Chromazen couldn’t open the artwork.", error);
        }
    }

    pub(super) fn apply_pending_artwork(&mut self) -> bool {
        let Some(pending) = self.pending_artwork.as_ref() else {
            return false;
        };
        if self.reference_load.is_loading() {
            return false;
        }
        let ready = match pending {
            PendingArtwork::Delete(_) => self.paint.as_ref().is_some_and(|paint| {
                !matches!(
                    self.autosave.status(paint, &self.references),
                    autosave::SaveStatus::Saving
                )
            }),
            _ => self
                .paint
                .as_ref()
                .is_some_and(|paint| self.autosave.is_clean(paint, &self.references)),
        };
        if !ready {
            return false;
        }
        let Some(pending) = self.pending_artwork.take() else {
            return false;
        };
        match pending {
            PendingArtwork::Open(id) => self.start_open_artwork(id),
            PendingArtwork::Create(size) => self.finish_create_artwork(size),
            PendingArtwork::Duplicate(id) => {
                if let Err(error) = self.gallery.start_duplicate(id)
                    && let Some(gui) = self.gui.as_mut()
                {
                    gui.open_error_dialog("Chromazen couldn’t duplicate the artwork.", error);
                }
            }
            PendingArtwork::Delete(id) => self.delete_active_artwork(id),
        }
        true
    }

    pub(super) fn delete_active_artwork(&mut self, id: crate::artwork::ArtworkId) {
        let artworks = self.gallery.artworks();
        let next = next_artwork_after(artworks, &id);
        if artworks.iter().any(|artwork| artwork.id == id)
            && let Err(error) = self.gallery.delete(&id)
        {
            if let Some(gui) = self.gui.as_mut() {
                gui.open_error_dialog("Chromazen couldn’t delete the artwork.", error);
            }
            return;
        }

        self.autosave.clear_session();
        self.references.clear();
        self.pending_reference_load = None;
        self.screen = AppScreen::Empty;
        self.pending_artwork = None;
        if let Some(window) = self.window.as_ref() {
            window.set_title(WINDOW_TITLE);
        }
        self.sync_history_menu();
        if let Some(id) = next {
            self.start_open_artwork(id);
        }
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

fn next_artwork_after(
    artworks: &[crate::artwork::ArtworkSummary],
    id: &crate::artwork::ArtworkId,
) -> Option<crate::artwork::ArtworkId> {
    artworks
        .iter()
        .position(|artwork| artwork.id == *id)
        .and_then(|index| {
            artworks
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|index| artworks.get(index)))
        })
        .or_else(|| artworks.iter().find(|artwork| artwork.id != *id))
        .map(|artwork| artwork.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: crate::artwork::ArtworkId) -> crate::artwork::ArtworkSummary {
        crate::artwork::ArtworkSummary {
            id,
            title: "Artwork".to_owned(),
            modified_unix_ms: 0,
            dimensions: [1, 1],
            thumbnail_path: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn deleting_active_artwork_selects_a_neighbor() {
        let first = crate::artwork::ArtworkId::new();
        let middle = crate::artwork::ArtworkId::new();
        let last = crate::artwork::ArtworkId::new();
        let artworks = vec![
            summary(first.clone()),
            summary(middle.clone()),
            summary(last.clone()),
        ];

        assert_eq!(next_artwork_after(&artworks, &middle), Some(last));
        assert_eq!(next_artwork_after(&artworks, &first), Some(middle));
        assert_eq!(
            next_artwork_after(&artworks, &artworks[2].id),
            Some(artworks[1].id.clone())
        );
        assert_eq!(next_artwork_after(&[summary(first.clone())], &first), None);
    }
}
