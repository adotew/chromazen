use super::*;

fn view(rotation: f32, flip: [f32; 2]) -> PaintViewSnapshot {
    PaintViewSnapshot {
        zoom: 2.0,
        center: [50.0, 40.0],
        workspace_center: [50.0, 40.0],
        viewport_center: [400.0, 300.0],
        rotation,
        flip,
    }
}

#[test]
fn document_and_window_coordinates_round_trip_when_rotated_and_flipped() {
    for rotation in [0.0, 0.37, std::f32::consts::FRAC_PI_2, -2.1] {
        for flip in [[1.0, 1.0], [-1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]] {
            let view = view(rotation, flip);
            let document = [72.0, -13.0];
            let round_trip = view.window_to_document(view.document_to_window(document));
            assert!((round_trip[0] - document[0]).abs() < 0.0001);
            assert!((round_trip[1] - document[1]).abs() < 0.0001);
        }
    }
}

#[test]
fn ninety_degree_rotation_maps_document_axes_to_window_axes() {
    let view = view(std::f32::consts::FRAC_PI_2, [1.0, 1.0]);
    assert_eq!(view.document_to_window([60.0, 40.0]), [400.0, 320.0]);
    assert_eq!(view.document_to_window([50.0, 50.0]), [380.0, 300.0]);
}

#[test]
fn cursor_centered_zoom_preserves_document_and_workspace_points() {
    let mut view = PaintView::default();
    view.set_surface_size([800, 600]);
    view.center = [200.0, 100.0];
    view.workspace_center = [-50.0, 75.0];
    view.rotation = 0.7;
    view.flip = [-1.0, 1.0];
    let cursor = [175.0, 480.0];
    let document_before = view.window_to_document(cursor);
    let workspace_before = view.snapshot().window_to_workspace(cursor);
    view.apply_zoom_at(1.1, cursor);
    let document_after = view.window_to_document(cursor);
    let workspace_after = view.snapshot().window_to_workspace(cursor);
    assert!((document_before[0] - document_after[0]).abs() < 0.0001);
    assert!((document_before[1] - document_after[1]).abs() < 0.0001);
    assert!((workspace_before[0] - workspace_after[0]).abs() < 0.0001);
    assert!((workspace_before[1] - workspace_after[1]).abs() < 0.0001);
}

#[test]
fn pan_moves_oriented_canvas_and_workspace_aids_by_same_screen_delta() {
    let mut view = PaintView {
        zoom: 1.3,
        center: [80.0, 60.0],
        workspace_center: [-20.0, 140.0],
        surface_size: [700.0, 500.0],
        rotation: 0.8,
        flip: [-1.0, 1.0],
    };
    let document_point = [120.0, 90.0];
    let workspace_point = [40.0, 200.0];
    let document_before = view.snapshot().document_to_window(document_point);
    let workspace_before = view.snapshot().workspace_to_window(workspace_point);
    let delta = [35.0, -22.0];

    view.pan_by_window_delta(delta);

    let document_after = view.snapshot().document_to_window(document_point);
    let workspace_after = view.snapshot().workspace_to_window(workspace_point);
    assert!((document_after[0] - document_before[0] - delta[0]).abs() < 0.0001);
    assert!((document_after[1] - document_before[1] - delta[1]).abs() < 0.0001);
    assert!((workspace_after[0] - workspace_before[0] - delta[0]).abs() < 0.0001);
    assert!((workspace_after[1] - workspace_before[1] - delta[1]).abs() < 0.0001);
}

#[test]
fn fit_accounts_for_rotated_document_bounds() {
    let mut view = PaintView {
        rotation: std::f32::consts::FRAC_PI_2,
        ..PaintView::default()
    };
    view.fit_to_screen([800, 400], [200, 100]);
    assert_eq!(view.zoom, 2.0);
    assert_eq!(view.center, [100.0, 50.0]);
}

#[test]
fn rotation_keeps_the_requested_document_anchor_stationary() {
    let mut view = PaintView {
        zoom: 1.7,
        center: [30.0, 20.0],
        surface_size: [800.0, 600.0],
        ..PaintView::default()
    };
    let canvas_center = [100.0, 50.0];
    let before = view.snapshot().document_to_window(canvas_center);

    assert!(view.set_rotation_around(0.73, canvas_center));

    let after = view.snapshot().document_to_window(canvas_center);
    assert!((before[0] - after[0]).abs() < 0.0001);
    assert!((before[1] - after[1]).abs() < 0.0001);
}

#[test]
fn workspace_aids_stay_stationary_during_canvas_orientation_changes() {
    let mut view = PaintView {
        zoom: 1.4,
        center: [30.0, 20.0],
        workspace_center: [30.0, 20.0],
        surface_size: [800.0, 600.0],
        ..PaintView::default()
    };
    let canvas_center = [100.0, 50.0];
    let reference_position = [220.0, -40.0];
    let before = view.snapshot().workspace_to_window(reference_position);

    view.set_rotation_around(0.91, canvas_center);
    view.toggle_flip_horizontal();
    view.toggle_flip_vertical();

    assert_eq!(
        view.snapshot().workspace_to_window(reference_position),
        before
    );
}

#[test]
fn flipping_mirrors_the_visible_region_across_the_viewport_center() {
    let mut view = PaintView {
        zoom: 0.8,
        center: [-40.0, 120.0],
        surface_size: [900.0, 500.0],
        rotation: 0.4,
        ..PaintView::default()
    };
    let document_point = [200.0, 150.0];
    let before = view.snapshot().document_to_window(document_point);

    view.toggle_flip_horizontal();
    let after = view.snapshot().document_to_window(document_point);
    assert!((after[0] - (900.0 - before[0])).abs() < 0.001);
    assert!((after[1] - before[1]).abs() < 0.001);

    view.toggle_flip_vertical();
    let after = view.snapshot().document_to_window(document_point);
    assert!((after[0] - (900.0 - before[0])).abs() < 0.001);
    assert!((after[1] - (500.0 - before[1])).abs() < 0.001);
}

#[test]
fn angle_normalization_avoids_unbounded_rotation() {
    assert!(normalize_angle(TAU).abs() < 0.000001);
    assert!(
        (normalize_angle(std::f32::consts::PI * 2.5) - std::f32::consts::FRAC_PI_2).abs() < 0.0001
    );
}
