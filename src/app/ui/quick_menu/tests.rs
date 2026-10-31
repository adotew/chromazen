use super::*;

const TOOL_SLOT: usize = 0;
const BRUSH_SLOT: usize = 1;
const LAYER_SLOT: usize = 3;
const CANVAS_SLOT: usize = 4;
const BRUSHES: usize = QUICK_MENU_GROUP_CAPACITY;

fn offset_at(degrees: f32, distance: f32) -> egui::Vec2 {
    quick_menu_direction(degrees.to_radians()) * distance
}

fn group_action_offset(slot: usize, index: usize) -> egui::Vec2 {
    let count = QUICK_SLOTS[slot].group().unwrap().actions(BRUSHES).len();
    quick_menu_direction(quick_menu_group_action_angle(slot, index, count))
        * QUICK_MENU_GROUP_RADIUS
}

fn layer(id: u64, visible: bool) -> LayerInfo {
    LayerInfo {
        id: LayerId(id),
        name: format!("Layer {id}"),
        visible,
        opacity: 255,
        clipped: false,
        blend_mode: BlendMode::default(),
    }
}

fn context(menu: ApplicationMenuState, layers: Vec<LayerInfo>, selection: u64) -> QuickMenuContext {
    let layers = LayerSnapshot {
        layers,
        selection: LayerId(selection),
        background_color: [1.0; 4],
    };
    QuickMenuContext::new(menu, EditorTool::default(), &layers)
}

#[test]
fn quick_menu_slots_run_clockwise_from_the_top() {
    for slot in 0..QUICK_SLOTS.len() {
        let offset = quick_menu_direction(quick_menu_slot_angle(slot)) * QUICK_MENU_SLOT_RADIUS;
        assert_eq!(quick_menu_slot_at(offset), Some(slot));
    }
    assert_eq!(quick_menu_slot_at(egui::vec2(0.0, -60.0)), Some(0));
}

#[test]
fn quick_menu_slot_boundaries_wrap_around_the_top() {
    assert_eq!(quick_menu_slot_at(offset_at(25.0, 60.0)), Some(0));
    assert_eq!(quick_menu_slot_at(offset_at(26.0, 60.0)), Some(1));
    assert_eq!(quick_menu_slot_at(offset_at(334.0, 60.0)), Some(6));
    assert_eq!(quick_menu_slot_at(offset_at(335.0, 60.0)), Some(0));
    assert_eq!(quick_menu_slot_at(offset_at(-10.0, 60.0)), Some(0));
}

#[test]
fn quick_menu_center_and_far_points_select_nothing() {
    assert_eq!(quick_menu_slot_at(egui::Vec2::ZERO), None);
    assert_eq!(
        quick_menu_slot_at(offset_at(90.0, QUICK_MENU_DEAD_ZONE - 1.0)),
        None
    );
    assert_eq!(
        quick_menu_slot_at(offset_at(90.0, QUICK_MENU_REACH + 1.0)),
        None
    );
    assert_eq!(
        quick_menu_slot_at(offset_at(60.0, QUICK_MENU_REACH)),
        Some(1)
    );
}

#[test]
fn groups_fit_their_arc() {
    for slot in QUICK_SLOTS {
        if let Some(group) = slot.group() {
            assert!((1..=QUICK_MENU_GROUP_CAPACITY).contains(&group.actions(BRUSHES).len()));
        }
    }
}

#[test]
fn group_actions_run_clockwise_along_the_arc() {
    for slot in [TOOL_SLOT, BRUSH_SLOT, LAYER_SLOT, CANVAS_SLOT] {
        let count = QUICK_SLOTS[slot].group().unwrap().actions(BRUSHES).len();
        for index in 0..count {
            assert_eq!(
                quick_menu_target_at(group_action_offset(slot, index), Some(slot), BRUSHES),
                Some(QuickTarget::GroupAction(index))
            );
        }
        let first = quick_menu_angle_of(group_action_offset(slot, 0));
        let last = quick_menu_angle_of(group_action_offset(slot, count - 1));
        // The tool arc straddles the top, where angles wrap.
        assert!((last - first).rem_euclid(TAU) < PI);
    }
}

#[test]
fn pointer_outside_the_open_arc_returns_to_ring_slots() {
    assert_eq!(
        quick_menu_target_at(offset_at(45.0, 130.0), Some(LAYER_SLOT), BRUSHES),
        Some(QuickTarget::Slot(1))
    );
    assert_eq!(
        quick_menu_target_at(
            offset_at(135.0, QUICK_MENU_GROUP_REACH + 1.0),
            Some(LAYER_SLOT),
            BRUSHES
        ),
        None
    );
    assert_eq!(
        quick_menu_target_at(offset_at(154.0, QUICK_MENU_GROUP_RADIUS), None, BRUSHES),
        Some(QuickTarget::Slot(LAYER_SLOT))
    );
    assert_eq!(
        quick_menu_target_at(egui::Vec2::ZERO, Some(LAYER_SLOT), BRUSHES),
        None
    );
}

#[test]
fn diagonal_path_to_an_edge_action_keeps_the_group_open() {
    let start = quick_menu_direction(quick_menu_slot_angle(LAYER_SLOT)) * QUICK_MENU_SLOT_RADIUS;
    let last = QuickGroup::Layer.actions(BRUSHES).len() - 1;
    let end = group_action_offset(LAYER_SLOT, last);
    let mut group = Some(LAYER_SLOT);
    let mut crossed_neighbor = false;
    for step in 0..=20 {
        let offset = start + (end - start) * (step as f32 / 20.0);
        crossed_neighbor |= quick_menu_slot_at(offset) == Some(CANVAS_SLOT);
        group = group_after_hover(group, quick_menu_target_at(offset, group, BRUSHES));
        assert_eq!(group, Some(LAYER_SLOT));
    }
    assert!(crossed_neighbor);
    assert_eq!(
        quick_menu_target_at(end, group, BRUSHES),
        Some(QuickTarget::GroupAction(last))
    );
}

#[test]
fn moving_between_neighboring_icons_switches_groups_before_reaching_the_next_icon() {
    let layer = quick_menu_direction(quick_menu_slot_angle(LAYER_SLOT)) * QUICK_MENU_SLOT_RADIUS;
    let canvas = quick_menu_direction(quick_menu_slot_angle(CANVAS_SLOT)) * QUICK_MENU_SLOT_RADIUS;
    let mut group = Some(LAYER_SLOT);
    for step in 0..=20 {
        let offset = layer + (canvas - layer) * (step as f32 / 20.0);
        group = group_after_hover(group, quick_menu_target_at(offset, group, BRUSHES));
        if step <= 10 {
            assert_eq!(group, Some(LAYER_SLOT));
        } else if step >= 15 {
            assert_eq!(group, Some(CANVAS_SLOT));
        }
    }
}

#[test]
fn hovering_a_direct_slot_closes_the_open_group() {
    assert_eq!(
        group_after_hover(None, Some(QuickTarget::Slot(LAYER_SLOT))),
        Some(LAYER_SLOT)
    );
    assert_eq!(
        group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::Slot(CANVAS_SLOT))),
        Some(CANVAS_SLOT)
    );
    assert_eq!(
        group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::Slot(2))),
        None
    );
    assert_eq!(group_after_hover(Some(LAYER_SLOT), None), Some(LAYER_SLOT));
    assert_eq!(
        group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::GroupAction(2))),
        Some(LAYER_SLOT)
    );
}

fn layer_band() -> QuickMenuBand {
    let actions = QuickGroup::Layer.actions(BRUSHES).len();
    QuickMenuBand::new(
        egui::Pos2::ZERO,
        QUICK_MENU_RADIUS,
        QUICK_MENU_BAND_WIDTH,
        QUICK_MENU_BAND_CORNER_RADIUS,
        [
            quick_menu_group_action_angle(LAYER_SLOT, 0, actions) - quick_menu_group_end_padding(),
            quick_menu_group_action_angle(LAYER_SLOT, actions - 1, actions)
                + quick_menu_group_end_padding(),
        ],
    )
}

#[test]
fn group_band_pads_action_highlights_like_the_ring() {
    let ring_padding = QUICK_MENU_RADIUS - QUICK_MENU_SLOT_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
    let band = layer_band();
    assert!(quick_menu_band_mesh(band, egui::Color32::WHITE, 1.0).is_valid());
    let edge = band.edge();
    let actions = QuickGroup::Layer.actions(BRUSHES).len();
    for index in 0..actions {
        let action = egui::Pos2::ZERO + group_action_offset(LAYER_SLOT, index);
        for (point, _) in &edge {
            assert!(point.distance(action) >= QUICK_MENU_HIGHLIGHT_RADIUS + ring_padding - 0.01);
        }
        let outer_padding =
            band.outer_radius - QUICK_MENU_GROUP_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
        assert!((outer_padding - ring_padding).abs() < 0.01);
    }
    let ring_highlight_edge = QUICK_MENU_SLOT_RADIUS + QUICK_MENU_HIGHLIGHT_RADIUS;
    let band_highlight_edge = QUICK_MENU_GROUP_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
    assert!((band_highlight_edge - ring_highlight_edge - ring_padding).abs() < 0.01);
}

#[test]
fn group_band_outline_starts_and_ends_on_the_disc_border() {
    let band = layer_band();
    let outline = quick_menu_band_outline(band, 1.0);
    let [first, last] = [outline[0], outline[outline.len() - 1]];
    for point in [first, last] {
        assert!((point.to_vec2().length() - (QUICK_MENU_RADIUS - 0.5)).abs() < 0.6);
    }
    for point in &outline[1..outline.len() - 1] {
        assert!(point.to_vec2().length() > QUICK_MENU_RADIUS);
    }
    assert!(band.inner_radius < QUICK_MENU_RADIUS - 0.5);
}

#[test]
fn group_band_corners_shrink_to_fit_a_narrow_band() {
    let band = QuickMenuBand::new(egui::Pos2::ZERO, 100.0, 4.0, 10.0, [0.0, 0.01]);
    assert!(band.corner_radius <= 2.0);
    assert!(band.corner_radius <= band.inner_radius * 0.005_f32.tan());
    assert!(quick_menu_band_mesh(band, egui::Color32::WHITE, 1.0).is_valid());
}

#[test]
fn quick_menu_stays_inside_the_window() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let inset = QUICK_MENU_RADIUS + QUICK_MENU_BAND_WIDTH + QUICK_MENU_SCREEN_MARGIN;

    assert_eq!(
        quick_menu_center(egui::pos2(400.0, 300.0), screen),
        egui::pos2(400.0, 300.0)
    );
    assert_eq!(
        quick_menu_center(egui::pos2(5.0, 590.0), screen),
        egui::pos2(inset, 600.0 - inset)
    );

    let small = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 200.0));
    assert_eq!(
        quick_menu_center(egui::pos2(0.0, 0.0), small),
        small.center()
    );
}

#[test]
fn quick_actions_dispatch_existing_editor_commands() {
    let selected = LayerId(7);
    let editor = |action: QuickAction| match action.command(selected, &[]) {
        AppCommand::Editor(command) => command,
        command => panic!("{action:?} dispatched {command:?}"),
    };
    let ring = QUICK_SLOTS.map(|slot| match slot {
        QuickSlot::Action(action) => Some(editor(action)),
        QuickSlot::Group(_) => None,
    });
    assert_eq!(
        ring,
        [
            None,
            None,
            Some(EditorCommand::Redo),
            None,
            None,
            Some(EditorCommand::SelectTool(EditorTool::Select)),
            Some(EditorCommand::Undo),
        ]
    );
    let tool: Vec<_> = QuickGroup::Tool
        .actions(BRUSHES)
        .iter()
        .map(|&action| editor(action))
        .collect();
    assert_eq!(
        tool,
        [
            EditorCommand::SelectTool(PaintTool::Brush.into()),
            EditorCommand::SelectTool(PaintTool::Eraser.into()),
            EditorCommand::SelectTool(PaintTool::Smudge.into()),
        ]
    );
    let brushes = [QuickBrush {
        tool: PaintTool::Eraser,
        id: "soft".to_owned(),
        name: "Soft".to_owned(),
        texture: None,
    }];
    assert_eq!(
        QuickGroup::Brush.actions(brushes.len()),
        [QuickAction::Preset(0)]
    );
    assert_eq!(
        QuickAction::Preset(0).command(selected, &brushes),
        AppCommand::Settings(SettingsCommand::SwitchBrush {
            tool: PaintTool::Eraser,
            id: "soft".to_owned(),
        })
    );
    let layer: Vec<_> = QuickGroup::Layer
        .actions(BRUSHES)
        .iter()
        .map(|&action| editor(action))
        .collect();
    assert_eq!(
        layer,
        [
            EditorCommand::AddLayer,
            EditorCommand::DuplicateSelectedLayer,
            EditorCommand::MergeLayerDown(selected),
            EditorCommand::ClearLayer,
            EditorCommand::DeleteSelectedLayer,
        ]
    );
    let canvas: Vec<_> = QuickGroup::Canvas
        .actions(BRUSHES)
        .iter()
        .map(|&action| editor(action))
        .collect();
    assert_eq!(
        canvas,
        [
            EditorCommand::RotateCanvasRight,
            EditorCommand::ToggleCanvasFlipHorizontal,
            EditorCommand::ResetCanvasRotation,
            EditorCommand::ToggleCanvasFlipVertical,
            EditorCommand::RotateCanvasLeft,
        ]
    );
}

#[test]
fn unavailable_actions_are_disabled() {
    let menu = ApplicationMenuState::new(true, false, false, true, false);
    let bottom = context(menu, vec![layer(1, true), layer(2, true)], 1);
    assert!(!QuickAction::Undo.is_enabled(bottom));
    assert!(QuickAction::Redo.is_enabled(bottom));
    assert!(!QuickAction::MergeDown.is_enabled(bottom));
    assert!(QuickAction::DeleteLayer.is_enabled(bottom));
    assert!(QuickAction::RotateLeft.is_enabled(bottom));

    let top = context(menu, vec![layer(1, true), layer(2, true)], 2);
    assert!(QuickAction::MergeDown.is_enabled(top));
    let hidden = context(menu, vec![layer(1, true), layer(2, false)], 2);
    assert!(!QuickAction::MergeDown.is_enabled(hidden));

    let cropping = ApplicationMenuState::new(true, true, true, true, false);
    let cropping = context(cropping, vec![layer(1, true)], 1);
    assert!(!QuickAction::FlipHorizontal.is_enabled(cropping));
    assert!(QuickAction::ClearLayer.is_enabled(cropping));
}

#[test]
fn animation_steps_reach_their_target_without_overshooting() {
    assert_eq!(step_toward(0.0, 1.0, 0.06, 0.12), 0.5);
    assert_eq!(step_toward(0.9, 1.0, 0.06, 0.12), 1.0);
    assert_eq!(step_toward(0.1, 0.0, 0.06, 0.09), 0.0);
    assert_eq!(step_toward(1.0, 1.0, 0.06, 0.12), 1.0);
}
