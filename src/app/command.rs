use crate::{
    artwork::ArtworkId,
    paint::PaintTool,
    renderer::{DropEdge, LayerId},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AppCommand {
    Undo,
    Redo,
    SelectTool(PaintTool),
    SelectLayer(LayerId),
    AddLayer,
    DeleteSelectedLayer,
    RenameLayer {
        id: LayerId,
        name: String,
    },
    SetLayerVisibility {
        id: LayerId,
        visible: bool,
    },
    SetLayerOpacity {
        id: LayerId,
        opacity: u8,
    },
    CommitLayerOpacity {
        id: LayerId,
        before: u8,
        after: u8,
    },
    MoveLayer {
        dragged: LayerId,
        target: LayerId,
        edge: DropEdge,
    },
    SetBackgroundColor([u8; 3]),
    CommitBackgroundColor {
        before: [u8; 3],
        after: [u8; 3],
    },
    SwitchBrush {
        tool: PaintTool,
        id: String,
    },
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
    NewArtwork,
    CreateArtwork {
        width: u32,
        height: u32,
    },
    OpenArtwork(ArtworkId),
    SaveArtwork,
    ExportPng,
    ShowGallery,
    RenameArtwork {
        id: ArtworkId,
        title: String,
    },
    DeleteArtwork(ArtworkId),
    CancelPendingNavigation,
    Quit,
}
