use super::*;

fn thumbnail_document(size: (u32, u32), background: [u8; 3]) -> crate::artwork::DocumentManifest {
    crate::artwork::DocumentManifest {
        schema_version: crate::artwork::DOCUMENT_SCHEMA_VERSION,
        width: size.0,
        height: size.1,
        background,
        brush_color: [170, 187, 204, 255],
        rotation: 0.0,
        flip: [false; 2],
        selected_layer: 1,
        layers: vec![crate::artwork::LayerManifest {
            id: 1,
            name: "Layer 1".to_owned(),
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: chromazen_canvas::BlendMode::Normal,
            file: "layers/1.png".to_owned(),
        }],
        references: Vec::new(),
    }
}

fn versions(generation: u64, layers: &[(u64, u64)]) -> DocumentVersions {
    DocumentVersions {
        generation,
        metadata: generation,
        layers: layers
            .iter()
            .map(|(id, version)| (LayerId(*id), *version))
            .collect(),
    }
}

#[test]
fn only_changed_and_new_layers_are_encoded() {
    let saved = versions(4, &[(1, 2), (2, 4)]);
    let current = versions(6, &[(1, 2), (2, 5), (3, 6)]);
    let changed = changed_layer_ids(&saved, &current);
    assert_eq!(changed, HashSet::from([LayerId(2), LayerId(3)]));
}

#[test]
fn metadata_only_changes_reuse_layer_pngs() {
    let saved = versions(4, &[(1, 2), (2, 4)]);
    let mut current = saved.clone();
    current.generation = 5;
    current.metadata = 5;
    assert!(changed_layer_ids(&saved, &current).is_empty());
}

#[test]
fn reference_metadata_changes_reuse_assets() {
    let saved = ReferenceVersions {
        generation: 2,
        assets: vec![(ReferenceId(1), 4), (ReferenceId(2), 7)],
    };
    let mut moved = saved.clone();
    moved.generation = 3;
    assert!(changed_reference_ids(&saved, &moved).is_empty());

    moved.assets[1].1 = 8;
    assert_eq!(
        changed_reference_ids(&saved, &moved),
        HashSet::from([ReferenceId(2)])
    );
}

#[test]
fn thumbnail_composites_premultiplied_layers() {
    let layer = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
    let document = thumbnail_document((1, 1), [0, 0, 255]);
    let png = encode_thumbnail(&[(LayerId(1), layer)], &document).unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
    let pixel = decoded.get_pixel(0, 0);
    assert!((127..=128).contains(&pixel[0]));
    assert_eq!(pixel[1], 0);
    assert!((126..=127).contains(&pixel[2]));
    assert_eq!(pixel[3], 255);
}

#[test]
fn thumbnail_follows_canvas_rotation_and_flip() {
    let mut layer = image::RgbaImage::from_pixel(4, 2, image::Rgba([0, 0, 255, 255]));
    layer.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    let mut document = thumbnail_document((4, 2), [0, 0, 0]);
    let at = |document: &crate::artwork::DocumentManifest, x: u32, y: u32| {
        let png = encode_thumbnail(&[(LayerId(1), layer.clone())], document).unwrap();
        *image::load_from_memory(&png)
            .unwrap()
            .to_rgba8()
            .get_pixel(x, y)
    };
    let red = image::Rgba([255, 0, 0, 255]);
    let near = THUMBNAIL_SIZE / 2 - 100;
    let far = THUMBNAIL_SIZE / 2 + 100;

    document.rotation = std::f32::consts::FRAC_PI_2;
    // A clockwise quarter turn moves the top-left corner to the top-right, in a tall frame.
    assert_eq!(at(&document, far + 20, 20), red);
    assert_eq!(at(&document, 20, THUMBNAIL_SIZE / 2)[3], 0);

    document.rotation = 0.0;
    document.flip = [true, false];
    assert_eq!(at(&document, THUMBNAIL_SIZE - 20, near + 20), red);
}

#[test]
fn rectangular_thumbnail_is_centered_without_stretching() {
    let layer = image::RgbaImage::new(4, 2);
    let document = thumbnail_document((4, 2), [10, 20, 30]);
    let png = encode_thumbnail(&[(LayerId(1), layer)], &document).unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0), &image::Rgba([0, 0, 0, 0]));
    assert_eq!(
        decoded.get_pixel(0, THUMBNAIL_SIZE / 2),
        &image::Rgba([10, 20, 30, 255])
    );
}
