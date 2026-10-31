use super::*;

#[test]
fn inserts_above_selection() {
    assert_eq!(insertion_index(Some(1), 3), 2);
    assert_eq!(insertion_index(None, 3), 3);
}

#[test]
fn merge_down_targets_the_next_lower_internal_layer() {
    assert_eq!(merge_down_target_index(2), Some(1));
    assert_eq!(merge_down_target_index(1), Some(0));
    assert_eq!(merge_down_target_index(0), None);
}

#[test]
fn deletion_prefers_layer_below() {
    assert_eq!(replacement_index_after_delete(3, 2), Some(1));
    assert_eq!(replacement_index_after_delete(3, 0), Some(1));
    assert_eq!(replacement_index_after_delete(1, 0), None);
}

#[test]
fn names_are_monotonic() {
    assert_eq!(layer_name(1), "Layer 1");
    assert_eq!(layer_name(42), "Layer 42");
}

#[test]
fn layer_names_are_trimmed_and_must_not_be_empty() {
    assert_eq!(
        normalized_layer_name("  Sketch  "),
        Some("Sketch".to_owned())
    );
    assert_eq!(normalized_layer_name("   "), None);
}

#[test]
fn relative_moves_translate_visual_edges_to_internal_order() {
    assert_eq!(relative_insertion_index(0, 1, DropEdge::Above), Some(1));
    assert_eq!(relative_insertion_index(2, 1, DropEdge::Below), Some(1));
    assert_eq!(relative_insertion_index(0, 1, DropEdge::Below), None);
    assert_eq!(relative_insertion_index(1, 1, DropEdge::Above), None);
}
