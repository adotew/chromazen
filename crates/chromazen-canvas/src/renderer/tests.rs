use super::*;

#[test]
fn arbitrary_canvas_resize_maps_the_intersection_into_the_new_canvas() {
    assert_eq!(
        canvas_copy_for_resize([100, 80], [60, 50], [25, 10]),
        Some(CanvasResizeCopy {
            source: TextureRect {
                x: 25,
                y: 10,
                width: 60,
                height: 50,
            },
            destination: [0, 0],
        })
    );
    assert_eq!(
        canvas_copy_for_resize([100, 80], [140, 120], [-20, -30]),
        Some(CanvasResizeCopy {
            source: TextureRect {
                x: 0,
                y: 0,
                width: 100,
                height: 80,
            },
            destination: [20, 30],
        })
    );
}

#[test]
fn canvas_resize_can_create_a_blank_canvas_outside_the_old_bounds() {
    assert_eq!(canvas_copy_for_resize([100, 80], [20, 20], [120, 90]), None);
}

#[test]
fn visible_canvas_rect_clips_to_the_surface() {
    let view = PaintViewSnapshot {
        zoom: 2.0,
        center: [27.25, 45.25],
        workspace_center: [27.25, 45.25],
        viewport_center: [75.0, 50.0],
        rotation: 0.0,
        flip: [1.0, 1.0],
    };

    assert_eq!(
        visible_canvas_rect(view, [100, 80], [150, 100]),
        Some(TextureRect {
            x: 20,
            y: 0,
            width: 130,
            height: 100,
        })
    );
}

#[test]
fn rotated_canvas_scissor_uses_all_four_corners() {
    let view = PaintViewSnapshot {
        zoom: 1.0,
        center: [50.0, 25.0],
        workspace_center: [50.0, 25.0],
        viewport_center: [100.0, 100.0],
        rotation: std::f32::consts::FRAC_PI_2,
        flip: [1.0, 1.0],
    };

    assert_eq!(
        visible_canvas_rect(view, [100, 50], [200, 200]),
        Some(TextureRect {
            x: 75,
            y: 50,
            width: 50,
            height: 100,
        })
    );
}

#[test]
fn canvas_outside_surface_has_no_scissor_rect() {
    let view = PaintViewSnapshot {
        zoom: 1.0,
        center: [250.0, 250.0],
        workspace_center: [250.0, 250.0],
        viewport_center: [50.0, 50.0],
        rotation: 0.0,
        flip: [1.0, 1.0],
    };

    assert_eq!(visible_canvas_rect(view, [100, 100], [100, 100]), None);
}

#[test]
fn persisted_layer_names_advance_the_default_number() {
    let layers = vec![
        LayerInfo {
            id: LayerId(1),
            name: "Layer 4".to_owned(),
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: BlendMode::Normal,
        },
        LayerInfo {
            id: LayerId(2),
            name: "Reference".to_owned(),
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: BlendMode::Normal,
        },
    ];
    assert_eq!(next_layer_number(&layers), 5);
}

#[test]
fn canvas_document_rejects_invalid_layer_metadata() {
    let mut document = CanvasDocument {
        size: [20, 30],
        background: [255; 3],
        selected_layer: LayerId(1),
        layers: vec![LayerInfo {
            id: LayerId(1),
            name: "Paint".to_owned(),
            visible: true,
            opacity: 100,
            clipped: false,
            blend_mode: BlendMode::Normal,
        }],
    };
    assert!(document.validate().is_ok());

    document.layers[0].opacity = 101;
    assert!(document.validate().is_err());
}

#[test]
fn document_background_round_trips_as_rgb8() {
    assert_eq!(rgb8(opaque_color([12, 34, 56])), [12, 34, 56]);
}

#[test]
fn brush_and_eraser_keep_preset_spacing() {
    let spacing = BrushSpacing {
        ratio: 0.03,
        minimum: 2.0,
    };

    assert_eq!(effective_spacing(PaintTool::Brush, spacing), spacing);
    assert_eq!(effective_spacing(PaintTool::Eraser, spacing), spacing);
}

#[test]
fn smudge_raises_dense_spacing_to_its_minimum_ratio() {
    let spacing = effective_spacing(
        PaintTool::Smudge,
        BrushSpacing {
            ratio: 0.03,
            minimum: 1.0,
        },
    );

    assert_eq!(spacing.ratio, SMUDGE_MIN_STEP_RATIO);
    assert_eq!(spacing.minimum, 1.0);
}

#[test]
fn smudge_keeps_coarser_preset_spacing() {
    let spacing = BrushSpacing {
        ratio: 0.25,
        minimum: 3.0,
    };

    assert_eq!(effective_spacing(PaintTool::Smudge, spacing), spacing);
}

#[test]
fn active_stroke_captures_layer_tool_and_premultiplied_color() {
    let stroke = ActiveStroke::new(
        LayerId(7),
        PaintTool::Brush,
        [0.8, 0.4, 0.2, 0.5],
        0.75,
        ColorMixing::Digital,
    );

    assert_eq!(stroke.layer_id, LayerId(7));
    assert_eq!(stroke.tool, PaintTool::Brush);
    assert_eq!(stroke.color, [0.4, 0.2, 0.1, 0.5]);
    assert_eq!(stroke.opacity, 0.75);
}

#[test]
fn stroke_uniform_matches_wgsl_struct_size() {
    // `Stroke { color: vec4f, colorMixing: u32 }` rounds up to its 16-byte alignment.
    assert_eq!(std::mem::size_of::<StrokeUniform>(), 32);
    let uniform = StrokeUniform::new([0.0; 4], ColorMixing::Pigment);
    assert_eq!(bytemuck::bytes_of(&uniform)[16..20], 1u32.to_ne_bytes());
}

#[test]
fn tool_opacity_scales_mask_strength_but_not_smudge_dab_strength() {
    let point = StrokePoint {
        x: 0.0,
        y: 0.0,
        radius: 10.0,
        opacity: 0.8,
    };
    let color = [0.0, 0.0, 0.0, 1.0];

    assert_eq!(
        ActiveStroke::new(
            LayerId(1),
            PaintTool::Brush,
            color,
            0.25,
            ColorMixing::Digital
        )
        .stamp_point(point)
        .opacity,
        0.2
    );
    assert_eq!(
        ActiveStroke::new(
            LayerId(1),
            PaintTool::Smudge,
            color,
            0.25,
            ColorMixing::Digital
        )
        .stamp_point(point)
        .opacity,
        0.8
    );
}

#[test]
fn alpha_bounds_wrap_visible_pixels() {
    let mut image = image::RgbaImage::new(8, 6);
    assert_eq!(alpha_content_bounds(&image, None), None);
    image.put_pixel(2, 4, image::Rgba([1, 2, 3, 1]));
    image.put_pixel(6, 1, image::Rgba([1, 2, 3, 255]));
    assert_eq!(
        alpha_content_bounds(&image, None),
        Some(LayerContentBounds {
            min: [2.0, 1.0],
            max: [7.0, 5.0],
        })
    );
}

#[test]
fn alpha_bounds_ignore_pixels_outside_the_selection() {
    let mut image = image::RgbaImage::new(8, 8);
    image.put_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
    image.put_pixel(5, 6, image::Rgba([0, 0, 0, 255]));
    let selection = selection::rasterize(&[[4.0, 4.0], [8.0, 4.0], [8.0, 8.0], [4.0, 8.0]], [8, 8]);
    assert_eq!(
        alpha_content_bounds(&image, selection.as_ref()),
        Some(LayerContentBounds {
            min: [5.0, 6.0],
            max: [6.0, 7.0],
        })
    );
}

#[test]
fn canvas_size_constraints_reject_invalid_dimensions() {
    let constraints = CanvasSizeConstraints {
        max_dimension: 8192,
        max_pixels: 32 * 1024 * 1024,
    };

    assert!(constraints.validate(DEFAULT_CANVAS_SIZE).is_ok());
    assert!(constraints.validate([0, 100]).is_err());
    assert!(constraints.validate([8193, 100]).is_err());
    assert!(constraints.validate([8192, 8192]).is_err());
    assert!(constraints.validate([8192, 4096]).is_ok());
}

#[test]
fn brush_stamps_must_fit_gpu_limits() {
    assert!(validate_brush_stamp(&image::RgbaImage::new(1, 1), 8).is_ok());
    assert!(validate_brush_stamp(&image::RgbaImage::new(0, 1), 8).is_err());
    assert!(validate_brush_stamp(&image::RgbaImage::new(9, 1), 8).is_err());
}

#[test]
fn layer_transform_maps_destination_back_to_source() {
    fn map(uniform: TransformUniform, point: [f32; 2]) -> [f32; 2] {
        [
            uniform.source_from_destination_x[0] * point[0]
                + uniform.source_from_destination_x[1] * point[1]
                + uniform.source_from_destination_x[2],
            uniform.source_from_destination_y[0] * point[0]
                + uniform.source_from_destination_y[1] * point[1]
                + uniform.source_from_destination_y[2],
        ]
    }

    let identity = LayerTransform::default().uniform([50.0, 40.0]);
    assert_eq!(map(identity, [20.5, 30.5]), [20.5, 30.5]);

    let translated = LayerTransform {
        translation: [10.0, -5.0],
        ..Default::default()
    }
    .uniform([50.0, 40.0]);
    assert_eq!(map(translated, [30.5, 25.5]), [20.5, 30.5]);

    let scaled = LayerTransform {
        scale: [2.0, 4.0],
        ..Default::default()
    }
    .uniform([50.0, 40.0]);
    assert_eq!(map(scaled, [70.0, 80.0]), [60.0, 50.0]);

    let rotated = LayerTransform {
        rotation: std::f32::consts::FRAC_PI_2,
        ..Default::default()
    }
    .uniform([50.0, 40.0]);
    let source = map(rotated, [50.0, 50.0]);
    assert!((source[0] - 60.0).abs() < 0.0001);
    assert!((source[1] - 40.0).abs() < 0.0001);
}

#[test]
fn layer_transform_map_point_inverts_the_shader_mapping() {
    let transform = LayerTransform {
        translation: [30.0, -12.0],
        scale: [0.5, 2.0],
        rotation: 0.7,
    };
    let pivot = [200.0, 150.0];
    let uniform = transform.uniform(pivot);
    for point in [[0.0, 0.0], [640.0, 20.0], [205.5, 149.0]] {
        let destination = transform.map_point(pivot, point);
        let source = [
            uniform.source_from_destination_x[0] * destination[0]
                + uniform.source_from_destination_x[1] * destination[1]
                + uniform.source_from_destination_x[2],
            uniform.source_from_destination_y[0] * destination[0]
                + uniform.source_from_destination_y[1] * destination[1]
                + uniform.source_from_destination_y[2],
        ];
        assert!((source[0] - point[0]).abs() < 1e-3 && (source[1] - point[1]).abs() < 1e-3);
    }
}

#[test]
fn transformed_points_cover_padded_document_pixels() {
    assert_eq!(
        pixel_rect_covering(&[[10.2, 5.0], [30.5, 40.9]], 1.0, [100, 100]),
        Some(TextureRect {
            x: 9,
            y: 4,
            width: 23,
            height: 38,
        })
    );
    assert_eq!(
        pixel_rect_covering(&[[-50.0, -50.0], [-10.0, -5.0]], 1.0, [100, 100]),
        None
    );
}

#[test]
fn content_bounds_cover_whole_pixels() {
    let bounds = LayerContentBounds {
        min: [3.0, 4.5],
        max: [10.0, 11.2],
    };
    assert_eq!(
        bounds.pixel_rect(),
        TextureRect {
            x: 3,
            y: 4,
            width: 7,
            height: 8,
        }
    );
}

#[test]
fn invalid_layer_transforms_are_rejected_and_scale_is_clamped() {
    assert!(
        LayerTransform {
            scale: [0.0, 1.0],
            ..Default::default()
        }
        .normalized()
        .is_none()
    );
    assert!(
        LayerTransform {
            rotation: f32::NAN,
            ..Default::default()
        }
        .normalized()
        .is_none()
    );
    assert_eq!(
        LayerTransform {
            scale: [1_000.0, 0.001],
            ..Default::default()
        }
        .normalized()
        .unwrap()
        .scale,
        [LayerTransform::MAX_SCALE, LayerTransform::MIN_SCALE]
    );
}

#[test]
fn brush_and_eraser_use_masks_while_smudge_is_direct() {
    let color = [0.0, 0.0, 0.0, 1.0];

    assert_eq!(
        ActiveStroke::new(
            LayerId(1),
            PaintTool::Brush,
            color,
            1.0,
            ColorMixing::Digital
        )
        .render_path(),
        StrokeRenderPath::Mask
    );
    assert_eq!(
        ActiveStroke::new(
            LayerId(1),
            PaintTool::Eraser,
            color,
            1.0,
            ColorMixing::Digital
        )
        .render_path(),
        StrokeRenderPath::Mask
    );
    assert_eq!(
        ActiveStroke::new(
            LayerId(1),
            PaintTool::Smudge,
            color,
            1.0,
            ColorMixing::Digital
        )
        .render_path(),
        StrokeRenderPath::DirectSmudge
    );
}
