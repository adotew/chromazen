use crate::{
    config::{
        AppConfig, BrushCatalog, ConfigError, ConfigStore, CurrentBrushConfig, LoadedBrushPreset,
    },
    paint::PaintTool,
};

pub(super) enum SettingsCommand {
    Save {
        brush: CurrentBrushConfig,
        tool_brushes: [String; 3],
        tool_sizes: [f32; 3],
        tool_opacities: [f32; 3],
    },
    SwitchBrush {
        tool: PaintTool,
        id: String,
        reset_size: bool,
    },
    ReloadFromDisk(PaintTool),
    OpenConfigDirectory,
}

pub(super) enum SettingsEffect {
    Success(String),
    Error(String),
}

pub(super) struct PendingBrushChange {
    pub(super) brush: LoadedBrushPreset,
    pub(super) tool: PaintTool,
    pub(super) reset_size: bool,
    reloaded_config: Option<AppConfig>,
    warning: Option<String>,
}

impl PendingBrushChange {
    fn switch(tool: PaintTool, brush: LoadedBrushPreset, reset_size: bool) -> Self {
        Self {
            brush,
            tool,
            reset_size,
            reloaded_config: None,
            warning: None,
        }
    }

    fn reload(
        mut config: AppConfig,
        tool: PaintTool,
        brush: LoadedBrushPreset,
        warning: Option<String>,
    ) -> Self {
        config.set_brush_for_tool(tool, brush.id.clone());
        Self {
            brush,
            tool,
            reset_size: false,
            reloaded_config: Some(config),
            warning,
        }
    }
}

pub(super) struct CompletedBrushChange {
    pub(super) catalog: BrushCatalog,
    pub(super) reloaded: bool,
    pub(super) warnings: Vec<String>,
}

pub(super) struct SettingsController {
    store: Option<ConfigStore>,
    config: AppConfig,
    active_brush: LoadedBrushPreset,
    startup_catalog: Option<BrushCatalog>,
    pending_brush_change: Option<PendingBrushChange>,
    startup_error: Option<String>,
}

impl SettingsController {
    pub(super) fn load() -> Self {
        let (store, mut config, mut startup_error) = match ConfigStore::discover() {
            Ok(store) => match store.load_app_config() {
                Ok(config) => (Some(store), config, None),
                Err(error) => {
                    log::error!("failed to load settings: {error}");
                    (Some(store), AppConfig::default(), Some(error.to_string()))
                }
            },
            Err(error) => {
                log::error!("failed to locate settings: {error}");
                (None, AppConfig::default(), Some(error.to_string()))
            }
        };

        let catalog = store
            .as_ref()
            .map_or_else(BrushCatalog::default, ConfigStore::discover_brushes);
        for warning in &catalog.warnings {
            log::warn!("failed to discover brush: {warning}");
        }
        if !catalog.warnings.is_empty() {
            append_message(
                &mut startup_error,
                format!(
                    "Some brush presets could not be loaded:\n{}",
                    catalog.warnings.join("\n")
                ),
            );
        }
        log::debug!("discovered {} brush preset(s)", catalog.brushes.len());

        let active_brush = if let Some(store) = &store {
            match store.load_brush(&config.active_brush) {
                Ok(brush) => brush,
                Err(error) => {
                    log::error!("failed to load brush preset: {error}");
                    append_message(
                        &mut startup_error,
                        format!("Could not load brush '{}': {error}", config.active_brush),
                    );
                    LoadedBrushPreset::bundled_charcoal()
                }
            }
        } else {
            LoadedBrushPreset::bundled_charcoal()
        };
        normalize_active_brush(&mut config, &active_brush);

        Self {
            store,
            config,
            active_brush,
            startup_catalog: Some(catalog),
            pending_brush_change: None,
            startup_error,
        }
    }

    pub(super) fn config(&self) -> &AppConfig {
        &self.config
    }

    pub(super) fn active_brush(&self) -> &LoadedBrushPreset {
        &self.active_brush
    }

    pub(super) fn take_startup_catalog(&mut self) -> BrushCatalog {
        self.startup_catalog.take().unwrap_or_default()
    }

    pub(super) fn take_startup_error(&mut self) -> Option<String> {
        self.startup_error.take()
    }

    pub(super) fn handle_command(&mut self, command: SettingsCommand) -> Option<SettingsEffect> {
        match command {
            SettingsCommand::Save {
                brush,
                tool_brushes,
                tool_sizes,
                tool_opacities,
            } => {
                self.config.brush = brush;
                self.config.brush.size = tool_sizes[0];
                self.config.eraser_size = tool_sizes[1];
                self.config.smudge_size = tool_sizes[2];
                self.config.brush.opacity = tool_opacities[0];
                self.config.eraser_opacity = tool_opacities[1];
                self.config.smudge_opacity = tool_opacities[2];
                self.config.active_brush = tool_brushes[0].clone();
                self.config.eraser_brush = tool_brushes[1].clone();
                self.config.smudge_brush = tool_brushes[2].clone();
                let Some(store) = &self.store else {
                    return Some(SettingsEffect::Error(
                        "The configuration directory is unavailable".to_owned(),
                    ));
                };
                match store.save_app_config(&self.config) {
                    Ok(()) => None,
                    Err(error) => Some(command_error("failed to save settings", error)),
                }
            }
            SettingsCommand::SwitchBrush {
                tool,
                id,
                reset_size,
            } => {
                let result = self.store().and_then(|store| store.load_brush(&id));
                match result {
                    Ok(brush) => {
                        self.pending_brush_change =
                            Some(PendingBrushChange::switch(tool, brush, reset_size));
                        None
                    }
                    Err(error) => Some(command_error("brush preset action failed", error)),
                }
            }
            SettingsCommand::ReloadFromDisk(tool) => {
                let result = self.store().and_then(|store| {
                    store.load_app_config().map(|config| {
                        let selected = config.brush_for_tool(tool);
                        let (brush, warning) = match store.load_brush(selected) {
                            Ok(brush) => (brush, None),
                            Err(error) => {
                                let warning = format!(
                                    "Could not reload brush '{}': {error}. Using bundled Charcoal instead.",
                                    selected
                                );
                                (LoadedBrushPreset::bundled_charcoal(), Some(warning))
                            }
                        };
                        PendingBrushChange::reload(config, tool, brush, warning)
                    })
                });
                match result {
                    Ok(change) => {
                        self.pending_brush_change = Some(change);
                        None
                    }
                    Err(error) => Some(command_error("brush preset action failed", error)),
                }
            }
            SettingsCommand::OpenConfigDirectory => {
                let result = self.store().and_then(ConfigStore::open_config_directory);
                Some(match result {
                    Ok(()) => SettingsEffect::Success("Opened the configuration folder".to_owned()),
                    Err(error) => command_error("brush preset action failed", error),
                })
            }
        }
    }

    pub(super) fn take_pending_brush_change(&mut self) -> Option<PendingBrushChange> {
        self.pending_brush_change.take()
    }

    pub(super) fn restore_pending_brush_change(&mut self, change: PendingBrushChange) {
        self.pending_brush_change = Some(change);
    }

    pub(super) fn complete_brush_change(
        &mut self,
        change: PendingBrushChange,
    ) -> CompletedBrushChange {
        let PendingBrushChange {
            brush,
            tool,
            reset_size: _,
            reloaded_config,
            warning,
        } = change;
        let catalog = self
            .store
            .as_ref()
            .map_or_else(BrushCatalog::default, ConfigStore::discover_brushes);
        let mut warnings = catalog.warnings.clone();
        if let Some(warning) = warning {
            log::warn!("{warning}");
            warnings.insert(0, warning);
        }
        for warning in &catalog.warnings {
            log::warn!("failed to discover brush: {warning}");
        }

        let reloaded = reloaded_config.is_some();
        if let Some(config) = reloaded_config {
            self.config = config;
        } else {
            self.config.set_brush_for_tool(tool, brush.id.clone());
        }
        self.active_brush = brush;

        CompletedBrushChange {
            catalog,
            reloaded,
            warnings,
        }
    }

    fn store(&self) -> Result<&ConfigStore, ConfigError> {
        self.store.as_ref().ok_or_else(ConfigError::unavailable)
    }
}

fn command_error(context: &str, error: ConfigError) -> SettingsEffect {
    log::error!("{context}: {error}");
    SettingsEffect::Error(error.to_string())
}

fn normalize_active_brush(config: &mut AppConfig, brush: &LoadedBrushPreset) {
    config.active_brush.clone_from(&brush.id);
}

fn append_message(existing: &mut Option<String>, message: String) {
    *existing = Some(match existing.take() {
        Some(existing) => format!("{existing}\n{message}"),
        None => message,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_normalizes_missing_brush_to_effective_fallback() {
        let config = AppConfig {
            active_brush: "missing".to_owned(),
            ..AppConfig::default()
        };

        let change = PendingBrushChange::reload(
            config,
            PaintTool::Brush,
            LoadedBrushPreset::bundled_charcoal(),
            Some("missing brush".to_owned()),
        );
        let brush_id = change.brush.id.clone();

        assert_eq!(
            change
                .reloaded_config
                .expect("reloaded config")
                .active_brush,
            brush_id
        );
    }
}
