use super::*;

fn document() -> DocumentManifest {
    DocumentManifest {
        schema_version: DOCUMENT_SCHEMA_VERSION,
        width: 4000,
        height: 4000,
        background: [255; 3],
        brush_color: default_brush_color(),
        rotation: 0.0,
        flip: [false; 2],
        selected_layer: 1,
        layers: vec![LayerManifest {
            id: 1,
            name: "Layer 1".to_owned(),
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: BlendMode::Normal,
            file: "layers/1.png".to_owned(),
        }],
        references: Vec::new(),
    }
}

#[test]
fn valid_document_is_accepted() {
    assert!(document().validate().is_ok());
}

#[test]
fn selected_layer_must_exist() {
    let mut document = document();
    document.selected_layer = 2;
    assert!(document.validate().is_err());
}

#[test]
fn bottom_layer_cannot_be_clipped() {
    let mut document = document();
    document.layers[0].clipped = true;
    assert!(document.validate().is_err());
}

#[test]
fn invalid_opacity_is_rejected() {
    let mut document = document();
    document.layers[0].opacity = 101;
    assert!(document.validate().is_err());
}

#[test]
fn brush_color_defaults_for_existing_documents() {
    let source = toml::to_string(&document()).unwrap();
    let without_color = source.replace("brush_color = [170, 187, 204, 255]\n", "");
    let decoded: DocumentManifest = toml::from_str(&without_color).unwrap();
    assert_eq!(decoded.brush_color, default_brush_color());
}

#[test]
fn clipping_defaults_to_disabled_for_existing_documents() {
    let source = toml::to_string(&document()).unwrap();
    let without_clipping = source.replace("clipped = false\n", "");
    let decoded: DocumentManifest = toml::from_str(&without_clipping).unwrap();
    assert!(!decoded.layers[0].clipped);
}

#[test]
fn blend_modes_round_trip_by_name() {
    let mut document = document();
    document.layers[0].blend_mode = BlendMode::Overlay;
    let source = toml::to_string(&document).unwrap();
    assert!(source.contains("blend_mode = \"overlay\"\n"));
    let decoded: DocumentManifest = toml::from_str(&source).unwrap();
    assert_eq!(decoded.layers[0].blend_mode, BlendMode::Overlay);
}

#[test]
fn unknown_blend_mode_is_rejected() {
    let source = toml::to_string(&document()).unwrap();
    let unknown = source.replace("blend_mode = \"normal\"", "blend_mode = \"dissolve\"");
    assert_ne!(unknown, source);
    assert!(toml::from_str::<DocumentManifest>(&unknown).is_err());
}

#[test]
fn version_three_document_is_migrated_with_normal_blend_modes() {
    let source = toml::to_string(&document()).unwrap();
    let version_three = source
        .replace("schema_version = 5\n", "schema_version = 3\n")
        .replace("rotation = 0.0\nflip = [false, false]\n", "")
        .replace("blend_mode = \"normal\"\n", "");
    let decoded: DocumentManifest = toml::from_str(&version_three).unwrap();
    let migrated = decoded.migrate().unwrap();
    assert_eq!(migrated.schema_version, DOCUMENT_SCHEMA_VERSION);
    assert_eq!(migrated.layers[0].blend_mode, BlendMode::Normal);
    assert!(migrated.validate().is_ok());
}

#[test]
fn version_four_document_is_migrated_upright() {
    let source = toml::to_string(&document()).unwrap();
    let version_four = source
        .replace("schema_version = 5\n", "schema_version = 4\n")
        .replace("rotation = 0.0\nflip = [false, false]\n", "");
    assert_ne!(version_four, source);
    let migrated = toml::from_str::<DocumentManifest>(&version_four)
        .unwrap()
        .migrate()
        .unwrap();
    assert_eq!(migrated, document());
}

#[test]
fn orientation_round_trips_and_must_be_finite() {
    let mut document = document();
    document.rotation = 1.25;
    document.flip = [true, false];
    let source = toml::to_string(&document).unwrap();
    assert_eq!(
        toml::from_str::<DocumentManifest>(&source).unwrap(),
        document
    );

    document.rotation = f32::NAN;
    assert!(document.validate().is_err());
}

#[test]
fn layer_metadata_is_required() {
    let source = toml::to_string(&document()).unwrap();
    let without_visibility = source.replace("visible = true\n", "");
    let without_opacity = source.replace("opacity = 100\n", "");
    assert!(toml::from_str::<DocumentManifest>(&without_visibility).is_err());
    assert!(toml::from_str::<DocumentManifest>(&without_opacity).is_err());
}

#[test]
fn version_two_document_is_migrated_without_references() {
    let mut document = document();
    document.schema_version = LEGACY_DOCUMENT_SCHEMA_VERSION;
    let migrated = document.migrate().unwrap();
    assert_eq!(migrated.schema_version, DOCUMENT_SCHEMA_VERSION);
    assert!(migrated.references.is_empty());
}

#[test]
fn unsupported_document_schema_is_rejected() {
    let mut document = document();
    document.schema_version = 1;
    assert!(document.migrate().is_err());
}

#[test]
fn reference_geometry_and_paths_are_validated() {
    let mut document = document();
    document.references.push(ReferenceManifest {
        id: 1,
        file: "references/1.png".to_owned(),
        position: [-100.0, 20.0],
        size: [640.0, 480.0],
        visible: true,
        locked: false,
    });
    assert!(document.validate().is_ok());

    document.references[0].size[0] = 0.0;
    assert!(document.validate().is_err());
    document.references[0].size[0] = 640.0;
    document.references[0].file = "../reference.png".to_owned();
    assert!(document.validate().is_err());
}

#[test]
fn generated_layer_paths_cannot_escape_revision() {
    let mut document = document();
    document.layers[0].file = "../outside.png".to_owned();
    assert!(document.validate().is_err());
}

#[test]
fn titles_are_trimmed_and_empty_titles_are_rejected() {
    assert_eq!(normalized_title("  Study  ").unwrap(), "Study");
    assert!(normalized_title("  ").is_err());
}
