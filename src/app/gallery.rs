use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

use crate::{
    artwork::{ArtworkId, ArtworkStore, ArtworkSummary, DocumentManifest, ReferenceManifest},
    renderer::CanvasSizeConstraints,
};

pub(super) struct OpenedArtwork {
    pub(super) id: ArtworkId,
    pub(super) title: String,
    pub(super) document: DocumentManifest,
    pub(super) layers: Vec<image::RgbaImage>,
    pub(super) reference_sources: Vec<(ReferenceManifest, PathBuf)>,
}

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

pub(super) struct GalleryController {
    store: Option<ArtworkStore>,
    artworks: Vec<ArtworkSummary>,
    warnings: Vec<String>,
    duplicate_receiver: Option<mpsc::Receiver<Result<ArtworkSummary, String>>>,
    wake: WakeCallback,
}

impl GalleryController {
    pub(super) fn discover(wake: WakeCallback) -> Self {
        let (store, artworks, warnings) = match ArtworkStore::discover() {
            Ok(store) => {
                let catalog = store.catalog();
                (Some(store), catalog.artworks, catalog.warnings)
            }
            Err(error) => (None, Vec::new(), vec![error.to_string()]),
        };
        Self {
            store,
            artworks,
            warnings,
            duplicate_receiver: None,
            wake,
        }
    }

    pub(super) fn store(&self) -> Option<ArtworkStore> {
        self.store.clone()
    }

    pub(super) fn artworks(&self) -> &[ArtworkSummary] {
        &self.artworks
    }

    pub(super) fn warning(&self) -> Option<String> {
        (!self.warnings.is_empty()).then(|| self.warnings.join("\n"))
    }

    pub(super) fn refresh(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        let catalog = store.catalog();
        self.artworks = catalog.artworks;
        self.warnings = catalog.warnings;
    }

    pub(super) fn open(
        &self,
        id: &ArtworkId,
        constraints: CanvasSizeConstraints,
    ) -> Result<OpenedArtwork, String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let loaded = store.load(id).map_err(|error| error.to_string())?;
        loaded.document.validate()?;
        constraints.validate([loaded.document.width, loaded.document.height])?;
        let mut layers = Vec::with_capacity(loaded.layer_paths.len());
        for (metadata, path) in loaded.document.layers.iter().zip(&loaded.layer_paths) {
            let image = image::open(path)
                .map_err(|error| {
                    format!(
                        "failed to decode layer {} from {}: {error}",
                        metadata.id,
                        path.display()
                    )
                })?
                .to_rgba8();
            if image.dimensions() != (loaded.document.width, loaded.document.height) {
                return Err(format!(
                    "layer {} has dimensions {}x{}; expected {}x{}",
                    metadata.id,
                    image.width(),
                    image.height(),
                    loaded.document.width,
                    loaded.document.height
                ));
            }
            layers.push(image);
        }
        let reference_sources = loaded
            .document
            .references
            .iter()
            .cloned()
            .zip(loaded.reference_paths)
            .collect();
        Ok(OpenedArtwork {
            id: loaded.summary.id,
            title: loaded.summary.title,
            document: loaded.document,
            layers,
            reference_sources,
        })
    }

    pub(super) fn duplicate(&mut self, id: ArtworkId) -> Result<(), String> {
        if self.duplicate_receiver.is_some() {
            return Ok(());
        }
        let store = self
            .store
            .clone()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let (sender, receiver) = mpsc::channel();
        self.duplicate_receiver = Some(receiver);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = store.duplicate(&id).map_err(|error| error.to_string());
            let _ = sender.send(result);
            wake();
        });
        Ok(())
    }

    pub(super) fn take_duplicate_completion(&mut self) -> Option<Result<ArtworkSummary, String>> {
        let result = self.duplicate_receiver.as_ref()?.try_recv().ok()?;
        self.duplicate_receiver = None;
        if result.is_ok() {
            self.refresh();
        }
        Some(result)
    }

    pub(super) fn rename(&mut self, id: &ArtworkId, title: &str) -> Result<(), String> {
        self.store
            .as_ref()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?
            .rename(id, title)
            .map_err(|error| error.to_string())?;
        self.refresh();
        Ok(())
    }

    pub(super) fn delete(&mut self, id: &ArtworkId) -> Result<(), String> {
        self.store
            .as_ref()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?
            .delete(id)
            .map_err(|error| error.to_string())?;
        self.refresh();
        Ok(())
    }
}
