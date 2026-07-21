#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    Undo,
    Redo,
    SwitchBrush(String),
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
}
