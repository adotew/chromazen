use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use brush::{discover_user_brushes, load_user_brush};
use chromazen_brush::{BrushError, DEFAULT_BRUSH_ID};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::paint::{ColorMixing, PaintTool, PressureCurve};
mod brush;
mod brush_import;

pub(crate) use brush::{BrushCatalog, BrushSummary};
pub(crate) use chromazen_brush::LoadedBrushPreset;

const APP_NAME: &str = "Chromazen";
const DATA_DIR_ENV: &str = "CHROMAZEN_DATA_DIR";
const CONFIG_FILE_NAME: &str = "config.toml";
const CURRENT_SCHEMA_VERSION: u32 = 1;
/// White stands for the monochrome accent, which is drawn black in light mode.
pub(crate) const DEFAULT_ACCENT_COLOR: [u8; 3] = [255, 255, 255];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub(crate) schema_version: u32,
    pub(crate) accent_color: [u8; 3],
    /// Brush-tool preset. Kept under its original name for config compatibility.
    pub(crate) active_brush: String,
    pub(crate) eraser_brush: String,
    pub(crate) smudge_brush: String,
    pub(crate) eraser_size: f32,
    pub(crate) smudge_size: f32,
    pub(crate) eraser_opacity: f32,
    pub(crate) smudge_opacity: f32,
    pub(crate) brush: CurrentBrushConfig,
    pub(crate) panel_layout: PanelLayout,
    pub(crate) surface_style: SurfaceStyle,
    pub(crate) workspace_background: WorkspaceBackground,
    pub(crate) pressure_curve: PressureCurve,
    pub(crate) color_mixing: ColorMixing,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            accent_color: DEFAULT_ACCENT_COLOR,
            active_brush: DEFAULT_BRUSH_ID.to_owned(),
            eraser_brush: DEFAULT_BRUSH_ID.to_owned(),
            smudge_brush: DEFAULT_BRUSH_ID.to_owned(),
            eraser_size: CurrentBrushConfig::default().size,
            smudge_size: CurrentBrushConfig::default().size,
            eraser_opacity: CurrentBrushConfig::default().opacity,
            smudge_opacity: CurrentBrushConfig::default().opacity,
            brush: CurrentBrushConfig::default(),
            panel_layout: PanelLayout::default(),
            surface_style: SurfaceStyle::default(),
            workspace_background: WorkspaceBackground::default(),
            pressure_curve: PressureCurve::default(),
            color_mixing: ColorMixing::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SurfaceStyle {
    #[default]
    Opaque,
    Frosted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceBackground {
    #[default]
    Standard,
    NeutralGray,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PanelLayout {
    /// Top-left corner of the floating Brush panel, in points.
    pub(crate) brush_panel_pos: [f32; 2],
    /// Top-left corner of the floating Color panel, in points.
    pub(crate) color_panel_pos: [f32; 2],
    /// Top-left corner of the floating Layers panel, in points.
    pub(crate) layers_panel_pos: [f32; 2],
    /// Content width of the floating Color panel, in points.
    pub(crate) color_panel_width: f32,
    /// Content width of the floating Brush panel, in points.
    pub(crate) brush_panel_width: f32,
    /// Content size of the floating Layers panel, in points.
    pub(crate) layers_panel_size: [f32; 2],
}

pub(crate) const DEFAULT_BRUSH_PANEL_POS: [f32; 2] = [1016.0, 80.0];
pub(crate) const DEFAULT_COLOR_PANEL_POS: [f32; 2] = [24.0, 80.0];
pub(crate) const DEFAULT_LAYERS_PANEL_POS: [f32; 2] = [340.0, 80.0];
pub(crate) const DEFAULT_COLOR_PANEL_WIDTH: f32 = 280.0;
pub(crate) const DEFAULT_BRUSH_PANEL_WIDTH: f32 = 240.0;
pub(crate) const DEFAULT_LAYERS_PANEL_SIZE: [f32; 2] = [300.0, 360.0];

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            brush_panel_pos: DEFAULT_BRUSH_PANEL_POS,
            color_panel_pos: DEFAULT_COLOR_PANEL_POS,
            layers_panel_pos: DEFAULT_LAYERS_PANEL_POS,
            color_panel_width: DEFAULT_COLOR_PANEL_WIDTH,
            brush_panel_width: DEFAULT_BRUSH_PANEL_WIDTH,
            layers_panel_size: DEFAULT_LAYERS_PANEL_SIZE,
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
        self.pressure_curve.validate().map_err(ConfigError::new)
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

pub(crate) fn data_dir_override() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

impl ConfigStore {
    pub(crate) fn discover() -> Result<Self, ConfigError> {
        if let Some(root) = data_dir_override() {
            return Ok(Self::from_root(root.join("config")));
        }
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
        if !config_path.exists()
            && let Some(brush) = LoadedBrushPreset::bundled(id)
        {
            return Ok(brush);
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

impl From<BrushError> for ConfigError {
    fn from(error: BrushError) -> Self {
        Self::new(error.to_string())
    }
}

#[cfg(test)]
mod tests;
