use super::*;

#[test]
fn canvas_document_round_trips_persisted_layer_metadata() {
    let persisted = DocumentManifest {
        schema_version: DOCUMENT_SCHEMA_VERSION,
        width: 20,
        height: 30,
        background: [1, 2, 3],
        brush_color: [4, 5, 6, 255],
        rotation: 0.0,
        flip: [false; 2],
        selected_layer: 7,
        layers: vec![LayerManifest {
            id: 7,
            name: "Paint".to_owned(),
            visible: true,
            opacity: 80,
            clipped: false,
            blend_mode: chromazen_canvas::BlendMode::Multiply,
            file: "layers/7.png".to_owned(),
        }],
        references: Vec::new(),
    };

    let canvas = canvas_document(&persisted);
    let converted = document_manifest(canvas);
    assert_eq!(converted.width, persisted.width);
    assert_eq!(converted.height, persisted.height);
    assert_eq!(converted.background, persisted.background);
    assert_eq!(converted.selected_layer, persisted.selected_layer);
    assert_eq!(converted.layers, persisted.layers);
}
