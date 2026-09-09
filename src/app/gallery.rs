use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use chromazen_canvas::CanvasSizeConstraints;

use crate::artwork::{
    ArtworkId, ArtworkStore, ArtworkSummary, DocumentManifest, ReferenceManifest,
};

pub(super) struct OpenedArtwork {
    pub(super) id: ArtworkId,
    pub(super) title: String,
    pub(super) document: DocumentManifest,
    pub(super) layers: Vec<image::RgbaImage>,
    pub(super) reference_sources: Vec<(ReferenceManifest, PathBuf)>,
}

pub(super) struct ThumbnailCompletion {
    pub(super) id: ArtworkId,
    pub(super) path: PathBuf,
    pub(super) result: Result<image::RgbaImage, String>,
}

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

const LOAD_DIALOG_DELAY: Duration = Duration::from_millis(200);

pub(super) struct GalleryController {
    store: Option<ArtworkStore>,
    artworks: Vec<ArtworkSummary>,
    warnings: Vec<String>,
    duplicate_receiver: Option<mpsc::Receiver<Result<ArtworkSummary, String>>>,
    load_receiver: Option<mpsc::Receiver<Result<OpenedArtwork, String>>>,
    load_started_at: Option<Instant>,
    thumbnail_sender: mpsc::Sender<ThumbnailCompletion>,
    thumbnail_receiver: mpsc::Receiver<ThumbnailCompletion>,
    thumbnail_paths: Vec<(ArtworkId, PathBuf)>,
    wake: WakeCallback,
}

impl GalleryController {
    pub(super) fn discover(wake: WakeCallback) -> Self {
        let (store, artworks, warnings) = match ArtworkStore::discover() {
            Ok(store) => {
                let catalog = store.scan_catalog();
                (Some(store), catalog.artworks, catalog.warnings)
            }
            Err(error) => (None, Vec::new(), vec![error.to_string()]),
        };
        let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
        let mut controller = Self {
            store,
            artworks,
            warnings,
            duplicate_receiver: None,
            load_receiver: None,
            load_started_at: None,
            thumbnail_sender,
            thumbnail_receiver,
            thumbnail_paths: Vec::new(),
            wake,
        };
        controller.start_thumbnail_loads();
        controller
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
        let catalog = store.scan_catalog();
        self.artworks = catalog.artworks;
        self.warnings = catalog.warnings;
        self.start_thumbnail_loads();
    }

    pub(super) fn start_load_artwork(
        &mut self,
        id: ArtworkId,
        constraints: CanvasSizeConstraints,
    ) -> Result<(), String> {
        if self.load_receiver.is_some() {
            return Ok(());
        }
        let store = self
            .store
            .clone()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let (sender, receiver) = mpsc::channel();
        self.load_receiver = Some(receiver);
        self.load_started_at = Some(Instant::now());
        let wake = self.wake.clone();
        wake();
        std::thread::spawn(move || {
            let result = load_artwork(&store, &id, constraints);
            let _ = sender.send(result);
            wake();
        });
        Ok(())
    }

    pub(super) fn take_load_completion(&mut self) -> Option<Result<OpenedArtwork, String>> {
        let result = self.load_receiver.as_ref()?.try_recv().ok()?;
        self.load_receiver = None;
        self.load_started_at = None;
        Some(result)
    }

    pub(super) fn is_loading(&self) -> bool {
        self.load_receiver.is_some()
    }

    pub(super) fn load_dialog_delay(&self) -> Option<Duration> {
        self.load_started_at
            .map(|started_at| LOAD_DIALOG_DELAY.saturating_sub(started_at.elapsed()))
    }

    pub(super) fn take_thumbnail_completion(&self) -> Option<ThumbnailCompletion> {
        while let Ok(completion) = self.thumbnail_receiver.try_recv() {
            if self
                .thumbnail_paths
                .iter()
                .any(|(id, path)| id == &completion.id && path == &completion.path)
            {
                return Some(completion);
            }
        }
        None
    }

    fn start_thumbnail_loads(&mut self) {
        self.thumbnail_paths.retain(|current| {
            self.artworks
                .iter()
                .any(|artwork| artwork.id == current.0 && artwork.thumbnail_path == current.1)
        });
        let pending: Vec<_> =
            self.artworks
                .iter()
                .filter(|artwork| {
                    !self.thumbnail_paths.iter().any(|current| {
                        artwork.id == current.0 && artwork.thumbnail_path == current.1
                    })
                })
                .map(|artwork| (artwork.id.clone(), artwork.thumbnail_path.clone()))
                .collect();
        if pending.is_empty() {
            return;
        }
        self.thumbnail_paths.extend(pending.iter().cloned());
        let sender = self.thumbnail_sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            for (id, path) in pending {
                let result = image::open(&path)
                    .map(image::DynamicImage::into_rgba8)
                    .map_err(|error| {
                        format!(
                            "failed to load artwork thumbnail {}: {error}",
                            path.display()
                        )
                    });
                let _ = sender.send(ThumbnailCompletion { id, path, result });
                wake();
            }
        });
    }

    pub(super) fn start_duplicate(&mut self, id: ArtworkId) -> Result<(), String> {
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

fn load_artwork(
    store: &ArtworkStore,
    id: &ArtworkId,
    constraints: CanvasSizeConstraints,
) -> Result<OpenedArtwork, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_thumbnail_completion_is_ignored() {
        let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
        let id = ArtworkId::new();
        let current_path = PathBuf::from("current.png");
        assert!(
            thumbnail_sender
                .send(ThumbnailCompletion {
                    id: id.clone(),
                    path: PathBuf::from("stale.png"),
                    result: Err("stale".to_owned()),
                })
                .is_ok()
        );
        assert!(
            thumbnail_sender
                .send(ThumbnailCompletion {
                    id: id.clone(),
                    path: current_path.clone(),
                    result: Ok(image::RgbaImage::new(1, 1)),
                })
                .is_ok()
        );
        let controller = GalleryController {
            store: None,
            artworks: Vec::new(),
            warnings: Vec::new(),
            duplicate_receiver: None,
            load_receiver: None,
            load_started_at: None,
            thumbnail_sender,
            thumbnail_receiver,
            thumbnail_paths: vec![(id, current_path.clone())],
            wake: Arc::new(|| {}),
        };

        assert_eq!(
            controller
                .take_thumbnail_completion()
                .map(|completion| completion.path),
            Some(current_path)
        );
    }
}
