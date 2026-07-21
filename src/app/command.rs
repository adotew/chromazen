use crate::renderer::LayerId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    Undo,
    Redo,
    SelectLayer(LayerId),
    SelectBackground,
    AddLayer,
    DeleteSelectedLayer,
    SetBackgroundColor([u8; 3]),
    CommitBackgroundColor { before: [u8; 3], after: [u8; 3] },
    SwitchBrush(String),
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
}
