use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use serde::{Deserialize, Serialize};

use super::ConfigError;

pub(crate) const DEFAULT_BRUSH_ID: &str = "charcoal";
pub(crate) const SKETCH_ID: &str = "sketch";
const BRUSH_SCHEMA_VERSION: u32 = 1;
const MAX_STAMP_DIMENSION: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct BrushPreset {
    pub(crate) schema_version: u32,
    pub(crate) name: String,
    pub(crate) stamp: String,
    pub(crate) size: SizeConfig,
    pub(crate) spacing: SpacingConfig,
    pub(crate) pressure: PressureConfig,
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            schema_version: BRUSH_SCHEMA_VERSION,
            name: "Charcoal".to_owned(),
            stamp: "stamp.png".to_owned(),
            size: SizeConfig::default(),
            spacing: SpacingConfig::default(),
            pressure: PressureConfig::default(),
        }
    }
}

impl BrushPreset {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != BRUSH_SCHEMA_VERSION {
            return Err(ConfigError::new(format!(
                "unsupported brush schema_version {}; expected {BRUSH_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(ConfigError::new("brush name must not be empty"));
        }
        validate_finite_positive("size.min", self.size.min)?;
        validate_finite_positive("size.max", self.size.max)?;
        validate_finite_positive("size.default", self.size.default)?;
        if self.size.max < self.size.min {
            return Err(ConfigError::new(
                "size.max must be greater than or equal to size.min",
            ));
        }
        if !(self.size.min..=self.size.max).contains(&self.size.default) {
            return Err(ConfigError::new(
                "size.default must be between size.min and size.max",
            ));
        }
        validate_finite_non_negative("spacing.ratio", self.spacing.ratio)?;
        validate_finite_at_least("spacing.minimum", self.spacing.minimum, 1.0)?;
        validate_unit("pressure.min_size", self.pressure.min_size)?;
        validate_unit("pressure.min_opacity", self.pressure.min_opacity)?;
        validate_finite_positive("pressure.opacity_gamma", self.pressure.opacity_gamma)?;
        validate_stamp_path(&self.stamp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SizeConfig {
    pub(crate) default: f32,
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl Default for SizeConfig {
    fn default() -> Self {
        Self {
            default: 300.0,
            min: 1.0,
            max: 2000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SpacingConfig {
    pub(crate) ratio: f32,
    pub(crate) minimum: f32,
}

impl Default for SpacingConfig {
    fn default() -> Self {
        Self {
            ratio: 0.25,
            minimum: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PressureConfig {
    pub(crate) min_size: f32,
    pub(crate) min_opacity: f32,
    pub(crate) opacity_gamma: f32,
}

impl Default for PressureConfig {
    fn default() -> Self {
        Self {
            min_size: 0.45,
            min_opacity: 0.08,
            opacity_gamma: 1.35,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoadedBrushPreset {
    pub(crate) id: String,
    pub(crate) preset: BrushPreset,
    pub(crate) stamp_image: Option<RgbaImage>,
}

impl LoadedBrushPreset {
    pub(crate) fn bundled_charcoal() -> Self {
        Self {
            id: DEFAULT_BRUSH_ID.to_owned(),
            preset: BrushPreset::default(),
            stamp_image: None,
        }
    }

    pub(crate) fn bundled_sketch() -> Self {
        Self {
            id: SKETCH_ID.to_owned(),
            preset: BrushPreset {
                name: "Sketch".to_owned(),
                size: SizeConfig {
                    default: 18.0,
                    min: 1.0,
                    max: 200.0,
                },
                spacing: SpacingConfig {
                    ratio: 0.08,
                    minimum: 1.0,
                },
                pressure: PressureConfig {
                    min_size: 0.25,
                    min_opacity: 0.01,
                    opacity_gamma: 2.4,
                },
                ..BrushPreset::default()
            },
            stamp_image: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrushSummary {
    pub(crate) id: String,
    pub(crate) name: String,
}

pub(crate) struct BrushCatalog {
    pub(crate) brushes: Vec<BrushSummary>,
    pub(crate) warnings: Vec<String>,
}

impl Default for BrushCatalog {
    fn default() -> Self {
        Self {
            brushes: vec![
                BrushSummary {
                    id: DEFAULT_BRUSH_ID.to_owned(),
                    name: BrushPreset::default().name,
                },
                BrushSummary {
                    id: SKETCH_ID.to_owned(),
                    name: "Sketch".to_owned(),
                },
            ],
            warnings: Vec::new(),
        }
    }
}

pub(super) fn load_user_brush(
    brushes_root: &Path,
    id: &str,
) -> Result<LoadedBrushPreset, ConfigError> {
    let (preset, stamp_path) = load_user_brush_metadata(brushes_root, id)?;
    let stamp_image = open_stamp_reader(&stamp_path)?
        .decode()
        .map_err(|error| {
            ConfigError::new(format!(
                "failed to decode brush stamp {}: {error}",
                stamp_path.display()
            ))
        })?
        .to_rgba8();

    Ok(LoadedBrushPreset {
        id: id.to_owned(),
        preset,
        stamp_image: Some(stamp_image),
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

    Ok(BrushSummary {
        id: id.to_owned(),
        name: preset.name,
    })
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

fn validate_stamp_path(stamp: &str) -> Result<(), ConfigError> {
    let path = Path::new(stamp);
    if stamp.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ConfigError::new(
            "stamp must be a relative path inside the brush directory",
        ));
    }
    Ok(())
}

fn validate_finite_positive(field: &str, value: f32) -> Result<(), ConfigError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ConfigError::new(format!(
            "{field} must be finite and greater than zero"
        )));
    }
    Ok(())
}

fn validate_finite_non_negative(field: &str, value: f32) -> Result<(), ConfigError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ConfigError::new(format!(
            "{field} must be finite and non-negative"
        )));
    }
    Ok(())
}

fn validate_finite_at_least(field: &str, value: f32, minimum: f32) -> Result<(), ConfigError> {
    if !value.is_finite() || value < minimum {
        return Err(ConfigError::new(format!(
            "{field} must be finite and at least {minimum}"
        )));
    }
    Ok(())
}

fn validate_unit(field: &str, value: f32) -> Result<(), ConfigError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(ConfigError::new(format!("{field} must be between 0 and 1")));
    }
    Ok(())
}
