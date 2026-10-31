use super::*;

#[test]
fn size_preview_keeps_aspect_ratio_within_bounds() {
    let bounds = egui::vec2(96.0, 120.0);
    for (size, expected) in [
        ([4000, 2000], egui::vec2(96.0, 48.0)),
        ([1000, 4000], egui::vec2(30.0, 120.0)),
        ([20000, 1], egui::vec2(96.0, 2.0)),
        ([0, 0], egui::vec2(96.0, 96.0)),
    ] {
        let fitted = fit_size_preview(size, bounds);
        assert!(
            (fitted - expected).length() < 1e-3,
            "{size:?} -> {fitted:?}"
        );
    }
}

#[test]
fn accent_colors_include_the_default() {
    let original = egui::Color32::from_rgb(
        DEFAULT_ACCENT_COLOR[0],
        DEFAULT_ACCENT_COLOR[1],
        DEFAULT_ACCENT_COLOR[2],
    );
    assert!(ACCENT_COLORS.iter().any(|(_, color)| *color == original));
}
