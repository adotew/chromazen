use super::*;

fn layer(id: u64, visible: bool, clipped: bool) -> LayerInfo {
    LayerInfo {
        id: LayerId(id),
        name: format!("Layer {id}"),
        visible,
        opacity: 255,
        clipped,
        blend_mode: BlendMode::default(),
    }
}

#[test]
fn merge_down_needs_two_visible_unclipped_layers() {
    let layers = [layer(1, true, false), layer(2, true, false)];
    assert!(!can_merge_layer_down(&layers, 0));
    assert!(can_merge_layer_down(&layers, 1));

    let hidden_lower = [layer(1, false, false), layer(2, true, false)];
    assert!(!can_merge_layer_down(&hidden_lower, 1));
    let hidden_upper = [layer(1, true, false), layer(2, false, false)];
    assert!(!can_merge_layer_down(&hidden_upper, 1));
    let clipped_lower = [
        layer(1, true, false),
        layer(2, true, true),
        layer(3, true, false),
    ];
    assert!(!can_merge_layer_down(&clipped_lower, 2));
    let clipped_above = [
        layer(1, true, false),
        layer(2, true, false),
        layer(3, true, true),
    ];
    assert!(!can_merge_layer_down(&clipped_above, 1));
}

#[test]
fn clipping_base_cannot_be_deleted_but_a_lone_layer_can() {
    assert!(can_delete_layer(&[layer(1, true, false)], 0));
    let clipped = [layer(1, true, false), layer(2, true, true)];
    assert!(!can_delete_layer(&clipped, 0));
    assert!(can_delete_layer(&clipped, 1));
    let unclipped = [layer(1, true, false), layer(2, true, false)];
    assert!(can_delete_layer(&unclipped, 0));
}
