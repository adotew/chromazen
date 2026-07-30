use std::{fs, path::Path, sync::Arc};

use image::GenericImageView;

use crate::artwork::{ReferenceManifest, encode_png};

const DEFAULT_REFERENCE_MAX_EDGE: f32 = 1200.0;
const MAX_REFERENCE_DIMENSION: u32 = 8192;
const MAX_REFERENCE_PIXELS: u64 = 32 * 1024 * 1024;
// Source limits bound transient decoder memory while allowing ordinary oversized references to be
// reduced to the smaller texture limits above.
const MAX_REFERENCE_SOURCE_DIMENSION: u32 = 16_384;
const MAX_REFERENCE_SOURCE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_REFERENCE_DECODE_ALLOCATION: u64 = 512 * 1024 * 1024;

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

    let mut reader = image::ImageReader::new(std::io::Cursor::new(contents))
        .with_guessed_format()
        .map_err(|error| format!("failed to identify reference {label}: {error}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_REFERENCE_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_REFERENCE_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_REFERENCE_DECODE_ALLOCATION);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("failed to decode reference {label}: {error}"))?;
    prepare_reference(image, fitted_reference_pixel_size(dimensions))
}

fn prepare_reference(
    image: image::DynamicImage,
    target_size: (u32, u32),
) -> Result<DecodedReference, String> {
    let image = if image.dimensions() == target_size {
        image.into_rgba8()
    } else {
        image
            .resize_exact(
                target_size.0,
                target_size.1,
                image::imageops::FilterType::Lanczos3,
            )
            .into_rgba8()
    };
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
    if size.0 > MAX_REFERENCE_SOURCE_DIMENSION || size.1 > MAX_REFERENCE_SOURCE_DIMENSION {
        return Err(format!(
            "reference source dimensions cannot exceed {MAX_REFERENCE_SOURCE_DIMENSION} pixels"
        ));
    }
    if u64::from(size.0) * u64::from(size.1) > MAX_REFERENCE_SOURCE_PIXELS {
        return Err(format!(
            "reference source area cannot exceed {} megapixels",
            MAX_REFERENCE_SOURCE_PIXELS / 1_000_000
        ));
    }
    Ok(())
}

fn fitted_reference_pixel_size(source: (u32, u32)) -> (u32, u32) {
    let dimension_scale = f64::from(MAX_REFERENCE_DIMENSION) / f64::from(source.0.max(source.1));
    let area = f64::from(source.0) * f64::from(source.1);
    let area_scale = (MAX_REFERENCE_PIXELS as f64 / area).sqrt();
    let scale = dimension_scale.min(area_scale).min(1.0);
    (
        (f64::from(source.0) * scale).floor().max(1.0) as u32,
        (f64::from(source.1) * scale).floor().max(1.0) as u32,
    )
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
    fn decodes_png_and_rejects_unsafe_source_dimensions() {
        let source = decoded(2, 1);
        let decoded = decode_reference_bytes(source.png.as_slice(), "memory").unwrap();
        assert_eq!(decoded.pixels.dimensions(), (2, 1));
        assert!(validate_source_dimensions((0, 1)).is_err());
        assert!(validate_source_dimensions((8192, 8192)).is_ok());
        assert!(validate_source_dimensions((MAX_REFERENCE_SOURCE_DIMENSION + 1, 1)).is_err());
        assert!(validate_source_dimensions((MAX_REFERENCE_SOURCE_DIMENSION, 8192)).is_err());
    }

    #[test]
    fn fitted_reference_pixels_obey_texture_limits() {
        assert_eq!(fitted_reference_pixel_size((4000, 2000)), (4000, 2000));

        for source in [(12_000, 2_000), (8_000, 8_000), (16_384, 4_096)] {
            let fitted = fitted_reference_pixel_size(source);
            assert!(fitted.0 <= MAX_REFERENCE_DIMENSION);
            assert!(fitted.1 <= MAX_REFERENCE_DIMENSION);
            assert!(u64::from(fitted.0) * u64::from(fitted.1) <= MAX_REFERENCE_PIXELS);
            let source_ratio = f64::from(source.0) / f64::from(source.1);
            let fitted_ratio = f64::from(fitted.0) / f64::from(fitted.1);
            assert!((source_ratio - fitted_ratio).abs() < 0.01);
        }
    }

    #[test]
    fn prepared_references_persist_resized_pixels() {
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::new(4, 2));
        let prepared = prepare_reference(source, (2, 1)).unwrap();
        assert_eq!(prepared.pixels.dimensions(), (2, 1));
        assert_eq!(
            image::load_from_memory(prepared.png.as_slice())
                .unwrap()
                .dimensions(),
            (2, 1)
        );
    }
}
