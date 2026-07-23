use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use brush::{DEFAULT_BRUSH_ID, SKETCH_ID, discover_user_brushes, load_user_brush};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
mod brush;

pub use brush::{BrushCatalog, BrushSummary, LoadedBrushPreset};

const APP_NAME: &str = "chromazen";
const CONFIG_FILE_NAME: &str = "config.toml";
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u32,
    pub active_brush: String,
    pub brush: CurrentBrushConfig,
    pub smoothing: SmoothingConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_brush: DEFAULT_BRUSH_ID.to_owned(),
            brush: CurrentBrushConfig::default(),
            smoothing: SmoothingConfig::default(),
        }
    }
}

impl AppConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::new(format!(
                "unsupported schema_version {}; expected {CURRENT_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.active_brush.trim().is_empty() {
            return Err(ConfigError::new("active_brush must not be empty"));
        }
        self.brush.validate()?;
        self.smoothing.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmoothingConfig {
    pub strength: f32,
}

impl Default for SmoothingConfig {
    fn default() -> Self {
        Self { strength: 0.8 }
    }
}

impl SmoothingConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if !self.strength.is_finite() || self.strength <= 0.0 || self.strength > 1.0 {
            return Err(ConfigError::new(
                "smoothing.strength must be greater than 0 and at most 1",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CurrentBrushConfig {
    pub size: f32,
    pub color: [u8; 4],
}

impl Default for CurrentBrushConfig {
    fn default() -> Self {
        Self {
            size: 300.0,
            color: [170, 187, 204, 255],
        }
    }
}

impl CurrentBrushConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if !self.size.is_finite() {
            return Err(ConfigError::new("brush.size must be finite"));
        }
        if self.size <= 0.0 {
            return Err(ConfigError::new("brush.size must be greater than zero"));
        }
        if self.color[3] != 255 {
            return Err(ConfigError::new(
                "brush.color alpha must be 255 because translucent brush colors are not supported",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConfigStore {
    root: PathBuf,
}

impl ConfigStore {
    pub(crate) fn discover() -> Result<Self, ConfigError> {
        let project_dirs = ProjectDirs::from("", "", APP_NAME).ok_or_else(|| {
            ConfigError::new("could not determine the user configuration directory")
        })?;
        Ok(Self::from_root(project_dirs.config_dir()))
    }

    fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE_NAME)
    }

    pub(crate) fn brushes_path(&self) -> PathBuf {
        self.root.join("brushes")
    }

    pub(crate) fn open_config_directory(&self) -> Result<(), ConfigError> {
        fs::create_dir_all(&self.root).map_err(|error| {
            ConfigError::io("create configuration directory for", &self.root, error)
        })?;
        open::that_detached(&self.root).map_err(|error| ConfigError::io("open", &self.root, error))
    }

    pub(crate) fn load_brush(&self, id: &str) -> Result<LoadedBrushPreset, ConfigError> {
        let config_path = self.brushes_path().join(id).join("brush.toml");
        if !config_path.exists() {
            match id {
                DEFAULT_BRUSH_ID => return Ok(LoadedBrushPreset::bundled_charcoal()),
                SKETCH_ID => return Ok(LoadedBrushPreset::bundled_sketch()),
                _ => {}
            }
        }
        load_user_brush(&self.brushes_path(), id)
    }

    pub(crate) fn discover_brushes(&self) -> BrushCatalog {
        discover_user_brushes(&self.brushes_path())
    }

    pub(crate) fn load_app_config(&self) -> Result<AppConfig, ConfigError> {
        let path = self.config_path();
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppConfig::default());
            }
            Err(error) => return Err(ConfigError::io("read", &path, error)),
        };

        let config: AppConfig = toml::from_str(&source).map_err(|error| {
            ConfigError::new(format!("failed to parse {}: {error}", path.display()))
        })?;
        config.validate().map_err(|error| {
            ConfigError::new(format!(
                "invalid configuration in {}: {error}",
                path.display()
            ))
        })?;
        Ok(config)
    }

    pub(crate) fn save_app_config(&self, config: &AppConfig) -> Result<(), ConfigError> {
        config.validate()?;

        fs::create_dir_all(&self.root).map_err(|error| {
            ConfigError::io("create configuration directory for", &self.root, error)
        })?;

        let serialized = toml::to_string_pretty(config)
            .map_err(|error| ConfigError::new(format!("failed to serialize settings: {error}")))?;
        let contents = format!(
            "# Chromazen settings. This file may be rewritten by the application.\n\n{serialized}"
        );
        atomic_write(&self.config_path(), contents.as_bytes())
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|error| ConfigError::io("open for atomic writing", path, error))?;
    file.write_all(contents)
        .map_err(|error| ConfigError::io("write", path, error))?;
    file.flush()
        .map_err(|error| ConfigError::io("flush", path, error))?;
    file.commit()
        .map_err(|error| ConfigError::io("commit", path, error))?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct ConfigError {
    message: String,
}

impl ConfigError {
    pub(crate) fn unavailable() -> Self {
        Self::new("the configuration directory is unavailable")
    }

    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(operation: &str, path: &Path, error: std::io::Error) -> Self {
        Self::new(format!("failed to {operation} {}: {error}", path.display()))
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_uses_defaults() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());

        assert_eq!(
            store.load_app_config().expect("defaults"),
            AppConfig::default()
        );
    }

    #[test]
    fn partial_config_fills_missing_fields_with_defaults() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        fs::write(store.config_path(), "[brush]\nsize = 425.0\n").expect("write config");

        let config = store.load_app_config().expect("load config");

        assert_eq!(config.brush.size, 425.0);
        assert_eq!(config.brush.color, CurrentBrushConfig::default().color);
        assert_eq!(config.active_brush, "charcoal");
        assert_eq!(config.smoothing, SmoothingConfig::default());
    }

    #[test]
    fn valid_config_round_trips_through_atomic_save() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path().join("nested"));
        let mut config = AppConfig::default();
        config.brush.size = 512.0;
        config.brush.color = [1, 2, 3, 255];

        store.save_app_config(&config).expect("save config");

        assert_eq!(store.load_app_config().expect("load config"), config);
    }

    #[test]
    fn rejected_save_preserves_previous_config() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        let valid = AppConfig::default();
        store.save_app_config(&valid).expect("save valid config");
        let previous = fs::read_to_string(store.config_path()).expect("read valid config");
        let mut invalid = valid;
        invalid.brush.size = -1.0;

        assert!(store.save_app_config(&invalid).is_err());
        assert_eq!(
            fs::read_to_string(store.config_path()).expect("read preserved config"),
            previous
        );
    }

    #[test]
    fn malformed_config_is_reported_and_preserved() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        let malformed = "[brush\nsize = ???\n";
        fs::write(store.config_path(), malformed).expect("write config");

        assert!(store.load_app_config().is_err());
        assert_eq!(
            fs::read_to_string(store.config_path()).expect("read config"),
            malformed
        );
    }

    #[test]
    fn unsupported_app_schema_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        fs::write(store.config_path(), "schema_version = 2\n").expect("write config");

        let error = store.load_app_config().expect_err("future schema");

        assert!(error.to_string().contains("unsupported schema_version 2"));
    }

    #[test]
    fn invalid_smoothing_strength_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        for strength in [0.0, 1.5] {
            fs::write(
                store.config_path(),
                format!("[smoothing]\nstrength = {strength}\n"),
            )
            .expect("write config");

            let error = store.load_app_config().expect_err("invalid config");
            assert!(error.to_string().contains("smoothing.strength"));
        }
    }

    #[test]
    fn invalid_brush_size_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        fs::write(store.config_path(), "[brush]\nsize = -1.0\n").expect("write config");

        let error = store.load_app_config().expect_err("invalid config");

        assert!(error.to_string().contains("brush.size"));
    }

    #[test]
    fn unknown_fields_are_reported() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        fs::write(store.config_path(), "unknown_setting = true\n").expect("write config");

        assert!(store.load_app_config().is_err());
    }

    #[test]
    fn bundled_brushes_are_available_without_user_files() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());

        let charcoal = store.load_brush("charcoal").expect("charcoal brush");
        let sketch = store.load_brush("sketch").expect("sketch brush");

        assert_eq!(charcoal.id, "charcoal");
        assert_eq!(sketch.id, "sketch");
        assert_eq!(sketch.preset.size.default, 18.0);
        assert_eq!(sketch.preset.spacing.ratio, 0.08);
        assert_eq!(sketch.preset.pressure.min_size, 0.25);
        assert_eq!(sketch.preset.pressure.min_opacity, 0.01);
        assert_eq!(sketch.preset.pressure.opacity_gamma, 2.4);
        assert!(charcoal.stamp_image.is_none());
        assert!(sketch.stamp_image.is_none());
    }

    #[test]
    fn subpixel_minimum_brush_spacing_is_rejected() {
        let mut preset = brush::BrushPreset::default();
        preset.spacing.minimum = 0.5;

        let error = preset.validate().expect_err("subpixel spacing");

        assert!(error.to_string().contains("spacing.minimum"));
    }

    #[test]
    fn unsupported_brush_schema_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        write_test_brush(
            &store,
            "future",
            "schema_version = 2\nname = \"Future\"\nstamp = \"tip.png\"\n",
        );

        let error = store.load_brush("future").expect_err("future schema");

        assert!(
            error
                .to_string()
                .contains("unsupported brush schema_version 2")
        );
    }

    #[test]
    fn user_brush_loads_stamp_relative_to_preset() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        write_test_brush(&store, "pencil", "name = \"Pencil\"\nstamp = \"tip.png\"\n");

        let brush = store.load_brush("pencil").expect("user brush");

        assert_eq!(brush.preset.name, "Pencil");
        assert_eq!(brush.stamp_image.expect("stamp").dimensions(), (2, 3));
    }

    #[test]
    fn oversized_stamp_is_rejected_during_metadata_inspection() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        write_test_brush(
            &store,
            "oversized",
            "name = \"Oversized\"\nstamp = \"tip.png\"\n",
        );
        image::RgbaImage::from_pixel(4097, 1, image::Rgba([0, 0, 0, 255]))
            .save(store.brushes_path().join("oversized/tip.png"))
            .expect("oversized stamp");

        let catalog = store.discover_brushes();

        assert!(!catalog.brushes.iter().any(|brush| brush.id == "oversized"));
        assert_eq!(catalog.warnings.len(), 1);
        assert!(store.load_brush("oversized").is_err());
    }

    #[test]
    fn stamp_paths_cannot_escape_brush_directory() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        let brush_dir = store.brushes_path().join("unsafe");
        fs::create_dir_all(&brush_dir).expect("brush directory");
        fs::write(
            brush_dir.join("brush.toml"),
            "name = \"Unsafe\"\nstamp = \"../outside.png\"\n",
        )
        .expect("brush config");

        assert!(store.load_brush("unsafe").is_err());
    }

    #[test]
    fn malformed_brush_does_not_hide_valid_brushes() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        write_test_brush(&store, "pencil", "name = \"Pencil\"\nstamp = \"tip.png\"\n");
        let broken_dir = store.brushes_path().join("broken");
        fs::create_dir_all(&broken_dir).expect("broken brush directory");
        fs::write(broken_dir.join("brush.toml"), "not valid = [").expect("broken config");

        let catalog = store.discover_brushes();

        assert!(catalog.brushes.iter().any(|brush| brush.id == "pencil"));
        assert_eq!(catalog.warnings.len(), 1);
    }

    fn write_test_brush(store: &ConfigStore, id: &str, config: &str) {
        let brush_dir = store.brushes_path().join(id);
        fs::create_dir_all(&brush_dir).expect("brush directory");
        fs::write(brush_dir.join("brush.toml"), config).expect("brush config");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 0, 255]))
            .save(brush_dir.join("tip.png"))
            .expect("stamp image");
    }
}
