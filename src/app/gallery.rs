use crate::artwork::{ArtworkId, ArtworkStore, ArtworkSummary, DocumentManifest};

pub(super) struct OpenedArtwork {
    pub(super) id: ArtworkId,
    pub(super) title: String,
    pub(super) document: DocumentManifest,
    pub(super) layers: Vec<image::RgbaImage>,
}

pub(super) struct GalleryController {
    store: Option<ArtworkStore>,
    artworks: Vec<ArtworkSummary>,
    warnings: Vec<String>,
}

impl GalleryController {
    pub(super) fn discover() -> Self {
        match ArtworkStore::discover() {
            Ok(store) => {
                let catalog = store.catalog();
                Self {
                    store: Some(store),
                    artworks: catalog.artworks,
                    warnings: catalog.warnings,
                }
            }
            Err(error) => Self {
                store: None,
                artworks: Vec::new(),
                warnings: vec![error.to_string()],
            },
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

    pub(super) fn open(&self, id: &ArtworkId) -> Result<OpenedArtwork, String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let loaded = store.load(id).map_err(|error| error.to_string())?;
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
        Ok(OpenedArtwork {
            id: loaded.summary.id,
            title: loaded.summary.title,
            document: loaded.document,
            layers,
        })
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
