use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use image::imageops::FilterType;

use crate::{
    artwork::{
        ArtworkId, ArtworkStore, LayerSource, LayerWrite, RevisionWrite, encode_png,
        flatten_premultiplied_layers,
    },
    renderer::{DocumentVersions, LayerId, PaintRenderer},
};

const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
const THUMBNAIL_SIZE: u32 = 512;

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SaveStatus {
    Clean,
    Waiting,
    Saving,
    Failed(String),
}

struct SaveCompletion {
    artwork_id: ArtworkId,
    versions: DocumentVersions,
    result: Result<(), String>,
}

struct ArtworkSession {
    id: ArtworkId,
    title: String,
    saved_versions: DocumentVersions,
    in_flight: Option<DocumentVersions>,
    dirty_since: Option<Instant>,
    save_requested: bool,
    error: Option<String>,
}

pub(super) struct AutosaveController {
    store: Option<ArtworkStore>,
    session: Option<ArtworkSession>,
    completion_sender: mpsc::Sender<SaveCompletion>,
    completion_receiver: mpsc::Receiver<SaveCompletion>,
    wake: WakeCallback,
}

impl AutosaveController {
    pub(super) fn new(store: Option<ArtworkStore>, wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            store,
            session: None,
            completion_sender,
            completion_receiver,
            wake,
        }
    }

    pub(super) fn begin_new(&mut self, id: ArtworkId, title: String) {
        self.session = Some(ArtworkSession {
            id,
            title,
            saved_versions: DocumentVersions {
                generation: 0,
                metadata: 0,
                layers: Vec::new(),
            },
            in_flight: None,
            dirty_since: Some(Instant::now()),
            save_requested: false,
            error: None,
        });
    }

    pub(super) fn begin_loaded(
        &mut self,
        id: ArtworkId,
        title: String,
        versions: DocumentVersions,
    ) {
        self.session = Some(ArtworkSession {
            id,
            title,
            saved_versions: versions,
            in_flight: None,
            dirty_since: None,
            save_requested: false,
            error: None,
        });
    }

    pub(super) fn clear(&mut self) {
        self.session = None;
    }

    pub(super) fn artwork_title(&self) -> Option<&str> {
        self.session.as_ref().map(|session| session.title.as_str())
    }

    pub(super) fn status(&self, paint: &PaintRenderer) -> SaveStatus {
        let Some(session) = &self.session else {
            return SaveStatus::Clean;
        };
        if let Some(error) = &session.error {
            return SaveStatus::Failed(error.clone());
        }
        if session.in_flight.is_some() {
            return SaveStatus::Saving;
        }
        if paint.document_versions().generation != session.saved_versions.generation {
            SaveStatus::Waiting
        } else {
            SaveStatus::Clean
        }
    }

    pub(super) fn is_clean(&self, paint: &PaintRenderer) -> bool {
        matches!(self.status(paint), SaveStatus::Clean)
    }

    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.session
            .as_ref()
            .filter(|session| session.in_flight.is_none() && session.error.is_none())
            .and_then(|session| session.dirty_since)
            .and_then(|dirty_since| dirty_since.checked_add(AUTOSAVE_DELAY))
    }

    pub(super) fn request_save(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.save_requested = true;
            session.error = None;
        }
    }

    pub(super) fn update(&mut self, paint: &PaintRenderer) -> bool {
        let mut changed = self.process_completions(paint);
        let Some(session) = self.session.as_mut() else {
            return changed;
        };
        let current = paint.document_versions();
        let target_generation = session
            .in_flight
            .as_ref()
            .map_or(session.saved_versions.generation, |versions| {
                versions.generation
            });
        if current.generation != target_generation && session.dirty_since.is_none() {
            session.dirty_since = Some(Instant::now());
            changed = true;
        }
        if session.in_flight.is_some() {
            return changed;
        }
        let due = session.save_requested
            || session
                .dirty_since
                .is_some_and(|since| since.elapsed() >= AUTOSAVE_DELAY);
        if !due {
            return changed;
        }
        session.save_requested = false;
        session.error = None;
        match self.start_save(paint, current) {
            Ok(()) => true,
            Err(error) => {
                if let Some(session) = self.session.as_mut() {
                    session.error = Some(error);
                    session.dirty_since = None;
                }
                true
            }
        }
    }

    fn start_save(
        &mut self,
        paint: &PaintRenderer,
        versions: DocumentVersions,
    ) -> Result<(), String> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let session = self.session.as_mut().expect("save requires a session");
        let document = paint.document_manifest();
        let readback = paint.begin_document_layer_readback()?;
        let dirty_ids = changed_layer_ids(&session.saved_versions, &versions);
        let first_revision = session.saved_versions.layers.is_empty();
        let artwork_id = session.id.clone();
        let title = session.title.clone();
        session.in_flight = Some(versions.clone());
        session.dirty_since = None;

        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let images = readback.finish()?;
                let write = build_revision_write(document, images, &dirty_ids, first_revision)?;
                store
                    .commit_revision(&artwork_id, &title, write)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })();
            let _ = sender.send(SaveCompletion {
                artwork_id,
                versions,
                result,
            });
            wake();
        });
        Ok(())
    }

    fn process_completions(&mut self, paint: &PaintRenderer) -> bool {
        let mut changed = false;
        while let Ok(completion) = self.completion_receiver.try_recv() {
            let Some(session) = self.session.as_mut() else {
                continue;
            };
            if completion.artwork_id != session.id {
                continue;
            }
            session.in_flight = None;
            match completion.result {
                Ok(()) => {
                    session.saved_versions = completion.versions;
                    session.error = None;
                    if paint.document_versions().generation != session.saved_versions.generation {
                        session.dirty_since = Some(Instant::now());
                    }
                }
                Err(error) => {
                    session.error = Some(error);
                    session.dirty_since = None;
                }
            }
            changed = true;
        }
        changed
    }
}

fn changed_layer_ids(saved: &DocumentVersions, current: &DocumentVersions) -> HashSet<LayerId> {
    let saved: HashMap<_, _> = saved.layers.iter().copied().collect();
    current
        .layers
        .iter()
        .filter_map(|(id, version)| (saved.get(id) != Some(version)).then_some(*id))
        .collect()
}

fn build_revision_write(
    document: crate::artwork::DocumentManifest,
    images: Vec<(LayerId, image::RgbaImage)>,
    dirty_ids: &HashSet<LayerId>,
    first_revision: bool,
) -> Result<RevisionWrite, String> {
    let thumbnail_png = encode_thumbnail(&images, document.background)?;
    let mut layers = Vec::with_capacity(images.len());
    for (id, image) in images {
        let source = if first_revision || dirty_ids.contains(&id) {
            LayerSource::Png(encode_png(&image)?)
        } else {
            LayerSource::ReuseCurrent
        };
        layers.push(LayerWrite { id: id.0, source });
    }
    Ok(RevisionWrite {
        document,
        layers,
        thumbnail_png,
    })
}

fn encode_thumbnail(
    layers: &[(LayerId, image::RgbaImage)],
    background: [u8; 3],
) -> Result<Vec<u8>, String> {
    let Some((_, first_layer)) = layers.first() else {
        return Err("cannot create a thumbnail without layers".to_owned());
    };
    let source_size = first_layer.dimensions();
    if source_size.0 == 0 || source_size.1 == 0 {
        return Err("cannot create a thumbnail for an empty canvas".to_owned());
    }
    if layers
        .iter()
        .any(|(_, layer)| layer.dimensions() != source_size)
    {
        return Err("thumbnail layers must have matching dimensions".to_owned());
    }

    let (thumbnail_width, thumbnail_height) = fit_dimensions(source_size, THUMBNAIL_SIZE);
    let resized: Vec<_> = layers
        .iter()
        .map(|(_, layer)| {
            image::imageops::resize(
                layer,
                thumbnail_width,
                thumbnail_height,
                FilterType::Triangle,
            )
        })
        .collect();
    let composite = flatten_premultiplied_layers(&resized, background)?;

    let mut thumbnail = image::RgbaImage::new(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    let x = (THUMBNAIL_SIZE - thumbnail_width) / 2;
    let y = (THUMBNAIL_SIZE - thumbnail_height) / 2;
    image::imageops::replace(&mut thumbnail, &composite, i64::from(x), i64::from(y));
    encode_png(&thumbnail)
}

fn fit_dimensions(source: (u32, u32), target: u32) -> (u32, u32) {
    if source.0 >= source.1 {
        (
            target,
            (u64::from(source.1) * u64::from(target) / u64::from(source.0)).max(1) as u32,
        )
    } else {
        (
            (u64::from(source.0) * u64::from(target) / u64::from(source.1)).max(1) as u32,
            target,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(generation: u64, layers: &[(u64, u64)]) -> DocumentVersions {
        DocumentVersions {
            generation,
            metadata: generation,
            layers: layers
                .iter()
                .map(|(id, version)| (LayerId(*id), *version))
                .collect(),
        }
    }

    #[test]
    fn only_changed_and_new_layers_are_encoded() {
        let saved = versions(4, &[(1, 2), (2, 4)]);
        let current = versions(6, &[(1, 2), (2, 5), (3, 6)]);
        let changed = changed_layer_ids(&saved, &current);
        assert_eq!(changed, HashSet::from([LayerId(2), LayerId(3)]));
    }

    #[test]
    fn thumbnail_composites_premultiplied_layers() {
        let layer = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
        let png = encode_thumbnail(&[(LayerId(1), layer)], [0, 0, 255]).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        let pixel = decoded.get_pixel(0, 0);
        assert!((127..=128).contains(&pixel[0]));
        assert_eq!(pixel[1], 0);
        assert!((126..=127).contains(&pixel[2]));
        assert_eq!(pixel[3], 255);
    }

    #[test]
    fn rectangular_thumbnail_is_centered_without_stretching() {
        let layer = image::RgbaImage::new(4, 2);
        let png = encode_thumbnail(&[(LayerId(1), layer)], [10, 20, 30]).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([0, 0, 0, 0]));
        assert_eq!(
            decoded.get_pixel(0, THUMBNAIL_SIZE / 2),
            &image::Rgba([10, 20, 30, 255])
        );
    }
}
