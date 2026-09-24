use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use chromazen_canvas::{Canvas, CanvasDocument, DocumentVersions, LayerId};
use image::imageops::FilterType;

use crate::artwork::{
    ArtworkId, ArtworkStore, CompositeLayer, LayerSource, LayerWrite, ReferenceSource,
    ReferenceWrite, RevisionWrite, ThumbnailSource, encode_png, flatten_premultiplied_layers,
};

use super::references::{ReferenceBoard, ReferenceId, ReferenceVersions};

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SaveVersions {
    paint: DocumentVersions,
    references: ReferenceVersions,
    brush_color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisualSignature {
    size: [u32; 2],
    background: [u8; 3],
    // The order and compositing settings affect the gallery thumbnail; the
    // selected layer, names, references and brush color do not.
    layers: Vec<(LayerId, bool, u8, bool)>,
}

impl From<&CanvasDocument> for VisualSignature {
    fn from(document: &CanvasDocument) -> Self {
        Self {
            size: document.size,
            background: document.background,
            layers: document
                .layers
                .iter()
                .map(|layer| (layer.id, layer.visible, layer.opacity, layer.clipped))
                .collect(),
        }
    }
}

struct SaveCompletion {
    artwork_id: ArtworkId,
    versions: SaveVersions,
    visual: VisualSignature,
    result: Result<(), String>,
}

struct ArtworkSession {
    id: ArtworkId,
    title: String,
    saved_versions: SaveVersions,
    saved_visual: Option<VisualSignature>,
    in_flight: Option<SaveVersions>,
    dirty_since: Option<Instant>,
    save_requested: bool,
    error: Option<String>,
}

pub(super) struct AutosaveController {
    store: Option<ArtworkStore>,
    session: Option<ArtworkSession>,
    brush_color: [u8; 4],
    completion_sender: mpsc::Sender<SaveCompletion>,
    completion_receiver: mpsc::Receiver<SaveCompletion>,
    catalog_dirty: bool,
    wake: WakeCallback,
}

impl AutosaveController {
    pub(super) fn new(store: Option<ArtworkStore>, wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            store,
            session: None,
            brush_color: [170, 187, 204, 255],
            completion_sender,
            completion_receiver,
            catalog_dirty: false,
            wake,
        }
    }

    pub(super) fn begin_new_session(&mut self, id: ArtworkId, title: String, brush_color: [u8; 4]) {
        self.brush_color = brush_color;
        self.session = Some(ArtworkSession {
            id,
            title,
            saved_versions: SaveVersions {
                paint: DocumentVersions {
                    generation: 0,
                    metadata: 0,
                    layers: Vec::new(),
                },
                references: ReferenceVersions {
                    generation: 0,
                    assets: Vec::new(),
                },
                brush_color,
            },
            saved_visual: None,
            in_flight: None,
            dirty_since: Some(Instant::now()),
            save_requested: false,
            error: None,
        });
    }

    pub(super) fn begin_loaded_session(
        &mut self,
        id: ArtworkId,
        title: String,
        versions: DocumentVersions,
        reference_versions: ReferenceVersions,
        brush_color: [u8; 4],
        document: &CanvasDocument,
    ) {
        self.brush_color = brush_color;
        self.session = Some(ArtworkSession {
            id,
            title,
            saved_versions: SaveVersions {
                paint: versions,
                references: reference_versions,
                brush_color,
            },
            saved_visual: Some(VisualSignature::from(document)),
            in_flight: None,
            dirty_since: None,
            save_requested: false,
            error: None,
        });
    }

    pub(super) fn clear_session(&mut self) {
        self.session = None;
    }

    pub(super) fn set_brush_color(&mut self, color: [u8; 4]) {
        self.brush_color = color;
    }

    pub(super) fn artwork_id(&self) -> Option<&ArtworkId> {
        self.session.as_ref().map(|session| &session.id)
    }

    pub(super) fn artwork_title(&self) -> Option<&str> {
        self.session.as_ref().map(|session| session.title.as_str())
    }

    pub(super) fn rename_artwork(&mut self, title: String) {
        if let Some(session) = self.session.as_mut() {
            session.title = title;
            session.save_requested = true;
        }
    }

    pub(super) fn take_catalog_dirty(&mut self) -> bool {
        std::mem::take(&mut self.catalog_dirty)
    }

    pub(super) fn status(&self, paint: &Canvas, references: &ReferenceBoard) -> SaveStatus {
        let Some(session) = &self.session else {
            return SaveStatus::Clean;
        };
        if let Some(error) = &session.error {
            return SaveStatus::Failed(error.clone());
        }
        if session.in_flight.is_some() {
            return SaveStatus::Saving;
        }
        if session.save_requested
            || capture_save_versions(paint, references, self.brush_color) != session.saved_versions
        {
            SaveStatus::Waiting
        } else {
            SaveStatus::Clean
        }
    }

    pub(super) fn is_clean(&self, paint: &Canvas, references: &ReferenceBoard) -> bool {
        matches!(self.status(paint, references), SaveStatus::Clean)
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

    pub(super) fn update(&mut self, paint: &Canvas, references: &ReferenceBoard) -> bool {
        let mut changed = self.apply_save_completions(paint, references);
        let Some(session) = self.session.as_mut() else {
            return changed;
        };
        let current = capture_save_versions(paint, references, self.brush_color);
        let target = session
            .in_flight
            .as_ref()
            .unwrap_or(&session.saved_versions);
        if current != *target && session.dirty_since.is_none() {
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
        match self.start_save_task(paint, references, current) {
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

    fn start_save_task(
        &mut self,
        paint: &Canvas,
        references: &ReferenceBoard,
        versions: SaveVersions,
    ) -> Result<(), String> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| "The artwork data directory is unavailable".to_owned())?;
        let session = self.session.as_mut().expect("save requires a session");
        let mut document = crate::artwork::document_manifest(paint.document_snapshot());
        document.brush_color = self.brush_color;
        document.references = references.manifest();
        let reference_images: Vec<_> = references
            .images()
            .iter()
            .map(|reference| (reference.id, Arc::clone(&reference.png)))
            .collect();
        let dirty_layer_ids = changed_layer_ids(&session.saved_versions.paint, &versions.paint);
        let dirty_reference_ids =
            changed_reference_ids(&session.saved_versions.references, &versions.references);
        let first_revision = session.saved_versions.paint.layers.is_empty();
        let visual = VisualSignature::from(&paint.document_snapshot());
        let reuse_thumbnail = !first_revision
            && dirty_layer_ids.is_empty()
            && session.saved_visual.as_ref() == Some(&visual);
        // Title, selection, brush color, references and layer renames need no
        // GPU transfer. A visual change still needs the exact same snapshot
        // pixels as the layer writes and thumbnail.
        let readback = (!reuse_thumbnail)
            .then(|| paint.begin_document_layer_readback())
            .transpose()?;
        let artwork_id = session.id.clone();
        let title = session.title.clone();
        session.in_flight = Some(versions.clone());
        session.dirty_since = None;

        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let images = readback.map(|readback| readback.finish()).transpose()?;
                let write = build_revision_write(
                    document,
                    images,
                    &dirty_layer_ids,
                    reference_images,
                    &dirty_reference_ids,
                    first_revision,
                )?;
                store
                    .commit_revision(&artwork_id, &title, write)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })();
            let _ = sender.send(SaveCompletion {
                artwork_id,
                versions,
                visual,
                result,
            });
            wake();
        });
        Ok(())
    }

    fn apply_save_completions(&mut self, paint: &Canvas, references: &ReferenceBoard) -> bool {
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
                    session.saved_visual = Some(completion.visual);
                    session.error = None;
                    self.catalog_dirty = true;
                    if capture_save_versions(paint, references, self.brush_color)
                        != session.saved_versions
                    {
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

fn capture_save_versions(
    paint: &Canvas,
    references: &ReferenceBoard,
    brush_color: [u8; 4],
) -> SaveVersions {
    SaveVersions {
        paint: paint.document_versions(),
        references: references.versions(),
        brush_color,
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

fn changed_reference_ids(
    saved: &ReferenceVersions,
    current: &ReferenceVersions,
) -> HashSet<ReferenceId> {
    let saved: HashMap<_, _> = saved.assets.iter().copied().collect();
    current
        .assets
        .iter()
        .filter_map(|(id, version)| (saved.get(id) != Some(version)).then_some(*id))
        .collect()
}

fn build_revision_write(
    document: crate::artwork::DocumentManifest,
    images: Option<Vec<(LayerId, image::RgbaImage)>>,
    dirty_layer_ids: &HashSet<LayerId>,
    reference_images: Vec<(ReferenceId, Arc<Vec<u8>>)>,
    dirty_reference_ids: &HashSet<ReferenceId>,
    first_revision: bool,
) -> Result<RevisionWrite, String> {
    let thumbnail = match &images {
        Some(images) => ThumbnailSource::Png(encode_thumbnail(images, &document)?),
        None if !first_revision && dirty_layer_ids.is_empty() => ThumbnailSource::ReuseCurrent,
        None => return Err("cannot save changed layers without snapshot pixels".to_owned()),
    };
    let mut images: HashMap<_, _> = images.unwrap_or_default().into_iter().collect();
    let mut layers = Vec::with_capacity(document.layers.len());
    for layer in &document.layers {
        let id = LayerId(layer.id);
        let source = if first_revision || dirty_layer_ids.contains(&id) {
            let image = images
                .remove(&id)
                .ok_or_else(|| format!("missing snapshot pixels for layer {}", layer.id))?;
            LayerSource::Png(encode_png(&image)?)
        } else {
            LayerSource::ReuseCurrent
        };
        layers.push(LayerWrite { id: id.0, source });
    }
    let references = reference_images
        .into_iter()
        .map(|(id, png)| ReferenceWrite {
            id: id.0,
            source: if first_revision || dirty_reference_ids.contains(&id) {
                ReferenceSource::Png(png.as_ref().clone())
            } else {
                ReferenceSource::ReuseCurrent
            },
        })
        .collect();
    Ok(RevisionWrite {
        document,
        layers,
        references,
        thumbnail,
    })
}

fn encode_thumbnail(
    layers: &[(LayerId, image::RgbaImage)],
    document: &crate::artwork::DocumentManifest,
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
    if layers.len() != document.layers.len()
        || layers
            .iter()
            .zip(&document.layers)
            .any(|((id, _), metadata)| id.0 != metadata.id)
    {
        return Err("thumbnail layers do not match document metadata".to_owned());
    }

    let (thumbnail_width, thumbnail_height) = fit_thumbnail_dimensions(source_size, THUMBNAIL_SIZE);
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
    let composite_layers: Vec<_> = resized
        .iter()
        .zip(&document.layers)
        .map(|(image, metadata)| CompositeLayer {
            image,
            visible: metadata.visible,
            opacity: metadata.opacity,
            clipped: metadata.clipped,
        })
        .collect();
    let composite = flatten_premultiplied_layers(&composite_layers, document.background)?;

    let mut thumbnail = image::RgbaImage::new(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    let x = (THUMBNAIL_SIZE - thumbnail_width) / 2;
    let y = (THUMBNAIL_SIZE - thumbnail_height) / 2;
    image::imageops::replace(&mut thumbnail, &composite, i64::from(x), i64::from(y));
    encode_png(&thumbnail)
}

fn fit_thumbnail_dimensions(source: (u32, u32), target: u32) -> (u32, u32) {
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
    use std::fs;

    fn thumbnail_document(
        size: (u32, u32),
        background: [u8; 3],
    ) -> crate::artwork::DocumentManifest {
        crate::artwork::DocumentManifest {
            schema_version: crate::artwork::DOCUMENT_SCHEMA_VERSION,
            width: size.0,
            height: size.1,
            background,
            brush_color: [170, 187, 204, 255],
            selected_layer: 1,
            layers: vec![crate::artwork::LayerManifest {
                id: 1,
                name: "Layer 1".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
                file: "layers/1.png".to_owned(),
            }],
            references: Vec::new(),
        }
    }

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
    fn active_rename_updates_the_title_and_requests_a_save() {
        let mut autosave = AutosaveController::new(None, Arc::new(|| {}));
        autosave.begin_new_session(ArtworkId::new(), "Untitled".to_owned(), [0; 4]);

        autosave.rename_artwork("Study".to_owned());

        assert_eq!(autosave.artwork_title(), Some("Study"));
        assert!(autosave.session.as_ref().unwrap().save_requested);
    }

    #[test]
    fn only_changed_and_new_layers_are_encoded() {
        let saved = versions(4, &[(1, 2), (2, 4)]);
        let current = versions(6, &[(1, 2), (2, 5), (3, 6)]);
        let changed = changed_layer_ids(&saved, &current);
        assert_eq!(changed, HashSet::from([LayerId(2), LayerId(3)]));
    }

    #[test]
    fn metadata_only_changes_reuse_layer_pngs() {
        let saved = versions(4, &[(1, 2), (2, 4)]);
        let mut current = saved.clone();
        current.generation = 5;
        current.metadata = 5;
        assert!(changed_layer_ids(&saved, &current).is_empty());
    }

    #[test]
    fn nonvisual_metadata_saves_need_no_layer_readback_or_thumbnail_encoding() {
        let mut document = thumbnail_document((16, 16), [255; 3]);
        let before = crate::artwork::canvas_document(&document);
        document.selected_layer = 1;
        document.layers[0].name = "Renamed".to_owned();
        document.brush_color = [1, 2, 3, 255];
        assert_eq!(
            VisualSignature::from(&before),
            VisualSignature::from(&crate::artwork::canvas_document(&document))
        );
        let write = build_revision_write(
            document.clone(),
            None,
            &HashSet::new(),
            Vec::new(),
            &HashSet::new(),
            false,
        )
        .unwrap();
        assert!(matches!(write.thumbnail, ThumbnailSource::ReuseCurrent));
        assert!(matches!(write.layers[0].source, LayerSource::ReuseCurrent));
        assert!(
            build_revision_write(
                document.clone(),
                None,
                &HashSet::new(),
                Vec::new(),
                &HashSet::new(),
                true
            )
            .is_err()
        );
        assert!(
            build_revision_write(
                document.clone(),
                None,
                &HashSet::from([LayerId(1)]),
                Vec::new(),
                &HashSet::new(),
                false
            )
            .is_err()
        );

        let mut changed = before;
        changed.layers[0].opacity = 50;
        assert_ne!(
            VisualSignature::from(&changed),
            VisualSignature::from(&crate::artwork::canvas_document(&document))
        );
        changed.layers[0].opacity = 100;
        changed.background = [0; 3];
        assert_ne!(
            VisualSignature::from(&changed),
            VisualSignature::from(&crate::artwork::canvas_document(&document))
        );
    }

    #[test]
    fn reference_metadata_changes_reuse_assets() {
        let saved = ReferenceVersions {
            generation: 2,
            assets: vec![(ReferenceId(1), 4), (ReferenceId(2), 7)],
        };
        let mut moved = saved.clone();
        moved.generation = 3;
        assert!(changed_reference_ids(&saved, &moved).is_empty());

        moved.assets[1].1 = 8;
        assert_eq!(
            changed_reference_ids(&saved, &moved),
            HashSet::from([ReferenceId(2)])
        );
    }

    #[test]
    #[ignore = "requires a wgpu adapter"]
    fn renaming_a_saved_artwork_never_stages_its_layers_again() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let canvas = Canvas::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            [16; 2],
            [16; 2],
            &image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
            [0.16; 3],
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        let mut controller = AutosaveController::new(Some(store.clone()), Arc::new(|| {}));
        let references = ReferenceBoard::default();
        let wait = |controller: &mut AutosaveController| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                controller.update(&canvas, &references);
                if controller.is_clean(&canvas, &references) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!(
                "autosave did not finish: {:?}",
                controller.status(&canvas, &references)
            );
        };
        controller.begin_new_session(id.clone(), "First".into(), [170, 187, 204, 255]);
        controller.request_save();
        wait(&mut controller);
        let before = canvas.work_counters().readback_bytes;
        assert!(before > 0);
        let thumbnail = fs::read(store.load(&id).unwrap().summary.thumbnail_path).unwrap();
        controller.rename_artwork("Second".into());
        assert_eq!(controller.status(&canvas, &references), SaveStatus::Waiting);
        wait(&mut controller);
        assert_eq!(canvas.work_counters().readback_bytes, before);
        let saved = store.load(&id).unwrap();
        assert_eq!(saved.summary.title, "Second");
        assert_eq!(fs::read(saved.summary.thumbnail_path).unwrap(), thumbnail);
    }

    #[test]
    fn thumbnail_composites_premultiplied_layers() {
        let layer = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
        let document = thumbnail_document((1, 1), [0, 0, 255]);
        let png = encode_thumbnail(&[(LayerId(1), layer)], &document).unwrap();
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
        let document = thumbnail_document((4, 2), [10, 20, 30]);
        let png = encode_thumbnail(&[(LayerId(1), layer)], &document).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([0, 0, 0, 0]));
        assert_eq!(
            decoded.get_pixel(0, THUMBNAIL_SIZE / 2),
            &image::Rgba([10, 20, 30, 255])
        );
    }
}
