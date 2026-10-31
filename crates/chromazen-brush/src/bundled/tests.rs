use super::*;

#[test]
fn bundled_brushes_are_valid_and_decode() {
    for id in BUNDLED_BRUSH_IDS {
        let brush = LoadedBrushPreset::bundled(id).expect("bundled brush");
        brush.preset.validate().expect("valid bundled preset");
        assert!(brush.stamp_image.width() > 0);
    }
}

#[test]
fn unknown_id_is_not_bundled() {
    assert!(bundled_preset("missing").is_none());
}
