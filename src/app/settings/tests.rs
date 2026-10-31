use super::*;

#[test]
fn reload_normalizes_missing_brush_to_effective_fallback() {
    let config = AppConfig {
        active_brush: "missing".to_owned(),
        ..AppConfig::default()
    };

    let change = PendingBrushChange::reload(
        config,
        PaintTool::Brush,
        LoadedBrushPreset::bundled_charcoal(),
        Some("missing brush".to_owned()),
    );
    let brush_id = change.brush.id.clone();

    assert_eq!(
        change
            .reloaded_config
            .expect("reloaded config")
            .active_brush,
        brush_id
    );
}
