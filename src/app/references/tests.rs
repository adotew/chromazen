use super::*;

fn decoded(width: u32, height: u32) -> DecodedReference {
    let image = image::RgbaImage::new(width, height);
    DecodedReference {
        png: Arc::new(encode_png(&image).unwrap()),
        pixels: Arc::new(image),
    }
}

#[test]
fn imported_references_are_sized_and_versioned() {
    let mut board = ReferenceBoard::default();
    let id = board.add(decoded(2400, 1200), [4100.0, 100.0]);
    assert_eq!(board.images()[0].size, [1200.0, 600.0]);
    assert_eq!(board.versions().generation, 1);
    assert_eq!(board.manifest()[0].id, id.0);
}

#[test]
fn transforms_reject_invalid_geometry_and_locked_images() {
    let mut board = ReferenceBoard::default();
    let id = board.add(decoded(10, 10), [0.0, 0.0]);
    assert!(!board.set_transform(id, [f32::NAN, 0.0], [10.0, 10.0]));
    assert!(board.toggle_locked(id));
    assert!(!board.set_transform(id, [20.0, 20.0], [10.0, 10.0]));
}

#[test]
fn loading_preserves_order_and_advances_ids() {
    let manifest = |id| ReferenceManifest {
        id,
        file: format!("references/{id}.png"),
        position: [id as f32, 0.0],
        size: [10.0, 10.0],
        visible: true,
        locked: false,
    };
    let mut board = ReferenceBoard::default();
    board.replace_with_loaded_references(vec![
        (manifest(7), decoded(10, 10)),
        (manifest(2), decoded(10, 10)),
    ]);
    assert_eq!(
        board
            .manifest()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        [7, 2]
    );
    assert_eq!(board.add(decoded(10, 10), [0.0, 0.0]), ReferenceId(8));
}

#[test]
fn decodes_png_and_rejects_unsafe_source_dimensions() {
    let source = decoded(2, 1);
    let decoded = decode_reference_bytes(source.png.as_slice(), "memory").unwrap();
    assert_eq!(decoded.pixels.dimensions(), (2, 1));
    assert!(validate_source_dimensions((0, 1)).is_err());
    assert!(validate_source_dimensions((8192, 8192)).is_ok());
    assert!(validate_source_dimensions((MAX_REFERENCE_SOURCE_DIMENSION + 1, 1)).is_err());
    assert!(validate_source_dimensions((MAX_REFERENCE_SOURCE_DIMENSION, 8192)).is_err());
}

#[test]
fn fitted_reference_pixels_obey_texture_limits() {
    assert_eq!(fitted_reference_pixel_size((4000, 2000)), (4000, 2000));

    for source in [(12_000, 2_000), (8_000, 8_000), (16_384, 4_096)] {
        let fitted = fitted_reference_pixel_size(source);
        assert!(fitted.0 <= MAX_REFERENCE_DIMENSION);
        assert!(fitted.1 <= MAX_REFERENCE_DIMENSION);
        assert!(u64::from(fitted.0) * u64::from(fitted.1) <= MAX_REFERENCE_PIXELS);
        let source_ratio = f64::from(source.0) / f64::from(source.1);
        let fitted_ratio = f64::from(fitted.0) / f64::from(fitted.1);
        assert!((source_ratio - fitted_ratio).abs() < 0.01);
    }
}

#[test]
fn imported_references_persist_resized_pixels() {
    let source = image::DynamicImage::ImageRgba8(image::RgbaImage::new(4, 2));
    let prepared = prepare_imported_reference(source, (2, 1)).unwrap();
    assert_eq!(prepared.pixels.dimensions(), (2, 1));
    assert_eq!(
        image::load_from_memory(prepared.png.as_slice())
            .unwrap()
            .dimensions(),
        (2, 1)
    );
}

#[test]
fn stored_references_keep_png_bytes_and_fit_texture_pixels() {
    let png = vec![1, 2, 3, 4];
    let source = image::DynamicImage::ImageRgba8(image::RgbaImage::new(400, 200));
    let prepared = prepare_stored_reference(source, png.clone());
    assert_eq!(prepared.png.as_slice(), png);
    assert_eq!(prepared.pixels.dimensions(), (400, 200));
    assert_eq!(fitted_texture_pixel_size((8000, 4000)), (4096, 2048));
    let area_fitted = fitted_texture_pixel_size((4096, 4096));
    assert!(u64::from(area_fitted.0) * u64::from(area_fitted.1) <= MAX_REFERENCE_TEXTURE_PIXELS);
}
