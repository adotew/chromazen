use super::*;

fn slider_frame(
    context: &egui::Context,
    value: &mut f32,
    events: impl IntoIterator<Item = egui::Event>,
    focused: bool,
) -> (egui::Rect, bool, Vec<egui::epaint::ClippedShape>) {
    let mut result = (egui::Rect::NOTHING, false);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 400.0),
            )),
            events: events.into_iter().collect(),
            focused,
            ..Default::default()
        },
        |ui| {
            let response = egui::Grid::new("test slider")
                .show(ui, |ui| {
                    panel_slider(ui, Some("Opacity"), value, 0.01..=1.0, false, true)
                })
                .inner;
            result = (response.rect, slider_preview_active(&response));
        },
    );
    (result.0, result.1, output.shapes)
}

fn slider_event(context: &egui::Context, value: &mut f32, event: egui::Event) -> bool {
    slider_frame(context, value, [event], true).1
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn slider_value_bubble_stays_above_the_thumb_and_within_horizontal_bounds() {
    let bounds = egui::Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(250.0, 400.0));
    let size = egui::vec2(60.0, 24.0);
    for x in [bounds.left(), bounds.center().x, bounds.right()] {
        let rect = slider_value_bubble_rect(egui::pos2(x, 80.0), size, bounds);
        assert!(bounds.contains_rect(rect));
        assert_eq!(rect.bottom(), 70.0);
    }
    let rect = slider_value_bubble_rect(egui::pos2(120.0, 10.0), size, bounds);
    assert_eq!(rect.top(), bounds.top());
}

#[test]
fn slider_value_is_hidden_at_rest_and_shown_on_hover_drag_and_focus() {
    let context = egui::Context::default();
    let mut value = 0.5;
    let has_value = |shapes: &[egui::epaint::ClippedShape]| {
        shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().ends_with('%'))
            })
    };
    let (rect, active, shapes) = slider_frame(&context, &mut value, [], true);
    assert!(!active);
    assert!(!has_value(&shapes));

    let (_, active, shapes) = slider_frame(
        &context,
        &mut value,
        [egui::Event::PointerMoved(rect.center())],
        true,
    );
    assert!(!active);
    assert!(has_value(&shapes));
    let (_, active, shapes) = slider_frame(
        &context,
        &mut value,
        [pointer_button(rect.center(), true)],
        true,
    );
    assert!(active);
    assert!(has_value(&shapes));

    let outside = egui::pos2(rect.right() + 80.0, rect.center().y);
    let (_, active, shapes) = slider_frame(
        &context,
        &mut value,
        [egui::Event::PointerMoved(outside)],
        true,
    );
    assert!(active);
    assert!(has_value(&shapes));
    assert_eq!(value, 1.0);
    let (_, active, shapes) =
        slider_frame(&context, &mut value, [pointer_button(outside, false)], true);
    assert!(!active);
    assert!(has_value(&shapes));
    assert!(!has_value(&slider_frame(&context, &mut value, [], false).2));
}

#[test]
fn slider_fill_fraction_matches_linear_and_quadratic_values() {
    assert_eq!(slider_fraction(0.5, &(0.0..=1.0), false), 0.5);
    assert!((slider_fraction(25.75, &(1.0..=100.0), true) - 0.5).abs() < f32::EPSILON);
    assert!((slider_value(0.5, &(1.0..=100.0), true) - 25.75).abs() < 0.0001);
}

#[test]
fn slider_drag_keyboard_and_focus_behavior_is_preserved() {
    let context = egui::Context::default();
    let mut value = 0.5;
    let rect = slider_frame(&context, &mut value, [], true).0;
    slider_event(
        &context,
        &mut value,
        egui::Event::PointerMoved(rect.center()),
    );
    assert!(slider_event(
        &context,
        &mut value,
        pointer_button(rect.center(), true)
    ));
    let outside = egui::pos2(rect.right() + 80.0, rect.center().y);
    assert!(slider_event(
        &context,
        &mut value,
        egui::Event::PointerMoved(outside)
    ));
    assert_eq!(value, 1.0);
    assert!(!slider_event(
        &context,
        &mut value,
        pointer_button(outside, false)
    ));
    assert!(!slider_frame(&context, &mut value, [], true).1);

    value = 0.5;
    let before = value;
    assert!(slider_event(
        &context,
        &mut value,
        egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    ));
    assert!(value > before);
    assert!(!slider_frame(&context, &mut value, [], false).1);
}

#[test]
fn panel_slider_updates_value_and_previews_keyboard_adjustment() {
    let context = egui::Context::default();
    let mut value = 25.75;
    let mut slider_id = None;
    for pressed in [false, true] {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 100.0),
                )),
                events: if pressed {
                    vec![egui::Event::Key {
                        key: egui::Key::ArrowRight,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                } else {
                    vec![]
                },
                focused: true,
                ..Default::default()
            },
            |ui| {
                egui::Grid::new("test controls").show(ui, |ui| {
                    let response =
                        panel_slider(ui, Some("Size"), &mut value, 1.0..=100.0, true, true);
                    if let Some(id) = slider_id {
                        assert_eq!(response.id, id);
                    }
                    slider_id = Some(response.id);
                    if pressed {
                        assert!(response.changed());
                        assert!(slider_preview_active(&response));
                    } else {
                        response.request_focus();
                    }
                });
            },
        );
        if pressed {
            assert!(output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().ends_with(" px"))
                }));
        }
    }
    assert!(value > 25.75);
}

#[test]
fn adjustment_preview_uses_correct_position_and_lifetime() {
    let workspace = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(810.0, 620.0));
    assert_eq!(
        adjustment_preview_center(true, None, true, workspace, 2.0),
        Some([820.0, 640.0])
    );
    assert_eq!(
        adjustment_preview_center(true, Some([73.0, 91.0]), true, workspace, 2.0),
        Some([73.0, 91.0])
    );
    assert_eq!(
        adjustment_preview_center(true, None, false, workspace, 1.0),
        None
    );
    assert_eq!(
        adjustment_preview_center(false, Some([20.0, 30.0]), true, workspace, 1.0),
        None
    );
}
