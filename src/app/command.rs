use crate::{
    app::{input::EditorTool, references::ReferenceId},
    artwork::ArtworkId,
    paint::PaintTool,
    renderer::{DropEdge, LayerId, LayerTransform},
};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(super) enum AppCommand {
    Undo,
    Redo,
    RotateCanvasLeft,
    RotateCanvasRight,
    ResetCanvasRotation,
    ToggleCanvasFlipHorizontal,
    ToggleCanvasFlipVertical,
    RequestCanvasResize,
    ResizeCanvas {
        width: u32,
        height: u32,
        origin: [i32; 2],
    },
    SelectTool(EditorTool),
    SetLayerTransform(LayerTransform),
    ApplyLayerTransform,
    CancelLayerTransform,
    SelectLayer(LayerId),
    AddLayer,
    DeleteSelectedLayer,
    RenameLayer {
        id: LayerId,
        name: String,
    },
    MergeLayerDown(LayerId),
    SetLayerClipped {
        id: LayerId,
        clipped: bool,
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
    AddReferences,
    SetReferenceTransform {
        id: ReferenceId,
        position: [f32; 2],
        size: [f32; 2],
    },
    ToggleReferenceLocked(ReferenceId),
    DeleteReference(ReferenceId),
    SetBrushColor([u8; 4]),
    SetBackgroundColor([u8; 3]),
    CommitBackgroundColor {
        before: [u8; 3],
        after: [u8; 3],
    },
    SwitchBrush {
        tool: PaintTool,
        id: String,
    },
    ImportBrushes,
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
    ShowShortcuts,
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
