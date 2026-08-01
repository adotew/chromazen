use std::{collections::HashSet, fs, path::Path};

use image::{GrayImage, Rgba, RgbaImage, imageops::FilterType};
use uuid::Uuid;

use super::{
    ConfigError, ConfigStore,
    abr::{AbrBrush, parse_abr},
    brush::{BrushPreset, PressureConfig, SizeConfig, SpacingConfig},
};

const MAX_ABR_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_IMPORTED_STAMP_DIMENSION: u32 = 4_096;
const FALLBACK_SPACING_RATIO: f32 = 0.05;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BrushImportResult {
    pub(crate) imported_ids: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl ConfigStore {
    pub(crate) fn import_abr(&self, path: &Path) -> Result<BrushImportResult, ConfigError> {
        let metadata =
            fs::metadata(path).map_err(|error| ConfigError::io("inspect", path, error))?;
        if metadata.len() > MAX_ABR_FILE_BYTES {
            return Err(ConfigError::new(format!(
                "ABR file {} is larger than {} MiB",
                path.display(),
                MAX_ABR_FILE_BYTES / 1024 / 1024
            )));
        }
        let bytes = fs::read(path).map_err(|error| ConfigError::io("read", path, error))?;
        let parsed = parse_abr(&bytes).map_err(|error| {
            ConfigError::new(format!("failed to parse {}: {error}", path.display()))
        })?;
        let source_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .unwrap_or("Imported ABR");

        fs::create_dir_all(&self.root).map_err(|error| {
            ConfigError::io("create configuration directory for", &self.root, error)
        })?;
        fs::create_dir_all(self.brushes_path()).map_err(|error| {
            ConfigError::io("create brush directory", &self.brushes_path(), error)
        })?;
        let staging_path = self.root.join(format!(".brush-import-{}", Uuid::new_v4()));
        fs::create_dir(&staging_path).map_err(|error| {
            ConfigError::io("create import staging directory", &staging_path, error)
        })?;
        let staging = StagingDirectory(&staging_path);

        let mut reserved_ids = HashSet::new();
        let mut imported_ids = Vec::new();
        let mut warnings = parsed.warnings;
        for (index, brush) in parsed.brushes.into_iter().enumerate() {
            let fallback_name = format!("{source_name} {:03}", index + 1);
            let name = brush
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(&fallback_name)
                .to_owned();
            let id = available_brush_id(
                &self.brushes_path(),
                &mut reserved_ids,
                &format!("abr-{source_name}-{name}"),
            );
            let staged_brush = staging.0.join(&id);

            match stage_brush(&staged_brush, &name, brush).and_then(|()| {
                let destination = self.brushes_path().join(&id);
                fs::rename(&staged_brush, &destination).map_err(|error| {
                    ConfigError::io("install imported brush at", &destination, error)
                })
            }) {
                Ok(()) => imported_ids.push(id),
                Err(error) => warnings.push(format!("Could not import brush {name:?}: {error}")),
            }
        }

        if imported_ids.is_empty() {
            return Err(ConfigError::new(warnings.first().cloned().unwrap_or_else(
                || "the ABR did not contain a usable brush".to_owned(),
            )));
        }
        drop(staging);

        Ok(BrushImportResult {
            imported_ids,
            warnings,
        })
    }
}

fn stage_brush(path: &Path, name: &str, brush: AbrBrush) -> Result<(), ConfigError> {
    fs::create_dir(path)
        .map_err(|error| ConfigError::io("create staged brush directory", path, error))?;
    let image = stamp_image(&brush)?;
    let diameter = brush.width.max(brush.height).clamp(1, 2_000) as f32;
    let preset = BrushPreset {
        name: name.to_owned(),
        stamp: "tip.png".to_owned(),
        size: SizeConfig {
            default: diameter,
            min: 1.0,
            max: (diameter * 4.0).clamp(2_000.0, 8_000.0),
        },
        spacing: SpacingConfig {
            ratio: brush
                .spacing_percent
                .map_or(FALLBACK_SPACING_RATIO, |spacing| spacing / 100.0),
            minimum: 1.0,
        },
        pressure: PressureConfig::default(),
        ..BrushPreset::default()
    };
    preset.validate()?;
    let serialized = toml::to_string_pretty(&preset).map_err(|error| {
        ConfigError::new(format!("failed to serialize imported brush: {error}"))
    })?;
    fs::write(path.join("brush.toml"), serialized)
        .map_err(|error| ConfigError::io("write", &path.join("brush.toml"), error))?;
    image.save(path.join("tip.png")).map_err(|error| {
        ConfigError::new(format!("failed to write imported brush tip: {error}"))
    })?;
    Ok(())
}

fn stamp_image(brush: &AbrBrush) -> Result<RgbaImage, ConfigError> {
    let grayscale = GrayImage::from_raw(brush.width, brush.height, brush.mask.clone())
        .ok_or_else(|| ConfigError::new("ABR brush tip dimensions do not match its image data"))?;
    let grayscale = if brush.width > MAX_IMPORTED_STAMP_DIMENSION
        || brush.height > MAX_IMPORTED_STAMP_DIMENSION
    {
        let scale = (MAX_IMPORTED_STAMP_DIMENSION as f64 / brush.width as f64)
            .min(MAX_IMPORTED_STAMP_DIMENSION as f64 / brush.height as f64);
        let width = (brush.width as f64 * scale).round().max(1.0) as u32;
        let height = (brush.height as f64 * scale).round().max(1.0) as u32;
        image::imageops::resize(&grayscale, width, height, FilterType::Lanczos3)
    } else {
        grayscale
    };

    Ok(RgbaImage::from_fn(
        grayscale.width(),
        grayscale.height(),
        |x, y| Rgba([255, 255, 255, grayscale.get_pixel(x, y).0[0]]),
    ))
}

fn available_brush_id(
    brushes_path: &Path,
    reserved_ids: &mut HashSet<String>,
    candidate: &str,
) -> String {
    let base = slug(candidate);
    for suffix in 1u32.. {
        let id = if suffix == 1 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if !reserved_ids.contains(&id) && !brushes_path.join(&id).exists() {
            reserved_ids.insert(id.clone());
            return id;
        }
    }
    unreachable!("u32 brush suffix space exhausted")
}

fn slug(value: &str) -> String {
    let mut slug = String::with_capacity(value.len().min(80));
    let mut separator_pending = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() && slug.len() < 80 {
                slug.push('-');
            }
            separator_pending = false;
            if slug.len() < 80 {
                slug.push(character.to_ascii_lowercase());
            }
        } else {
            separator_pending = true;
        }
    }
    if slug.is_empty() {
        "abr-imported-brush".to_owned()
    } else {
        slug
    }
}

struct StagingDirectory<'a>(&'a Path);

impl Drop for StagingDirectory<'_> {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "failed to remove brush import staging directory {}: {error}",
                self.0.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_abr_as_native_brush() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ConfigStore::from_root(temp.path());
        let abr_path = temp.path().join("My Inks.abr");
        fs::write(&abr_path, legacy_abr("Soft Ink", 12, 2, 1, &[0, 255])).expect("ABR fixture");

        let result = store.import_abr(&abr_path).expect("import");

        assert_eq!(result.imported_ids, vec!["abr-my-inks-soft-ink"]);
        let loaded = store
            .load_brush(&result.imported_ids[0])
            .expect("installed brush");
        assert_eq!(loaded.preset.name, "Soft Ink");
        assert_eq!(loaded.preset.spacing.ratio, 0.12);
        assert_eq!(loaded.preset.size.default, 2.0);
        assert_eq!(
            loaded.stamp_image.expect("tip").get_pixel(1, 0).0,
            [255, 255, 255, 255]
        );
    }

    #[test]
    fn repeated_import_does_not_overwrite_existing_brush() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ConfigStore::from_root(temp.path());
        let abr_path = temp.path().join("set.abr");
        fs::write(&abr_path, legacy_abr("Ink", 25, 1, 1, &[255])).expect("ABR fixture");

        let first = store.import_abr(&abr_path).expect("first import");
        let second = store.import_abr(&abr_path).expect("second import");

        assert_eq!(first.imported_ids, vec!["abr-set-ink"]);
        assert_eq!(second.imported_ids, vec!["abr-set-ink-2"]);
    }

    #[test]
    fn rejects_invalid_file_without_installing_brushes() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ConfigStore::from_root(temp.path());
        let abr_path = temp.path().join("bad.abr");
        fs::write(&abr_path, b"not an abr").expect("ABR fixture");

        assert!(store.import_abr(&abr_path).is_err());
        assert!(!store.brushes_path().exists());
    }

    #[test]
    fn missing_abr_spacing_uses_dense_fallback() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let brush_path = temp.path().join("brush");
        let brush = AbrBrush {
            name: None,
            sample_id: None,
            width: 1,
            height: 1,
            mask: vec![255],
            spacing_percent: None,
        };

        stage_brush(&brush_path, "Imported", brush).expect("staged brush");
        let source = fs::read_to_string(brush_path.join("brush.toml")).expect("preset");
        let preset: BrushPreset = toml::from_str(&source).expect("parsed preset");

        assert_eq!(preset.spacing.ratio, 0.05);
    }

    #[test]
    fn oversized_tip_is_scaled_to_native_limit() {
        let brush = AbrBrush {
            name: None,
            sample_id: None,
            width: MAX_IMPORTED_STAMP_DIMENSION + 1,
            height: 1,
            mask: vec![255; (MAX_IMPORTED_STAMP_DIMENSION + 1) as usize],
            spacing_percent: None,
        };

        let image = stamp_image(&brush).expect("scaled stamp");

        assert_eq!(image.dimensions(), (MAX_IMPORTED_STAMP_DIMENSION, 1));
    }

    #[test]
    fn slug_is_safe_and_bounded() {
        assert_eq!(slug("  My Ink / 02  "), "my-ink-02");
        assert_eq!(slug("画筆"), "abr-imported-brush");
        assert!(slug(&"a".repeat(200)).len() <= 80);
    }

    fn legacy_abr(name: &str, spacing: u16, width: i32, height: i32, mask: &[u8]) -> Vec<u8> {
        let utf16 = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut sample = Vec::new();
        sample.extend_from_slice(&0u32.to_be_bytes());
        sample.extend_from_slice(&spacing.to_be_bytes());
        sample.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        for character in utf16 {
            sample.extend_from_slice(&character.to_be_bytes());
        }
        sample.push(1);
        sample.extend_from_slice(&[0; 8]);
        for value in [0i32, 0, height, width] {
            sample.extend_from_slice(&value.to_be_bytes());
        }
        sample.extend_from_slice(&8u16.to_be_bytes());
        sample.push(0);
        sample.extend_from_slice(mask);

        let mut abr = Vec::new();
        abr.extend_from_slice(&2u16.to_be_bytes());
        abr.extend_from_slice(&1u16.to_be_bytes());
        abr.extend_from_slice(&2u16.to_be_bytes());
        abr.extend_from_slice(&(sample.len() as u32).to_be_bytes());
        abr.extend_from_slice(&sample);
        abr
    }
}
