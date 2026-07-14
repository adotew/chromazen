#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use muda::{Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

    #[cfg(target_os = "macos")]
    use muda::AboutMetadata;

    use super::super::command::AppCommand;

    const SAVE_SETTINGS_ID: &str = "minipaint.settings.save";
    const RELOAD_CONFIGURATION_ID: &str = "minipaint.settings.reload";
    const RESET_BRUSH_ID: &str = "minipaint.settings.reset-brush";
    const OPEN_CONFIG_DIRECTORY_ID: &str = "minipaint.settings.open-config-directory";

    pub(crate) struct NativeMenu {
        _menu: Menu,
    }

    impl NativeMenu {
        pub(crate) fn new() -> Result<Self, String> {
            let menu = Menu::new();

            #[cfg(target_os = "macos")]
            menu.append(&application_menu()?)
                .map_err(|error| format!("failed to add application menu: {error}"))?;

            menu.append(&settings_menu()?)
                .map_err(|error| format!("failed to add settings menu: {error}"))?;

            Ok(Self { _menu: menu })
        }
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
            Some("About minipaint-rs"),
            Some(AboutMetadata {
                name: Some("minipaint-rs".to_owned()),
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
            "minipaint-rs",
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

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used when menu event forwarding is connected")
    )]
    pub(super) fn command_for_id(id: &MenuId) -> Option<AppCommand> {
        match id.as_ref() {
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
}
