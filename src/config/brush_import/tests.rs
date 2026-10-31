use super::*;

#[test]
fn imports_abr_as_native_brush() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let store = ConfigStore::from_root(temp.path());
    let abr_path = temp.path().join("My Inks.abr");
    fs::write(&abr_path, legacy_abr("Soft Ink", 12, 2, 1, &[0, 255])).expect("ABR fixture");

    let result = store.import_abr(&abr_path).expect("import");

    assert_eq!(result.imported_ids, vec!["abr-my-inks-soft-ink"]);
    let loaded = store
        .load_brush(&result.imported_ids[0])
        .expect("installed brush");
    assert_eq!(loaded.preset.name, "Soft Ink");
    assert_eq!(loaded.preset.spacing.ratio, 0.12);
    assert_eq!(loaded.preset.size.default, 2.0);
    assert_eq!(loaded.stamp_image.get_pixel(1, 0).0, [255, 255, 255, 255]);
}

#[test]
fn repeated_import_does_not_overwrite_existing_brush() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let store = ConfigStore::from_root(temp.path());
    let abr_path = temp.path().join("set.abr");
    fs::write(&abr_path, legacy_abr("Ink", 25, 1, 1, &[255])).expect("ABR fixture");

    let first = store.import_abr(&abr_path).expect("first import");
    let second = store.import_abr(&abr_path).expect("second import");

    assert_eq!(first.imported_ids, vec!["abr-set-ink"]);
    assert_eq!(second.imported_ids, vec!["abr-set-ink-2"]);
}

#[test]
fn rejects_invalid_file_without_installing_brushes() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let store = ConfigStore::from_root(temp.path());
    let abr_path = temp.path().join("bad.abr");
    fs::write(&abr_path, b"not an abr").expect("ABR fixture");

    assert!(store.import_abr(&abr_path).is_err());
    assert!(!store.brushes_path().exists());
}

#[test]
fn missing_abr_spacing_uses_dense_fallback() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let brush_path = temp.path().join("brush");
    let brush = AbrBrush {
        name: None,
        width: 1,
        height: 1,
        mask: vec![255],
        spacing_percent: None,
    };

    write_staged_brush(&brush_path, "Imported", brush).expect("staged brush");
    let source = fs::read_to_string(brush_path.join("brush.toml")).expect("preset");
    let preset: BrushPreset = toml::from_str(&source).expect("parsed preset");

    assert_eq!(preset.spacing.ratio, 0.05);
}

#[test]
fn oversized_tip_is_scaled_to_native_limit() {
    let brush = AbrBrush {
        name: None,
        width: MAX_IMPORTED_STAMP_DIMENSION + 1,
        height: 1,
        mask: vec![255; (MAX_IMPORTED_STAMP_DIMENSION + 1) as usize],
        spacing_percent: None,
    };

    let image = build_stamp_image_from_abr_mask(&brush).expect("scaled stamp");

    assert_eq!(image.dimensions(), (MAX_IMPORTED_STAMP_DIMENSION, 1));
}

#[test]
fn slug_is_safe_and_bounded() {
    assert_eq!(slug("  My Ink / 02  "), "my-ink-02");
    assert_eq!(slug("画筆"), "abr-imported-brush");
    assert!(slug(&"a".repeat(200)).len() <= 80);
}

fn legacy_abr(name: &str, spacing: u16, width: i32, height: i32, mask: &[u8]) -> Vec<u8> {
    let utf16 = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut sample = Vec::new();
    sample.extend_from_slice(&0u32.to_be_bytes());
    sample.extend_from_slice(&spacing.to_be_bytes());
    sample.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    for character in utf16 {
        sample.extend_from_slice(&character.to_be_bytes());
    }
    sample.push(1);
    sample.extend_from_slice(&[0; 8]);
    for value in [0i32, 0, height, width] {
        sample.extend_from_slice(&value.to_be_bytes());
    }
    sample.extend_from_slice(&8u16.to_be_bytes());
    sample.push(0);
    sample.extend_from_slice(mask);

    let mut abr = Vec::new();
    abr.extend_from_slice(&2u16.to_be_bytes());
    abr.extend_from_slice(&1u16.to_be_bytes());
    abr.extend_from_slice(&2u16.to_be_bytes());
    abr.extend_from_slice(&(sample.len() as u32).to_be_bytes());
    abr.extend_from_slice(&sample);
    abr
}
