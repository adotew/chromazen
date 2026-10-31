use super::*;

#[test]
fn frost_is_refreshed_only_for_content_near_the_surface() {
    let first = ui::FrostedSurface {
        shape_index: 0,
        rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
    };
    let second = ui::FrostedSurface {
        shape_index: 2,
        rect: egui::Rect::from_min_max(egui::pos2(400.0, 0.0), egui::pos2(500.0, 100.0)),
    };
    let reference = egui::Rect::from_min_max(egui::pos2(420.0, 20.0), egui::pos2(480.0, 80.0));
    let distant = egui::Rect::from_min_max(egui::pos2(700.0, 20.0), egui::pos2(760.0, 80.0));

    assert!(frost_refresh_required(0, &first, &[], 2.0));
    assert!(frost_refresh_required(1, &second, &[reference], 2.0));
    assert!(!frost_refresh_required(1, &second, &[distant], 2.0));
}

#[test]
fn frost_is_refreshed_for_nearby_stacked_surfaces() {
    let first = ui::FrostedSurface {
        shape_index: 0,
        rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
    };
    let second = ui::FrostedSurface {
        shape_index: 1,
        rect: egui::Rect::from_min_max(egui::pos2(150.0, 0.0), egui::pos2(250.0, 100.0)),
    };

    assert!(frost_refresh_required(1, &second, &[first.rect], 1.0));
    assert!(!frost_refresh_required(1, &second, &[first.rect], 2.0));
}
