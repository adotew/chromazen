use super::*;

#[test]
fn dialog_buttons_report_the_clicked_enabled_button() {
    let buttons = [
        DialogButton::new("Create", DialogButtonKind::Primary),
        DialogButton::new("Cancel", DialogButtonKind::Secondary),
    ];
    for (x, buttons, expected) in [
        (40.0, buttons, Some(0)),
        (260.0, buttons, Some(1)),
        (40.0, [buttons[0].enabled(false), buttons[1]], None),
    ] {
        let context = egui::Context::default();
        let position = egui::pos2(x, 18.0);
        let mut clicked = None;
        for pressed in [None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(position)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = context.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(DIALOG_WIDTH, 100.0),
                    )),
                    ..Default::default()
                },
                |ui| clicked = clicked.or(dialog_buttons(ui, &buttons)),
            );
        }
        assert_eq!(clicked, expected, "click at {x}");
    }
}

#[test]
fn macos_titlebar_space_tracks_fullscreen_transitions() {
    assert_eq!(has_macos_titlebar(false), cfg!(target_os = "macos"));
    assert!(!has_macos_titlebar(true));
    assert_eq!(has_macos_titlebar(false), cfg!(target_os = "macos"));
}

#[test]
fn workspace_background_follows_theme_only_for_standard() {
    assert_eq!(
        workspace_background_color(WorkspaceBackground::Standard, true),
        [0.16; 3]
    );
    assert_eq!(
        workspace_background_color(WorkspaceBackground::Standard, false),
        [0.82; 3]
    );
    for dark_mode in [true, false] {
        assert_eq!(
            workspace_background_color(WorkspaceBackground::NeutralGray, dark_mode),
            [0.5; 3]
        );
    }
}

#[test]
fn application_menu_disables_document_actions_without_artwork() {
    let state = ApplicationMenuState::new(false, false, true, true, false);

    assert!(!state.document_enabled);
    assert!(!state.can_undo);
    assert!(!state.can_redo);
    assert!(!state.canvas_enabled);
    assert!(!state.export_enabled);
}

#[test]
fn application_menu_respects_transient_editor_restrictions() {
    let cropping = ApplicationMenuState::new(true, true, true, true, false);
    assert!(cropping.document_enabled);
    assert!(!cropping.can_undo);
    assert!(!cropping.can_redo);
    assert!(!cropping.canvas_enabled);
    assert!(cropping.export_enabled);

    let exporting = ApplicationMenuState::new(true, false, true, false, true);
    assert!(exporting.can_undo);
    assert!(!exporting.can_redo);
    assert!(exporting.canvas_enabled);
    assert!(!exporting.export_enabled);
}

#[test]
fn error_dialog_hides_technical_details() {
    let dialog = MessageDialog::error(
        "Chromazen couldn’t export the PNG.",
        "renderer unavailable at adapter 0",
    );

    assert_eq!(dialog.title, "Something went wrong");
    assert_eq!(dialog.message, "Chromazen couldn’t export the PNG.");
    assert!(!dialog.message.contains("adapter 0"));
}

#[test]
fn crop_handles_have_precise_targets_and_entire_edges_are_draggable() {
    let corners = [
        egui::pos2(0.0, 0.0),
        egui::pos2(400.0, 0.0),
        egui::pos2(400.0, 200.0),
        egui::pos2(0.0, 200.0),
    ];
    let handles = canvas_crop_handle_positions(corners);
    let rect = CanvasCropRect {
        min: [0.0, 0.0],
        max: [400.0, 200.0],
    };

    assert_eq!(
        canvas_crop_handle_at(egui::pos2(-6.0, -6.0), [-6.0, -6.0], rect, &handles),
        Some(CanvasCropHandle::TopLeft)
    );
    assert_eq!(
        canvas_crop_handle_at(egui::pos2(200.0, 8.0), [200.0, 8.0], rect, &handles),
        Some(CanvasCropHandle::Top)
    );
    assert_eq!(
        canvas_crop_handle_at(egui::pos2(100.0, 4.0), [100.0, 4.0], rect, &handles),
        Some(CanvasCropHandle::Top)
    );
    assert_eq!(
        canvas_crop_handle_at(egui::pos2(20.0, 20.0), [20.0, 20.0], rect, &handles),
        Some(CanvasCropHandle::Move)
    );
    assert_eq!(
        canvas_crop_handle_at(egui::pos2(200.0, 100.0), [200.0, 100.0], rect, &handles),
        Some(CanvasCropHandle::Move)
    );
}

#[test]
fn small_transform_box_stays_movable_inside_and_outside() {
    let corners = [
        egui::pos2(100.0, 100.0),
        egui::pos2(124.0, 100.0),
        egui::pos2(124.0, 124.0),
        egui::pos2(100.0, 124.0),
    ];
    let handles = canvas_crop_handle_positions(corners);
    let rotation_handle = layer_rotation_handle(corners);
    let handle_at = |x, y| layer_transform_handle_at(egui::pos2(x, y), &handles, rotation_handle);

    assert_eq!(handle_at(112.0, 112.0), LayerTransformHandle::Move);
    assert_eq!(handle_at(300.0, 300.0), LayerTransformHandle::Move);
    assert_eq!(
        handle_at(98.0, 98.0),
        LayerTransformHandle::Scale(CanvasCropHandle::TopLeft)
    );
    assert_eq!(handle_at(112.0, 64.0), LayerTransformHandle::Rotate);
}

#[test]
fn transform_handles_move_scale_and_rotate_from_the_drag_origin() {
    let start = LayerTransform {
        translation: [10.0, -5.0],
        scale: [2.0, 2.0],
        rotation: 0.0,
    };
    let bounds = LayerContentBounds {
        min: [20.0, 10.0],
        max: [80.0, 90.0],
    };
    let drag = |handle, start_pointer| LayerTransformDrag {
        handle,
        start_transform: start,
        start_pointer,
    };

    assert_eq!(
        layer_transform_from_drag(
            drag(LayerTransformHandle::Move, [20.0, 30.0]),
            [35.0, 5.0],
            bounds,
            false,
        )
        .translation,
        [25.0, -30.0]
    );
    assert_eq!(
        layer_transform_from_drag(
            drag(
                LayerTransformHandle::Scale(CanvasCropHandle::TopRight),
                [120.0, -35.0],
            ),
            [150.0, -15.0],
            bounds,
            false,
        )
        .scale,
        [3.0, 1.5]
    );
    assert_eq!(
        layer_transform_from_drag(
            drag(
                LayerTransformHandle::Scale(CanvasCropHandle::TopRight),
                [120.0, -35.0],
            ),
            [180.0, -115.0],
            bounds,
            true,
        )
        .scale,
        [4.0, 4.0]
    );
    let rotated = layer_transform_from_drag(
        drag(LayerTransformHandle::Rotate, [110.0, 45.0]),
        [60.0, 95.0],
        bounds,
        false,
    );
    assert!((rotated.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
}

#[test]
fn crop_handles_resize_selected_edges_in_document_coordinates() {
    let start_rect = CanvasCropRect {
        min: [10.0, 20.0],
        max: [110.0, 220.0],
    };
    let drag = CanvasCropDrag {
        handle: CanvasCropHandle::TopLeft,
        start_rect,
        start_pointer: [10.0, 20.0],
    };
    assert_eq!(
        canvas_crop_rect_from_drag(drag, [-15.0, 45.0]),
        CanvasCropRect {
            min: [-15.0, 45.0],
            max: [110.0, 220.0],
        }
    );
}

#[test]
fn moving_crop_preserves_its_dimensions() {
    let start_rect = CanvasCropRect {
        min: [10.0, 20.0],
        max: [110.0, 220.0],
    };
    let drag = CanvasCropDrag {
        handle: CanvasCropHandle::Move,
        start_rect,
        start_pointer: [30.0, 40.0],
    };
    assert_eq!(
        canvas_crop_rect_from_drag(drag, [45.0, 10.0]),
        CanvasCropRect {
            min: [25.0, -10.0],
            max: [125.0, 190.0],
        }
    );
}

#[test]
fn crop_request_rounds_to_pixels_and_validates_renderer_limits() {
    let constraints = CanvasSizeConstraints {
        max_dimension: 8192,
        max_pixels: 32 * 1024 * 1024,
    };
    assert_eq!(
        canvas_crop_request(
            CanvasCropRect {
                min: [-10.4, 20.4],
                max: [90.6, 220.6],
            },
            constraints,
        ),
        Ok(CanvasCropRequest {
            size: [101, 201],
            origin: [-10, 20],
        })
    );
    assert!(
        canvas_crop_request(
            CanvasCropRect {
                min: [0.0, 0.0],
                max: [9000.0, 10.0],
            },
            constraints,
        )
        .is_err()
    );
}

#[test]
fn frosted_surfaces_share_screen_mapped_texture() {
    let fill = surface_fill(true, SurfaceStyle::Frosted);
    let first_rect = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(30.0, 40.0));
    let second_rect = egui::Rect::from_min_max(egui::pos2(50.0, 60.0), egui::pos2(80.0, 90.0));
    let mut shape = egui::Shape::Vec(vec![
        egui::Shape::rect_filled(first_rect, 8, fill),
        egui::Shape::rect_filled(second_rect, 8, fill),
    ]);
    let texture_id = egui::TextureId::User(42);

    let mut surface_rects = Vec::new();
    frost_shape(
        &mut shape,
        texture_id,
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
        fill,
        &[frosted_row_fill(true, false), frosted_row_fill(true, true)],
        &mut surface_rects,
    );
    assert_eq!(surface_rects, [first_rect, second_rect]);

    let egui::Shape::Vec(surfaces) = shape else {
        panic!("surface list");
    };
    for surface in surfaces {
        let egui::Shape::Vec(layers) = surface else {
            panic!("frosted surface layers");
        };
        let egui::Shape::Rect(backdrop) = &layers[0] else {
            panic!("frosted backdrop");
        };
        assert_eq!(
            backdrop
                .brush
                .as_ref()
                .expect("backdrop texture")
                .fill_texture_id,
            texture_id
        );
        let egui::Shape::Rect(tint) = &layers[1] else {
            panic!("surface tint");
        };
        assert_eq!(backdrop.rect, tint.rect);
        assert_eq!((backdrop.fill, tint.fill), frosted_layer_fills(fill));
        assert_eq!(tint.stroke, egui::Stroke::NONE);
        let egui::Shape::Rect(edge) = &layers[2] else {
            panic!("surface edge");
        };
        assert_eq!(edge.rect, tint.rect);
        assert_eq!(edge.stroke, frosted_surface_edge());
    }
}

#[test]
fn frosted_sheen_fades_from_the_top_left_and_stays_inside_the_surface() {
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 300.0));
    let [egui::Shape::Mesh(sheen), _] = frosted_surface_lighting(rect, 20.0.into()) else {
        panic!("sheen mesh");
    };
    let brightest = sheen
        .vertices
        .iter()
        .max_by_key(|vertex| vertex.color.a())
        .expect("sheen vertices");
    assert!(brightest.pos.x < rect.center().x && brightest.pos.y < rect.center().y);
    for vertex in &sheen.vertices {
        assert!(rect.expand(0.01).contains(vertex.pos));
        let progress = (vertex.pos.x - rect.min.x + vertex.pos.y - rect.min.y) / 400.0;
        assert!(progress <= 0.551, "sheen extends to {progress}");
    }
    assert!(sheen.vertices.iter().any(|vertex| vertex.color.a() == 0));
}

#[test]
fn frosted_surface_meshes_keep_their_shape_and_fringe() {
    let fill = surface_fill(true, SurfaceStyle::Frosted);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(10.0, 20.0), fill);
    mesh.colored_vertex(egui::pos2(50.0, 20.0), fill);
    mesh.colored_vertex(egui::pos2(30.0, 60.0), egui::Color32::TRANSPARENT);
    mesh.add_triangle(0, 1, 2);
    let mut shape = egui::Shape::mesh(mesh.clone());
    let texture_id = egui::TextureId::User(42);
    let mut surface_rects = Vec::new();

    frost_shape(
        &mut shape,
        texture_id,
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
        fill,
        &[],
        &mut surface_rects,
    );

    assert_eq!(surface_rects, [mesh.calc_bounds()]);
    let egui::Shape::Vec(layers) = shape else {
        panic!("frosted mesh layers");
    };
    let egui::Shape::Mesh(backdrop) = &layers[0] else {
        panic!("frosted mesh backdrop");
    };
    assert_eq!(backdrop.texture_id, texture_id);
    assert_eq!(backdrop.indices, mesh.indices);
    assert_eq!(backdrop.vertices[1].uv, egui::pos2(0.5, 0.2));
    assert_eq!(backdrop.vertices[0].color, egui::Color32::WHITE);
    assert_eq!(backdrop.vertices[2].color, egui::Color32::TRANSPARENT);
    assert_eq!(layers[1], egui::Shape::mesh(mesh));
}

#[test]
fn meshes_with_other_colors_are_not_frosted() {
    let fill = surface_fill(true, SurfaceStyle::Frosted);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(10.0, 20.0), fill);
    mesh.colored_vertex(egui::pos2(50.0, 20.0), egui::Color32::RED);
    mesh.colored_vertex(egui::pos2(30.0, 60.0), fill);
    mesh.add_triangle(0, 1, 2);
    let original = egui::Shape::mesh(mesh);
    let mut shape = original.clone();
    let mut surface_rects = Vec::new();

    frost_shape(
        &mut shape,
        egui::TextureId::User(42),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
        fill,
        &[],
        &mut surface_rects,
    );

    assert!(surface_rects.is_empty());
    assert_eq!(shape, original);
}

#[test]
fn frosted_row_uses_backdrop_without_starting_a_surface_batch() {
    let surface = surface_fill(true, SurfaceStyle::Frosted);
    let row_fill = frosted_row_fill(true, true);
    let mut shape = egui::Shape::rect_filled(
        egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(30.0, 40.0)),
        8,
        row_fill,
    );
    let mut surface_rects = Vec::new();

    frost_shape(
        &mut shape,
        egui::TextureId::User(42),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)),
        surface,
        &[frosted_row_fill(true, false), row_fill],
        &mut surface_rects,
    );

    assert!(surface_rects.is_empty());
    let egui::Shape::Vec(layers) = shape else {
        panic!("frosted row layers");
    };
    let egui::Shape::Rect(tint) = &layers[1] else {
        panic!("frosted row tint");
    };
    assert_eq!(tint.fill, frosted_layer_fills(row_fill).1);
    assert!(row_fill.r() > surface.r());
    assert!(frosted_row_fill(false, true).r() < surface_fill(false, SurfaceStyle::Frosted).r());
}

#[test]
fn frosted_slider_chrome_keeps_theme_contrast() {
    let [dark_track, dark_fill, ..] = brush_slider_fills(true, true);
    let [light_track, light_fill, ..] = brush_slider_fills(false, true);

    assert!(dark_track.r() < dark_fill.r());
    assert!(light_track.r() > light_fill.r());
    let over_midgray = |color: egui::Color32| {
        let alpha = u16::from(color.a());
        (u16::from(color.r()) * alpha + 128 * (255 - alpha)) / 255
    };
    for (dark_mode, track) in [(true, dark_track), (false, light_track)] {
        let panel = surface_fill(dark_mode, SurfaceStyle::Frosted);
        assert!(over_midgray(track).abs_diff(over_midgray(panel)) >= 16);
    }
    assert!(
        brush_slider_fills(true, true)
            .into_iter()
            .all(|fill| fill.a() < u8::MAX)
    );
    assert!(
        brush_slider_fills(true, false)
            .into_iter()
            .all(|fill| fill.a() == u8::MAX)
    );
}

#[test]
fn accent_color_updates_selection_style() {
    let mut style = egui::Style::default();
    let accent = egui::Color32::from_rgb(12, 34, 56);

    apply_accent_color_to_style(&mut style, accent);

    assert_eq!(style.visuals.selection.bg_fill, accent);
    assert_eq!(style.visuals.selection.stroke.color, egui::Color32::WHITE);
}

#[test]
fn monochrome_accent_contrasts_with_the_theme() {
    let mut dark = egui::Style::default();
    dark.visuals = egui::Visuals::dark();
    apply_accent_color_to_style(&mut dark, egui::Color32::WHITE);
    assert_eq!(dark.visuals.selection.bg_fill, egui::Color32::WHITE);
    assert_eq!(dark.visuals.selection.stroke.color, egui::Color32::BLACK);

    let mut light = egui::Style::default();
    light.visuals = egui::Visuals::light();
    apply_accent_color_to_style(&mut light, egui::Color32::WHITE);
    assert_eq!(light.visuals.selection.bg_fill, egui::Color32::BLACK);
    assert_eq!(light.visuals.selection.stroke.color, egui::Color32::WHITE);
}

#[test]
fn background_color_round_trips_through_ui() {
    let color = [0.25, 0.5, 0.75, 1.0];
    assert_eq!(rgb(background_color(color)), [64, 128, 191]);
}

#[test]
fn replaced_layer_resource_invalidates_cached_preview() {
    let cached = LayerPreviewKey {
        layer_id: LayerId(1),
        resource_id: LayerResourceId(4),
    };
    let replacement = LayerPreviewKey {
        layer_id: LayerId(1),
        resource_id: LayerResourceId(5),
    };
    assert!(!layer_preview_is_current(&[replacement], cached));
}

#[test]
fn unchanged_layer_resource_keeps_cached_preview() {
    let cached = LayerPreviewKey {
        layer_id: LayerId(2),
        resource_id: LayerResourceId(7),
    };
    assert!(layer_preview_is_current(&[cached], cached));
}

#[test]
fn reference_transform_uses_total_drag_delta_from_the_origin() {
    let position = [10.0, 20.0];
    let size = [100.0, 50.0];

    let (_, resized) =
        reference_transform_from_drag_origin(position, size, ReferenceDrag::Resize, [25.0, 20.0]);
    assert!((resized[0] - 128.0).abs() < 0.0001);
    assert!((resized[1] - 64.0).abs() < 0.0001);
    assert!((resized[0] / resized[1] - size[0] / size[1]).abs() < 0.0001);
    assert_eq!(
        reference_transform_from_drag_origin(position, size, ReferenceDrag::Move, [50.0, 25.0],),
        ([60.0, 45.0], size),
    );
}

#[test]
fn resize_handle_overlaps_the_corner_with_a_larger_invisible_hit_target() {
    let reference = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(120.0, 130.0));
    let (center, hit_rect) = reference_resize_handle_geometry(reference);

    assert_eq!(center, reference.right_bottom());
    assert_eq!(hit_rect.min, egui::pos2(106.0, 116.0));
    assert_eq!(hit_rect.max, egui::pos2(134.0, 144.0));
}

#[test]
fn outer_resize_handle_press_is_reference_input_without_image_layer() {
    let image = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(120.0, 130.0));
    let resize = reference_resize_handle_geometry(image).1;
    let outer_edge = egui::pos2(128.0, 138.0);

    assert!(!image.contains(outer_edge));
    assert!(reference_hit_at(outer_edge, &[image], Some(resize), false));
    assert!(!reference_hit_at(
        egui::pos2(50.0, 50.0),
        &[image],
        Some(resize),
        false,
    ));
    assert!(reference_hit_at(
        egui::pos2(50.0, 50.0),
        &[image],
        Some(resize),
        true,
    ));
    assert!(!reference_hit_at(
        egui::pos2(140.0, 138.0),
        &[image],
        Some(resize),
        false,
    ));
}

#[test]
fn reference_hover_is_limited_to_the_visible_workspace() {
    let reference = egui::Rect::from_min_max(egui::pos2(80.0, 20.0), egui::pos2(140.0, 80.0));
    let workspace = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(100.0, 100.0));

    assert!(pointer_over_visible_reference(
        Some(egui::pos2(90.0, 50.0)),
        reference,
        workspace,
    ));
    assert!(!pointer_over_visible_reference(
        Some(egui::pos2(120.0, 50.0)),
        reference,
        workspace,
    ));
}

#[test]
fn canvas_press_clears_reference_selection_without_affecting_ui_presses() {
    assert!(should_clear_reference_selection(
        true, true, false, false, false
    ));
    assert!(!should_clear_reference_selection(
        true, true, true, false, false
    ));
    assert!(!should_clear_reference_selection(
        true, true, false, true, false
    ));
    assert!(!should_clear_reference_selection(
        false, true, false, false, false
    ));
    assert!(!should_clear_reference_selection(
        true, true, false, false, true
    ));
}
