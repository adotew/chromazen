use std::sync::mpsc::TrySendError;

use chromazen::{config::ensure_config_directory, protocol::UiCommand};
use tauri::{
    App, AppHandle, Manager, Wry,
    menu::{
        AboutMetadata, Menu, MenuBuilder, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
        SubmenuBuilder,
    },
};
use tauri_plugin_opener::OpenerExt;

use crate::ControlSender;

const UNDO_ID: &str = "chromazen.edit.undo";
const REDO_ID: &str = "chromazen.edit.redo";
const SAVE_SETTINGS_ID: &str = "chromazen.settings.save";
const RELOAD_CONFIGURATION_ID: &str = "chromazen.settings.reload";
const RESET_BRUSH_ID: &str = "chromazen.settings.reset-brush";
const OPEN_CONFIG_DIRECTORY_ID: &str = "chromazen.settings.open-config-directory";

#[derive(Clone)]
pub(crate) struct HistoryMenu {
    undo: MenuItem<Wry>,
    redo: MenuItem<Wry>,
}

impl HistoryMenu {
    pub(crate) fn set_enabled(&self, can_undo: bool, can_redo: bool) {
        if let Err(error) = self.undo.set_enabled(can_undo) {
            log::warn!("failed to update Undo menu item: {error}");
        }
        if let Err(error) = self.redo.set_enabled(can_redo) {
            log::warn!("failed to update Redo menu item: {error}");
        }
    }
}

pub(crate) struct NativeMenu {
    pub(crate) menu: Menu<Wry>,
    pub(crate) history: HistoryMenu,
}

impl NativeMenu {
    pub(crate) fn new(app: &App<Wry>) -> tauri::Result<Self> {
        let undo = MenuItem::with_id(app, UNDO_ID, "Undo", false, Some("CmdOrCtrl+Z"))?;
        let redo_accelerator = if cfg!(target_os = "macos") {
            "CmdOrCtrl+Shift+Z"
        } else {
            "CmdOrCtrl+Y"
        };
        let redo = MenuItem::with_id(app, REDO_ID, "Redo", false, Some(redo_accelerator))?;

        let mut menu = MenuBuilder::new(app);
        #[cfg(target_os = "macos")]
        {
            menu = menu.item(&application_menu(app)?);
        }
        menu = menu
            .item(&edit_menu(app, &undo, &redo)?)
            .item(&settings_menu(app)?)
            .item(&window_menu(app)?);

        Ok(Self {
            menu: menu.build()?,
            history: HistoryMenu { undo, redo },
        })
    }
}

pub(crate) fn handle_menu_event(app: &AppHandle<Wry>, event: MenuEvent) {
    let command = match event.id().as_ref() {
        UNDO_ID => Some(UiCommand::Undo),
        REDO_ID => Some(UiCommand::Redo),
        SAVE_SETTINGS_ID => Some(UiCommand::SaveSettings),
        RELOAD_CONFIGURATION_ID => Some(UiCommand::ReloadConfiguration),
        RESET_BRUSH_ID => Some(UiCommand::ResetBrush),
        OPEN_CONFIG_DIRECTORY_ID => {
            open_config_directory(app);
            None
        }
        _ => None,
    };
    if let Some(command) = command {
        enqueue_menu_command(app, command);
    }
}

fn enqueue_menu_command(app: &AppHandle<Wry>, command: UiCommand) {
    match app.state::<ControlSender>().0.try_send(command) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            log::warn!("control queue is full; menu command was ignored");
        }
        Err(TrySendError::Disconnected(_)) => {
            log::warn!("control queue is unavailable; menu command was ignored");
        }
    }
}

fn open_config_directory(app: &AppHandle<Wry>) {
    let path = match ensure_config_directory() {
        Ok(path) => path,
        Err(error) => {
            log::error!("failed to prepare configuration directory: {error}");
            return;
        }
    };
    if let Err(error) = app.opener().open_path(path.to_string_lossy(), None::<&str>) {
        log::error!("failed to open configuration directory: {error}");
    }
}

fn edit_menu(
    app: &App<Wry>,
    undo: &MenuItem<Wry>,
    redo: &MenuItem<Wry>,
) -> tauri::Result<Submenu<Wry>> {
    SubmenuBuilder::new(app, "Edit")
        .item(undo)
        .item(redo)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()
}

fn settings_menu(app: &App<Wry>) -> tauri::Result<Submenu<Wry>> {
    SubmenuBuilder::new(app, "Settings")
        .text(SAVE_SETTINGS_ID, "Save Settings")
        .text(RELOAD_CONFIGURATION_ID, "Reload Configuration")
        .text(RESET_BRUSH_ID, "Reset Brush to Defaults")
        .separator()
        .text(OPEN_CONFIG_DIRECTORY_ID, "Open Configuration Folder")
        .build()
}

fn window_menu(app: &App<Wry>) -> tauri::Result<Submenu<Wry>> {
    SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()
}

#[cfg(target_os = "macos")]
fn application_menu(app: &App<Wry>) -> tauri::Result<Submenu<Wry>> {
    let about = PredefinedMenuItem::about(
        app,
        Some("About Chromazen"),
        Some(AboutMetadata {
            name: Some("Chromazen".to_owned()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            ..AboutMetadata::default()
        }),
    )?;
    SubmenuBuilder::new(app, "Chromazen")
        .item(&about)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_are_stable() {
        assert_eq!(UNDO_ID, "chromazen.edit.undo");
        assert_eq!(REDO_ID, "chromazen.edit.redo");
        assert_eq!(
            OPEN_CONFIG_DIRECTORY_ID,
            "chromazen.settings.open-config-directory"
        );
    }
}
