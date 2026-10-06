use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use chromazen_brush::{
    BUNDLED_BRUSH_IDS, BrushPreset, LoadedBrushPreset, MAX_STAMP_DIMENSION, PressureConfig,
    SizeConfig, SpacingConfig, bundled_preset, bundled_stamp,
};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};

use super::ConfigError;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) preview: BrushPreviewSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushPreviewSpec {
    pub(crate) stamp_path: Option<PathBuf>,
    pub(crate) size: SizeConfig,
    pub(crate) spacing: SpacingConfig,
    pub(crate) pressure: PressureConfig,
}

impl BrushSummary {
    fn new(id: String, preset: BrushPreset, stamp_path: Option<PathBuf>) -> Self {
        Self {
            id,
            name: preset.name,
            preview: BrushPreviewSpec {
                stamp_path,
                size: preset.size,
                spacing: preset.spacing,
                pressure: preset.pressure,
            },
        }
    }

    pub(crate) fn load_preview_stamp(&self) -> Result<RgbaImage, ConfigError> {
        if let Some(path) = &self.preview.stamp_path {
            return decode_stamp(path);
        }
        Ok(bundled_stamp(&self.id)?)
    }
}

pub(crate) struct BrushCatalog {
    pub(crate) brushes: Vec<BrushSummary>,
    pub(crate) warnings: Vec<String>,
}

impl Default for BrushCatalog {
    fn default() -> Self {
        Self {
            brushes: BUNDLED_BRUSH_IDS
                .into_iter()
                .filter_map(|id| {
                    let preset = bundled_preset(id)?;
                    Some(BrushSummary::new(id.to_owned(), preset, None))
                })
                .collect(),
            warnings: Vec::new(),
        }
    }
}

pub(super) fn load_user_brush(
    brushes_root: &Path,
    id: &str,
) -> Result<LoadedBrushPreset, ConfigError> {
    let (preset, stamp_path) = load_user_brush_metadata(brushes_root, id)?;
    let stamp_image = decode_stamp(&stamp_path)?;

    Ok(LoadedBrushPreset {
        id: id.to_owned(),
        preset,
        stamp_image,
    })
}

fn load_user_brush_summary(brushes_root: &Path, id: &str) -> Result<BrushSummary, ConfigError> {
    let (preset, stamp_path) = load_user_brush_metadata(brushes_root, id)?;
    open_stamp_reader(&stamp_path)?
        .into_dimensions()
        .map_err(|error| {
            ConfigError::new(format!(
                "failed to inspect brush stamp {}: {error}",
                stamp_path.display()
            ))
        })?;

    Ok(BrushSummary::new(id.to_owned(), preset, Some(stamp_path)))
}

fn load_user_brush_metadata(
    brushes_root: &Path,
    id: &str,
) -> Result<(BrushPreset, PathBuf), ConfigError> {
    validate_brush_id(id)?;
    let preset_dir = brushes_root.join(id);
    let config_path = preset_dir.join("brush.toml");
    let source = fs::read_to_string(&config_path)
        .map_err(|error| ConfigError::io("read", &config_path, error))?;
    let preset: BrushPreset = toml::from_str(&source).map_err(|error| {
        ConfigError::new(format!(
            "failed to parse {}: {error}",
            config_path.display()
        ))
    })?;
    preset.validate().map_err(|error| {
        ConfigError::new(format!(
            "invalid brush preset in {}: {error}",
            config_path.display()
        ))
    })?;

    let canonical_dir = preset_dir
        .canonicalize()
        .map_err(|error| ConfigError::io("resolve", &preset_dir, error))?;
    let stamp_path = preset_dir.join(&preset.stamp);
    let canonical_stamp = stamp_path
        .canonicalize()
        .map_err(|error| ConfigError::io("resolve", &stamp_path, error))?;
    if !canonical_stamp.starts_with(&canonical_dir) {
        return Err(ConfigError::new(format!(
            "stamp path {} escapes brush directory {}",
            stamp_path.display(),
            preset_dir.display()
        )));
    }

    Ok((preset, canonical_stamp))
}

fn decode_stamp(path: &Path) -> Result<RgbaImage, ConfigError> {
    open_stamp_reader(path)?
        .decode()
        .map(image::DynamicImage::into_rgba8)
        .map_err(|error| {
            ConfigError::new(format!(
                "failed to decode brush stamp {}: {error}",
                path.display()
            ))
        })
}

fn open_stamp_reader(path: &Path) -> Result<ImageReader<BufReader<File>>, ConfigError> {
    let mut reader = ImageReader::open(path)
        .map_err(|error| ConfigError::io("open", path, error))?
        .with_guessed_format()
        .map_err(|error| ConfigError::io("inspect", path, error))?;
    if reader.format() != Some(ImageFormat::Png) {
        return Err(ConfigError::new(format!(
            "brush stamp {} must be a PNG image",
            path.display()
        )));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_STAMP_DIMENSION);
    limits.max_image_height = Some(MAX_STAMP_DIMENSION);
    reader.limits(limits);
    Ok(reader)
}

pub(super) fn discover_user_brushes(brushes_root: &Path) -> BrushCatalog {
    let mut catalog = BrushCatalog::default();

    let entries = match fs::read_dir(brushes_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return catalog,
        Err(error) => {
            catalog
                .warnings
                .push(ConfigError::io("read brush directory", brushes_root, error).to_string());
            return catalog;
        }
    };

    let mut ids = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    ids.sort();

    for id in ids {
        match load_user_brush_summary(brushes_root, &id) {
            Ok(summary) => {
                if let Some(existing) = catalog
                    .brushes
                    .iter_mut()
                    .find(|brush| brush.id == summary.id)
                {
                    *existing = summary;
                } else {
                    catalog.brushes.push(summary);
                }
            }
            Err(error) => catalog.warnings.push(error.to_string()),
        }
    }

    catalog
}

fn validate_brush_id(id: &str) -> Result<(), ConfigError> {
    let mut components = Path::new(id).components();
    if id.trim().is_empty()
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(ConfigError::new(format!("invalid brush ID {id:?}")));
    }
    Ok(())
}
