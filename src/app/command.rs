use crate::{artwork::ArtworkId, paint::PaintTool, renderer::LayerId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    Undo,
    Redo,
    SelectTool(PaintTool),
    SelectLayer(LayerId),
    AddLayer,
    DeleteSelectedLayer,
    SetBackgroundColor([u8; 3]),
    CommitBackgroundColor { before: [u8; 3], after: [u8; 3] },
    SwitchBrush(String),
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
    NewArtwork,
    OpenArtwork(ArtworkId),
    SaveArtwork,
    ShowGallery,
    RenameArtwork { id: ArtworkId, title: String },
    DeleteArtwork(ArtworkId),
    CancelPendingNavigation,
}
