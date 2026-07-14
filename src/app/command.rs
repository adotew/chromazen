#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    SwitchBrush(String),
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
}
