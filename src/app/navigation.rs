use super::*;

impl App {
    pub(super) fn navigation_pending(&self) -> bool {
        self.pending_gallery || self.pending_exit
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
        if let Some(gui) = self.gui.as_mut() {
            gui.close_new_artwork_dialog();
        }
        self.pending_exit = true;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(super) fn create_artwork(&mut self, size: [u32; 2]) {
        if self.screen == AppScreen::Editor {
            self.pending_gallery = true;
            self.pending_new_artwork = Some(size);
            self.autosave.request_save();
            return;
        }
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.reset_document(size) {
            if let Some(gui) = self.gui.as_mut() {
                gui.show_error("Chromazen couldn’t create the artwork.", error);
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
            .begin_new(id, "Untitled".to_owned(), brush_color);
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title("Untitled • Chromazen");
        }
    }

    pub(super) fn open_artwork(&mut self, id: &crate::artwork::ArtworkId) {
        let Some(constraints) = self
            .paint
            .as_ref()
            .map(PaintRenderer::canvas_size_constraints)
        else {
            return;
        };
        let opened = match self.gallery.open(id, constraints) {
            Ok(opened) => opened,
            Err(error) => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.show_error("Chromazen couldn’t open the artwork.", error);
                }
                return;
            }
        };
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.load_document(&opened.document, opened.layers) {
            if let Some(gui) = self.gui.as_mut() {
                gui.show_error("Chromazen couldn’t open the artwork.", error);
            }
            return;
        }
        let versions = paint.document_versions();
        if let Some(gui) = self.gui.as_mut() {
            gui.set_brush_color(opened.document.brush_color);
        }
        self.references.clear();
        self.autosave.clear();
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(&format!("{} • Chromazen", opened.title));
        }
        if opened.reference_sources.is_empty() {
            self.autosave.begin_loaded(
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

    pub(super) fn finish_gallery_navigation(&mut self) {
        self.gallery.refresh();
        self.autosave.clear();
        self.references.clear();
        self.pending_reference_load = None;
        self.screen = AppScreen::Gallery;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(WINDOW_TITLE);
        }
        self.sync_history_menu();
    }
}
