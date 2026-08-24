use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use brush::{
    BRISTLE_ID, DEFAULT_BRUSH_ID, RECTANGLE_ID, ROUNDED_ID, SKETCH_ID, discover_user_brushes,
    load_user_brush,
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::paint::PaintTool;
mod abr;
mod brush;
mod brush_import;

pub(crate) use brush::{BrushCatalog, BrushSummary, LoadedBrushPreset};

const APP_NAME: &str = "Chromazen";
const CONFIG_FILE_NAME: &str = "config.toml";
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub(crate) schema_version: u32,
    /// Brush-tool preset. Kept under its original name for config compatibility.
    pub(crate) active_brush: String,
    pub(crate) eraser_brush: String,
    pub(crate) smudge_brush: String,
    pub(crate) eraser_size: f32,
    pub(crate) smudge_size: f32,
    pub(crate) eraser_opacity: f32,
    pub(crate) smudge_opacity: f32,
    pub(crate) brush: CurrentBrushConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_brush: DEFAULT_BRUSH_ID.to_owned(),
            eraser_brush: DEFAULT_BRUSH_ID.to_owned(),
            smudge_brush: DEFAULT_BRUSH_ID.to_owned(),
            eraser_size: CurrentBrushConfig::default().size,
            smudge_size: CurrentBrushConfig::default().size,
            eraser_opacity: CurrentBrushConfig::default().opacity,
            smudge_opacity: CurrentBrushConfig::default().opacity,
            brush: CurrentBrushConfig::default(),
        }
    }
}

impl AppConfig {
    pub(crate) fn brush_for_tool(&self, tool: PaintTool) -> &str {
        match tool {
            PaintTool::Brush => &self.active_brush,
            PaintTool::Eraser => &self.eraser_brush,
            PaintTool::Smudge => &self.smudge_brush,
        }
    }

    pub(crate) fn set_brush_for_tool(&mut self, tool: PaintTool, id: String) {
        match tool {
            PaintTool::Brush => self.active_brush = id,
            PaintTool::Eraser => self.eraser_brush = id,
            PaintTool::Smudge => self.smudge_brush = id,
        }
    }

    pub(crate) fn size_for_tool(&self, tool: PaintTool) -> f32 {
        match tool {
            PaintTool::Brush => self.brush.size,
            PaintTool::Eraser => self.eraser_size,
            PaintTool::Smudge => self.smudge_size,
        }
    }

    pub(crate) fn opacity_for_tool(&self, tool: PaintTool) -> f32 {
        match tool {
            PaintTool::Brush => self.brush.opacity,
            PaintTool::Eraser => self.eraser_opacity,
            PaintTool::Smudge => self.smudge_opacity,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::new(format!(
                "unsupported schema_version {}; expected {CURRENT_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        for (name, id) in [
            ("active_brush", &self.active_brush),
            ("eraser_brush", &self.eraser_brush),
            ("smudge_brush", &self.smudge_brush),
        ] {
            if id.trim().is_empty() {
                return Err(ConfigError::new(format!("{name} must not be empty")));
            }
        }
        self.brush.validate()?;
        for (name, size) in [
            ("eraser_size", self.eraser_size),
            ("smudge_size", self.smudge_size),
        ] {
            if !size.is_finite() || size <= 0.0 {
                return Err(ConfigError::new(format!(
                    "{name} must be finite and greater than zero"
                )));
            }
        }
        for (name, opacity) in [
            ("eraser_opacity", self.eraser_opacity),
            ("smudge_opacity", self.smudge_opacity),
        ] {
            validate_opacity(name, opacity)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CurrentBrushConfig {
    pub(crate) size: f32,
    pub(crate) opacity: f32,
    pub(crate) color: [u8; 4],
}

impl Default for CurrentBrushConfig {
    fn default() -> Self {
        Self {
            size: 300.0,
            opacity: 1.0,
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
        validate_opacity("brush.opacity", self.opacity)?;
        if self.color[3] != 255 {
            return Err(ConfigError::new(
                "brush.color alpha must be 255 because translucent brush colors are not supported",
            ));
        }
        Ok(())
    }
}

fn validate_opacity(name: &str, opacity: f32) -> Result<(), ConfigError> {
    if !opacity.is_finite() || !(0.01..=1.0).contains(&opacity) {
        return Err(ConfigError::new(format!(
            "{name} must be finite and between 0.01 and 1"
        )));
    }
    Ok(())
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
                ROUNDED_ID => return Ok(LoadedBrushPreset::bundled_rounded()),
                RECTANGLE_ID => return Ok(LoadedBrushPreset::bundled_rectangle()),
                BRISTLE_ID => return Ok(LoadedBrushPreset::bundled_bristle()),
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
        write_file_atomically(&self.config_path(), contents.as_bytes())
    }
}

fn write_file_atomically(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
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
        assert_eq!(config.brush.opacity, 1.0);
        assert_eq!(config.eraser_opacity, 1.0);
        assert_eq!(config.smudge_opacity, 1.0);
        assert_eq!(config.brush.color, CurrentBrushConfig::default().color);
        assert_eq!(config.active_brush, "charcoal");
    }

    #[test]
    fn tool_brushes_are_selected_independently() {
        let mut config = AppConfig::default();
        config.set_brush_for_tool(PaintTool::Eraser, "hard-round".to_owned());
        config.set_brush_for_tool(PaintTool::Smudge, "soft-blend".to_owned());

        assert_eq!(config.brush_for_tool(PaintTool::Brush), "charcoal");
        assert_eq!(config.brush_for_tool(PaintTool::Eraser), "hard-round");
        assert_eq!(config.brush_for_tool(PaintTool::Smudge), "soft-blend");
    }

    #[test]
    fn valid_config_round_trips_through_atomic_save() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path().join("nested"));
        let mut config = AppConfig::default();
        config.brush.size = 512.0;
        config.brush.opacity = 0.4;
        config.eraser_opacity = 0.6;
        config.smudge_opacity = 0.8;
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
    fn invalid_brush_size_is_rejected() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        fs::write(store.config_path(), "[brush]\nsize = -1.0\n").expect("write config");

        let error = store.load_app_config().expect_err("invalid config");

        assert!(error.to_string().contains("brush.size"));
    }

    #[test]
    fn invalid_tool_opacity_is_rejected() {
        for source in [
            "eraser_opacity = 0.0\n",
            "smudge_opacity = 1.1\n",
            "[brush]\nopacity = 0.0\n",
        ] {
            let config: AppConfig = toml::from_str(source).expect("parse config");
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn tool_opacities_are_selected_independently() {
        let config = AppConfig {
            eraser_opacity: 0.5,
            smudge_opacity: 0.25,
            ..AppConfig::default()
        };

        assert_eq!(config.opacity_for_tool(PaintTool::Brush), 1.0);
        assert_eq!(config.opacity_for_tool(PaintTool::Eraser), 0.5);
        assert_eq!(config.opacity_for_tool(PaintTool::Smudge), 0.25);
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
        let rounded = store.load_brush("rounded").expect("rounded brush");
        let rectangle = store.load_brush("rectangle").expect("rectangle brush");
        let bristle = store.load_brush("bristle").expect("bristle brush");

        assert_eq!(charcoal.id, "charcoal");
        assert_eq!(charcoal.preset.spacing.ratio, 0.03);
        assert_eq!(sketch.id, "sketch");
        assert_eq!(sketch.preset.size.default, 18.0);
        assert_eq!(sketch.preset.spacing.ratio, 0.08);
        assert_eq!(sketch.preset.pressure.min_size, 0.25);
        assert_eq!(sketch.preset.pressure.min_opacity, 0.01);
        assert_eq!(sketch.preset.pressure.full_opacity_pressure, 0.8);
        assert_eq!(sketch.preset.pressure.opacity_gamma, 4.0);
        assert_eq!(rounded.preset.spacing.minimum, 0.5);
        assert_eq!(rectangle.preset.spacing.minimum, 0.5);
        assert_eq!(
            rounded.stamp_image.as_ref().unwrap().dimensions(),
            (128, 128)
        );
        assert_eq!(
            rectangle.stamp_image.as_ref().unwrap().dimensions(),
            (192, 96)
        );
        assert_eq!(
            bristle.stamp_image.as_ref().unwrap().dimensions(),
            (982, 561)
        );
        assert_eq!(rounded.stamp_image.as_ref().unwrap().get_pixel(0, 0)[3], 0);
        assert_eq!(
            rectangle.stamp_image.as_ref().unwrap().get_pixel(0, 0)[3],
            255
        );
        assert!(charcoal.stamp_image.is_none());
        assert!(sketch.stamp_image.is_none());
        assert_eq!(
            store
                .discover_brushes()
                .brushes
                .into_iter()
                .map(|brush| brush.id)
                .collect::<Vec<_>>(),
            ["charcoal", "sketch", "rounded", "rectangle", "bristle"]
        );
    }

    #[test]
    fn subpixel_minimum_brush_spacing_has_a_safe_floor() {
        let mut preset = brush::BrushPreset::default();
        preset.spacing.minimum = 0.5;
        preset.validate().expect("half-pixel spacing");

        preset.spacing.minimum = 0.24;
        let error = preset.validate().expect_err("spacing below safe floor");
        assert!(error.to_string().contains("spacing.minimum"));
    }

    #[test]
    fn full_opacity_pressure_must_be_positive_and_at_most_one() {
        for pressure in [0.0, 1.1] {
            let mut preset = brush::BrushPreset::default();
            preset.pressure.full_opacity_pressure = pressure;

            let error = preset.validate().expect_err("invalid pressure threshold");

            assert!(error.to_string().contains("pressure.full_opacity_pressure"));
        }
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
    fn discovery_keeps_brush_preview_metadata() {
        let temp = tempfile::tempdir().expect("temp directory");
        let store = ConfigStore::from_root(temp.path());
        write_test_brush(
            &store,
            "pencil",
            "name = \"Pencil\"\nstamp = \"tip.png\"\n[spacing]\nratio = 0.4\n",
        );

        let catalog = store.discover_brushes();
        let pencil = catalog
            .brushes
            .iter()
            .find(|brush| brush.id == "pencil")
            .expect("pencil summary");

        assert_eq!(pencil.preview.spacing.ratio, 0.4);
        assert_eq!(
            pencil.preview.stamp_path,
            Some(
                fs::canonicalize(store.brushes_path().join("pencil/tip.png"))
                    .expect("canonical stamp path")
            )
        );
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
