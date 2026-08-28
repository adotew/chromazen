use crate::{
    app::{input::EditorTool, references::ReferenceId},
    artwork::ArtworkId,
    paint::PaintTool,
    renderer::{DropEdge, LayerId, LayerTransform},
};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(super) enum AppCommand {
    Editor(EditorCommand),
    Gallery(GalleryCommand),
    Navigation(NavigationCommand),
    Settings(SettingsCommand),
    Ui(UiCommand),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum EditorCommand {
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
    DuplicateSelectedLayer,
    ClearLayer,
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
    SaveArtwork,
    ExportPng,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum SettingsCommand {
    SwitchBrush { tool: PaintTool, id: String },
    ImportBrushes,
    Save,
    ReloadConfiguration,
    ResetBrush,
    OpenConfigDirectory,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum NavigationCommand {
    NewArtwork,
    CreateArtwork { width: u32, height: u32 },
    OpenArtwork(ArtworkId),
    ShowGallery,
    CancelPending,
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum GalleryCommand {
    Rename { id: ArtworkId, title: String },
    Duplicate(ArtworkId),
    Delete(ArtworkId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum UiCommand {
    ShowShortcuts,
}
