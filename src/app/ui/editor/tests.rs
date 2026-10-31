use super::*;

#[test]
fn floating_panel_reports_the_size_it_was_restored_with() {
    let context = egui::Context::default();
    let size = egui::vec2(360.0, 500.0);
    let mut reported = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1200.0),
            )),
            ..Default::default()
        };
        let _ = context.run_ui(input, |ui| {
            reported = egui::Window::new("Layers")
                .default_size(size)
                .resizable(true)
                .show(ui.ctx(), |ui| ui.max_rect().size())
                .and_then(|response| response.inner);
        });
    }
    // Saving a larger size than was restored would grow the panel on every launch.
    assert_eq!(reported, Some(size));
}

#[test]
fn sidebar_frame_tracks_content_instead_of_window_height() {
    let context = egui::Context::default();
    for (window_height, content_height) in [(1600.0, 600.0), (2000.0, 300.0), (1200.0, 500.0)] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, window_height),
            )),
            ..Default::default()
        };
        let output = context.run_ui(input, |ui| {
            show_sidebar_frame(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), content_height),
                        egui::Sense::hover(),
                    );
                });
            });
        });
        let panel = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.corner_radius == egui::CornerRadius::same(28) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .expect("sidebar background");
        assert!((panel.width() - (SIDEBAR_WIDTH - 24.0)).abs() < 1.0);
        assert!((panel.height() - (content_height + 32.0)).abs() < 3.0);
    }
}
