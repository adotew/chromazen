use std::{fs, path::Path, sync::Arc};

use image::GenericImageView;

use crate::artwork::{ReferenceManifest, encode_png};

const DEFAULT_REFERENCE_MAX_EDGE: f32 = 1200.0;
const MAX_REFERENCE_DIMENSION: u32 = 8192;
const MAX_REFERENCE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_REFERENCE_TEXTURE_DIMENSION: u32 = 4096;
const MAX_REFERENCE_TEXTURE_PIXELS: u64 = 8 * 1024 * 1024;
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

    pub(crate) fn replace_with_loaded_references(
        &mut self,
        references: Vec<(ReferenceManifest, DecodedReference)>,
    ) {
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

pub(crate) fn decode_stored_reference_file(path: &Path) -> Result<DecodedReference, String> {
    let contents = fs::read(path)
        .map_err(|error| format!("failed to read reference {}: {error}", path.display()))?;
    let image = decode_reference_image(&contents, &path.display().to_string())?;
    Ok(prepare_stored_reference(image, contents))
}

fn decode_reference_bytes(contents: &[u8], label: &str) -> Result<DecodedReference, String> {
    let image = decode_reference_image(contents, label)?;
    let target_size = fitted_reference_pixel_size(image.dimensions());
    prepare_imported_reference(image, target_size)
}

fn decode_reference_image(contents: &[u8], label: &str) -> Result<image::DynamicImage, String> {
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
    reader
        .decode()
        .map_err(|error| format!("failed to decode reference {label}: {error}"))
}

fn prepare_imported_reference(
    image: image::DynamicImage,
    target_size: (u32, u32),
) -> Result<DecodedReference, String> {
    let image = resize_reference(image, target_size);
    let png = encode_png(&image)?;
    let pixels = fit_reference_pixels_for_texture(image);
    Ok(DecodedReference {
        pixels: Arc::new(pixels),
        png: Arc::new(png),
    })
}

fn prepare_stored_reference(image: image::DynamicImage, png: Vec<u8>) -> DecodedReference {
    let texture_size = fitted_texture_pixel_size(image.dimensions());
    DecodedReference {
        pixels: Arc::new(resize_reference(image, texture_size)),
        png: Arc::new(png),
    }
}

fn resize_reference(image: image::DynamicImage, target_size: (u32, u32)) -> image::RgbaImage {
    if image.dimensions() == target_size {
        image.into_rgba8()
    } else {
        image
            .resize_exact(
                target_size.0,
                target_size.1,
                image::imageops::FilterType::Lanczos3,
            )
            .into_rgba8()
    }
}

fn fit_reference_pixels_for_texture(image: image::RgbaImage) -> image::RgbaImage {
    let texture_size = fitted_texture_pixel_size(image.dimensions());
    if image.dimensions() == texture_size {
        image
    } else {
        image::imageops::resize(
            &image,
            texture_size.0,
            texture_size.1,
            image::imageops::FilterType::Lanczos3,
        )
    }
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
    fitted_pixel_size(source, MAX_REFERENCE_DIMENSION, MAX_REFERENCE_PIXELS)
}

fn fitted_texture_pixel_size(source: (u32, u32)) -> (u32, u32) {
    fitted_pixel_size(
        source,
        MAX_REFERENCE_TEXTURE_DIMENSION,
        MAX_REFERENCE_TEXTURE_PIXELS,
    )
}

fn fitted_pixel_size(source: (u32, u32), max_dimension: u32, max_pixels: u64) -> (u32, u32) {
    let dimension_scale = f64::from(max_dimension) / f64::from(source.0.max(source.1));
    let area = f64::from(source.0) * f64::from(source.1);
    let area_scale = (max_pixels as f64 / area).sqrt();
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
mod tests;
