use crate::renderer::LayerId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    Undo,
    Redo,
    SelectLayer(LayerId),
    SelectBackground,
    AddLayer,
    DeleteSelectedLayer,
    SwitchBrush(String),
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
}
