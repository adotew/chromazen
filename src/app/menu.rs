#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use muda::{
        Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
        accelerator::{Accelerator, CMD_OR_CTRL, Code},
    };
    use winit::window::Window;

    #[cfg(target_os = "macos")]
    use muda::accelerator::Modifiers;

    #[cfg(target_os = "windows")]
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[cfg(target_os = "macos")]
    use muda::AboutMetadata;

    use super::super::command::AppCommand;

    const UNDO_ID: &str = "chromazen.edit.undo";
    const REDO_ID: &str = "chromazen.edit.redo";
    const SAVE_SETTINGS_ID: &str = "chromazen.settings.save";
    const RELOAD_CONFIGURATION_ID: &str = "chromazen.settings.reload";
    const RESET_BRUSH_ID: &str = "chromazen.settings.reset-brush";
    const OPEN_CONFIG_DIRECTORY_ID: &str = "chromazen.settings.open-config-directory";

    pub(crate) struct NativeMenu {
        menu: Menu,
        undo: MenuItem,
        redo: MenuItem,
        installed: bool,
    }

    impl NativeMenu {
        pub(crate) fn new() -> Result<Self, String> {
            let menu = Menu::new();

            #[cfg(target_os = "macos")]
            menu.append(&application_menu()?)
                .map_err(|error| format!("failed to add application menu: {error}"))?;

            let (edit_menu, undo, redo) = edit_menu()?;
            menu.append(&edit_menu)
                .map_err(|error| format!("failed to add edit menu: {error}"))?;
            menu.append(&settings_menu()?)
                .map_err(|error| format!("failed to add settings menu: {error}"))?;

            Ok(Self {
                menu,
                undo,
                redo,
                installed: false,
            })
        }

        pub(crate) fn set_event_handler<F>(&self, handler: F)
        where
            F: Fn(AppCommand) + Send + Sync + 'static,
        {
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(command) = command_for_id(event.id()) {
                    handler(command);
                }
            }));
        }

        pub(crate) fn set_history_enabled(&self, can_undo: bool, can_redo: bool) {
            self.undo.set_enabled(can_undo);
            self.redo.set_enabled(can_redo);
        }

        pub(crate) fn install(&mut self, _window: &Window) -> Result<(), String> {
            if self.installed {
                return Ok(());
            }

            #[cfg(target_os = "macos")]
            self.menu.init_for_nsapp();

            #[cfg(target_os = "windows")]
            {
                let window_handle = _window
                    .window_handle()
                    .map_err(|error| format!("failed to get window handle: {error}"))?;
                let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
                    return Err("expected a Win32 window handle on Windows".to_owned());
                };
                unsafe { self.menu.init_for_hwnd(handle.hwnd.get()) }
                    .map_err(|error| format!("failed to install Windows menu: {error}"))?;
            }

            self.installed = true;
            Ok(())
        }
    }

    fn edit_menu() -> Result<(Submenu, MenuItem, MenuItem), String> {
        let undo = MenuItem::with_id(
            UNDO_ID,
            "Undo",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyZ)),
        );
        #[cfg(target_os = "macos")]
        let redo_accelerator = Accelerator::new(Some(CMD_OR_CTRL | Modifiers::SHIFT), Code::KeyZ);
        #[cfg(target_os = "windows")]
        let redo_accelerator = Accelerator::new(Some(CMD_OR_CTRL), Code::KeyY);
        let redo = MenuItem::with_id(REDO_ID, "Redo", false, Some(redo_accelerator));
        let menu = Submenu::with_items("Edit", true, &[&undo, &redo])
            .map_err(|error| format!("failed to build edit menu: {error}"))?;
        Ok((menu, undo, redo))
    }

    fn settings_menu() -> Result<Submenu, String> {
        let save = MenuItem::with_id(SAVE_SETTINGS_ID, "Save Settings", true, None);
        let reload = MenuItem::with_id(RELOAD_CONFIGURATION_ID, "Reload Configuration", true, None);
        let reset = MenuItem::with_id(RESET_BRUSH_ID, "Reset Brush to Defaults", true, None);
        let separator = PredefinedMenuItem::separator();
        let open = MenuItem::with_id(
            OPEN_CONFIG_DIRECTORY_ID,
            "Open Configuration Folder",
            true,
            None,
        );

        Submenu::with_items(
            "Settings",
            true,
            &[&save, &reload, &reset, &separator, &open],
        )
        .map_err(|error| format!("failed to build settings menu: {error}"))
    }

    #[cfg(target_os = "macos")]
    fn application_menu() -> Result<Submenu, String> {
        let about = PredefinedMenuItem::about(
            Some("About Chromazen"),
            Some(AboutMetadata {
                name: Some("Chromazen".to_owned()),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                ..AboutMetadata::default()
            }),
        );
        let separator_1 = PredefinedMenuItem::separator();
        let services = PredefinedMenuItem::services(None);
        let separator_2 = PredefinedMenuItem::separator();
        let hide = PredefinedMenuItem::hide(None);
        let hide_others = PredefinedMenuItem::hide_others(None);
        let show_all = PredefinedMenuItem::show_all(None);
        let separator_3 = PredefinedMenuItem::separator();
        let quit = PredefinedMenuItem::quit(None);

        Submenu::with_items(
            "Chromazen",
            true,
            &[
                &about,
                &separator_1,
                &services,
                &separator_2,
                &hide,
                &hide_others,
                &show_all,
                &separator_3,
                &quit,
            ],
        )
        .map_err(|error| format!("failed to build application menu: {error}"))
    }

    fn command_for_id(id: &MenuId) -> Option<AppCommand> {
        match id.as_ref() {
            UNDO_ID => Some(AppCommand::Undo),
            REDO_ID => Some(AppCommand::Redo),
            SAVE_SETTINGS_ID => Some(AppCommand::SaveSettings),
            RELOAD_CONFIGURATION_ID => Some(AppCommand::ReloadConfiguration),
            RESET_BRUSH_ID => Some(AppCommand::ResetBrush),
            OPEN_CONFIG_DIRECTORY_ID => Some(AppCommand::OpenConfigDirectory),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_stable_menu_ids_to_commands() {
            assert_eq!(
                command_for_id(&MenuId::new(UNDO_ID)),
                Some(AppCommand::Undo)
            );
            assert_eq!(
                command_for_id(&MenuId::new(REDO_ID)),
                Some(AppCommand::Redo)
            );
            assert_eq!(
                command_for_id(&MenuId::new(SAVE_SETTINGS_ID)),
                Some(AppCommand::SaveSettings)
            );
            assert_eq!(
                command_for_id(&MenuId::new(RELOAD_CONFIGURATION_ID)),
                Some(AppCommand::ReloadConfiguration)
            );
            assert_eq!(
                command_for_id(&MenuId::new(RESET_BRUSH_ID)),
                Some(AppCommand::ResetBrush)
            );
            assert_eq!(
                command_for_id(&MenuId::new(OPEN_CONFIG_DIRECTORY_ID)),
                Some(AppCommand::OpenConfigDirectory)
            );
            assert_eq!(command_for_id(&MenuId::new("unknown")), None);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) use imp::NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) struct NativeMenu;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl NativeMenu {
    pub(super) fn new() -> Result<Self, String> {
        Ok(Self)
    }

    pub(super) fn set_event_handler<F>(&self, _handler: F)
    where
        F: Fn(super::command::AppCommand) + Send + Sync + 'static,
    {
    }

    pub(super) fn set_history_enabled(&self, _can_undo: bool, _can_redo: bool) {}

    pub(super) fn install(&mut self, _window: &winit::window::Window) -> Result<(), String> {
        Ok(())
    }
}
