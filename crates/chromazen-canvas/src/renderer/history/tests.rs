use super::*;

#[test]
fn inclusive_bounds_have_nonzero_copy_dimensions() {
    assert_eq!(
        TextureRect::from_inclusive(10, 20, 10, 20),
        TextureRect {
            x: 10,
            y: 20,
            width: 1,
            height: 1,
        }
    );
}

#[test]
fn rectangles_union() {
    assert_eq!(
        TextureRect {
            x: 10,
            y: 20,
            width: 5,
            height: 10,
        }
        .union(TextureRect {
            x: 2,
            y: 25,
            width: 10,
            height: 10,
        }),
        TextureRect {
            x: 2,
            y: 20,
            width: 13,
            height: 15,
        }
    );
}

#[test]
fn budget_evicts_oldest_entries_and_keeps_newest() {
    assert_eq!(eviction_count(300, 200, 3, [100, 100, 100].into_iter()), 1);
    assert_eq!(eviction_count(300, 50, 1, [300].into_iter()), 0);
}

#[test]
fn metadata_changes_are_structural_and_use_no_texture_budget() {
    let actions = [
        HistoryAction::BackgroundColor {
            before: [1.0; 4],
            after: [0.0, 0.0, 0.0, 1.0],
        },
        HistoryAction::RenameLayer {
            layer_id: LayerId(1),
            before: "Before".to_owned(),
            after: "After".to_owned(),
        },
        HistoryAction::LayerVisibility {
            layer_id: LayerId(1),
            before: true,
            after: false,
        },
        HistoryAction::LayerClipping {
            layer_id: LayerId(1),
            before: false,
            after: true,
        },
        HistoryAction::LayerOpacity {
            layer_id: LayerId(1),
            before: 100,
            after: 50,
        },
        HistoryAction::LayerBlendMode {
            layer_id: LayerId(1),
            before: BlendMode::Normal,
            after: BlendMode::Multiply,
        },
        HistoryAction::MoveLayer {
            layer_id: LayerId(1),
            before: 0,
            after: 1,
        },
    ];
    for action in actions {
        assert_eq!(action.target(), HistoryTarget::Structure);
        assert_eq!(action.byte_len(), 0);
    }
}
