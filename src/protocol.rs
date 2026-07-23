use serde::{Deserialize, Serialize};

use crate::{
    config::BrushSummary,
    paint::PaintTool,
    renderer::{LayerId, LayerSnapshot},
};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiCommand {
    RequestSnapshot,
    SetTool { tool: PaintTool },
    SetBrushSize { size: f32 },
    SetBrushColor { color: [u8; 4] },
    SetSmoothingStrength { strength: f32 },
    SelectBrush { id: String },
    SelectLayer { id: LayerId },
    SelectBackground,
    AddLayer,
    DeleteSelectedLayer,
    SetBackgroundColor { color: [u8; 3] },
    CommitBackgroundColor { before: [u8; 3], after: [u8; 3] },
    FitCanvas,
    Undo,
    Redo,
    SaveSettings,
    ReloadConfiguration,
    ResetBrush,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrushUiState {
    pub size: f32,
    pub color: [u8; 4],
    pub minimum_size: f32,
    pub maximum_size: f32,
    pub default_size: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    pub text: String,
    pub is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    pub revision: u64,
    pub tool: PaintTool,
    pub brush: BrushUiState,
    pub smoothing_strength: f32,
    pub active_brush: String,
    pub brushes: Vec<BrushSummary>,
    pub layers: LayerSnapshot,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_delete_layer: bool,
    pub message: Option<UiMessage>,
}

#[cfg(test)]
mod tests {
    use super::UiCommand;
    use crate::paint::PaintTool;

    #[test]
    fn command_wire_format_is_tagged_and_camel_case() {
        let command: UiCommand =
            serde_json::from_str(r#"{"type":"setTool","tool":"eraser"}"#).expect("valid command");
        assert_eq!(
            command,
            UiCommand::SetTool {
                tool: PaintTool::Eraser
            }
        );
    }
}
