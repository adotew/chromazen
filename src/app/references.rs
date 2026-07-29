use std::{fs, path::Path, sync::Arc};

use crate::artwork::{ReferenceManifest, encode_png};

const DEFAULT_REFERENCE_MAX_EDGE: f32 = 1200.0;
const MAX_REFERENCE_DIMENSION: u32 = 8192;
const MAX_REFERENCE_PIXELS: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ReferenceId(pub(crate) u64);

#[derive(Clone)]
pub(crate) struct DecodedReference {
    pub(crate) pixels: Arc<image::RgbaImage>,
    pub(crate) png: Arc<Vec<u8>>,
}

#[derive(Clone)]
pub(crate) struct ReferenceImage {
    pub(crate) id: ReferenceId,
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) visible: bool,
    pub(crate) locked: bool,
    pub(crate) resource_version: u64,
    pub(crate) pixels: Arc<image::RgbaImage>,
    pub(crate) png: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReferenceVersions {
    pub(crate) generation: u64,
    pub(crate) assets: Vec<(ReferenceId, u64)>,
}

#[derive(Clone)]
pub(crate) struct ReferenceBoardSnapshot {
    images: Vec<ReferenceImage>,
}

#[derive(Default)]
pub(crate) struct ReferenceBoard {
    images: Vec<ReferenceImage>,
    generation: u64,
    next_id: u64,
    next_resource_version: u64,
}

impl ReferenceBoard {
    pub(crate) fn images(&self) -> &[ReferenceImage] {
        &self.images
    }

    pub(crate) fn snapshot(&self) -> ReferenceBoardSnapshot {
        ReferenceBoardSnapshot {
            images: self.images.clone(),
        }
    }

    pub(crate) fn restore(&mut self, snapshot: ReferenceBoardSnapshot) {
        self.images = snapshot.images;
        self.next_id = self
            .images
            .iter()
            .map(|reference| reference.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        self.mark_changed();
    }

    pub(crate) fn clear(&mut self) {
        self.images.clear();
        self.generation = 0;
        self.next_id = 1;
        self.next_resource_version = self.next_resource_version.saturating_add(1).max(1);
    }

    pub(crate) fn load(&mut self, references: Vec<(ReferenceManifest, DecodedReference)>) {
        self.clear();
        self.images = references
            .into_iter()
            .map(|(manifest, decoded)| {
                let resource_version = self.allocate_resource_version();
                ReferenceImage {
                    id: ReferenceId(manifest.id),
                    position: manifest.position,
                    size: manifest.size,
                    visible: manifest.visible,
                    locked: manifest.locked,
                    resource_version,
                    pixels: decoded.pixels,
                    png: decoded.png,
                }
            })
            .collect();
        self.next_id = self
            .images
            .iter()
            .map(|reference| reference.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        self.generation = 0;
    }

    pub(crate) fn add(&mut self, decoded: DecodedReference, position: [f32; 2]) -> ReferenceId {
        let id = ReferenceId(self.next_id.max(1));
        self.next_id = id.0.saturating_add(1);
        let size = fitted_display_size(decoded.pixels.dimensions(), DEFAULT_REFERENCE_MAX_EDGE);
        let resource_version = self.allocate_resource_version();
        self.images.push(ReferenceImage {
            id,
            position,
            size,
            visible: true,
            locked: false,
            resource_version,
            pixels: decoded.pixels,
            png: decoded.png,
        });
        self.mark_changed();
        id
    }

    pub(crate) fn remove(&mut self, id: ReferenceId) -> bool {
        let Some(index) = self.images.iter().position(|image| image.id == id) else {
            return false;
        };
        self.images.remove(index);
        self.mark_changed();
        true
    }

    pub(crate) fn set_transform(
        &mut self,
        id: ReferenceId,
        position: [f32; 2],
        size: [f32; 2],
    ) -> bool {
        if position.into_iter().any(|value| !value.is_finite())
            || size
                .into_iter()
                .any(|value| !value.is_finite() || value <= 0.0)
        {
            return false;
        }
        let Some(reference) = self.images.iter_mut().find(|image| image.id == id) else {
            return false;
        };
        if reference.locked || reference.position == position && reference.size == size {
            return false;
        }
        reference.position = position;
        reference.size = size;
        self.mark_changed();
        true
    }

    pub(crate) fn toggle_locked(&mut self, id: ReferenceId) -> bool {
        let Some(reference) = self.images.iter_mut().find(|image| image.id == id) else {
            return false;
        };
        reference.locked = !reference.locked;
        self.mark_changed();
        true
    }

    pub(crate) fn toggle_visible(&mut self, id: ReferenceId) -> bool {
        let Some(reference) = self.images.iter_mut().find(|image| image.id == id) else {
            return false;
        };
        reference.visible = !reference.visible;
        self.mark_changed();
        true
    }

    pub(crate) fn bring_forward(&mut self, id: ReferenceId) -> bool {
        let Some(index) = self.images.iter().position(|image| image.id == id) else {
            return false;
        };
        if index + 1 == self.images.len() {
            return false;
        }
        self.images.swap(index, index + 1);
        self.mark_changed();
        true
    }

    pub(crate) fn send_backward(&mut self, id: ReferenceId) -> bool {
        let Some(index) = self.images.iter().position(|image| image.id == id) else {
            return false;
        };
        if index == 0 {
            return false;
        }
        self.images.swap(index, index - 1);
        self.mark_changed();
        true
    }

    pub(crate) fn manifest(&self) -> Vec<ReferenceManifest> {
        self.images
            .iter()
            .map(|reference| ReferenceManifest {
                id: reference.id.0,
                file: format!("references/{}.png", reference.id.0),
                position: reference.position,
                size: reference.size,
                visible: reference.visible,
                locked: reference.locked,
            })
            .collect()
    }

    pub(crate) fn versions(&self) -> ReferenceVersions {
        let mut assets: Vec<_> = self
            .images
            .iter()
            .map(|reference| (reference.id, reference.resource_version))
            .collect();
        assets.sort_by_key(|(id, _)| id.0);
        ReferenceVersions {
            generation: self.generation,
            assets,
        }
    }

    fn allocate_resource_version(&mut self) -> u64 {
        let version = self.next_resource_version.max(1);
        self.next_resource_version = version.saturating_add(1);
        version
    }

    fn mark_changed(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }
}

pub(crate) fn decode_reference_file(path: &Path) -> Result<DecodedReference, String> {
    let contents = fs::read(path)
        .map_err(|error| format!("failed to read reference {}: {error}", path.display()))?;
    decode_reference_bytes(&contents, &path.display().to_string())
}

fn decode_reference_bytes(contents: &[u8], label: &str) -> Result<DecodedReference, String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(contents))
        .with_guessed_format()
        .map_err(|error| format!("failed to identify reference {label}: {error}"))?;
    let dimensions = reader
        .into_dimensions()
        .map_err(|error| format!("failed to inspect reference {label}: {error}"))?;
    validate_source_dimensions(dimensions)?;
    let image = image::load_from_memory(contents)
        .map_err(|error| format!("failed to decode reference {label}: {error}"))?
        .to_rgba8();
    let png = encode_png(&image)?;
    Ok(DecodedReference {
        pixels: Arc::new(image),
        png: Arc::new(png),
    })
}

fn validate_source_dimensions(size: (u32, u32)) -> Result<(), String> {
    if size.0 == 0 || size.1 == 0 {
        return Err("reference dimensions must be non-zero".to_owned());
    }
    if size.0 > MAX_REFERENCE_DIMENSION || size.1 > MAX_REFERENCE_DIMENSION {
        return Err(format!(
            "reference dimensions cannot exceed {MAX_REFERENCE_DIMENSION} pixels"
        ));
    }
    if u64::from(size.0) * u64::from(size.1) > MAX_REFERENCE_PIXELS {
        return Err(format!(
            "reference area cannot exceed {} megapixels",
            MAX_REFERENCE_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

fn fitted_display_size(source: (u32, u32), max_edge: f32) -> [f32; 2] {
    let scale = (max_edge / source.0.max(source.1) as f32).min(1.0);
    [source.0 as f32 * scale, source.1 as f32 * scale]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(width: u32, height: u32) -> DecodedReference {
        let image = image::RgbaImage::new(width, height);
        DecodedReference {
            png: Arc::new(encode_png(&image).unwrap()),
            pixels: Arc::new(image),
        }
    }

    #[test]
    fn imported_references_are_sized_and_versioned() {
        let mut board = ReferenceBoard::default();
        let id = board.add(decoded(2400, 1200), [4100.0, 100.0]);
        assert_eq!(board.images()[0].size, [1200.0, 600.0]);
        assert_eq!(board.versions().generation, 1);
        assert_eq!(board.manifest()[0].id, id.0);
    }

    #[test]
    fn transforms_reject_invalid_geometry_and_locked_images() {
        let mut board = ReferenceBoard::default();
        let id = board.add(decoded(10, 10), [0.0, 0.0]);
        assert!(!board.set_transform(id, [f32::NAN, 0.0], [10.0, 10.0]));
        assert!(board.toggle_locked(id));
        assert!(!board.set_transform(id, [20.0, 20.0], [10.0, 10.0]));
    }

    #[test]
    fn loading_preserves_order_and_advances_ids() {
        let manifest = |id| ReferenceManifest {
            id,
            file: format!("references/{id}.png"),
            position: [id as f32, 0.0],
            size: [10.0, 10.0],
            visible: true,
            locked: false,
        };
        let mut board = ReferenceBoard::default();
        board.load(vec![
            (manifest(7), decoded(10, 10)),
            (manifest(2), decoded(10, 10)),
        ]);
        assert_eq!(
            board
                .manifest()
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            [7, 2]
        );
        assert_eq!(board.add(decoded(10, 10), [0.0, 0.0]), ReferenceId(8));
    }

    #[test]
    fn decodes_png_and_rejects_oversized_dimensions() {
        let source = decoded(2, 1);
        let decoded = decode_reference_bytes(source.png.as_slice(), "memory").unwrap();
        assert_eq!(decoded.pixels.dimensions(), (2, 1));
        assert!(validate_source_dimensions((MAX_REFERENCE_DIMENSION + 1, 1)).is_err());
    }
}
