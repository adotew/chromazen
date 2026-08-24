use super::*;

pub(super) fn canvas_crop_screen_corners(
    rect: CanvasCropRect,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [egui::Pos2; 4] {
    [
        [rect.min[0], rect.min[1]],
        [rect.max[0], rect.min[1]],
        [rect.max[0], rect.max[1]],
        [rect.min[0], rect.max[1]],
    ]
    .map(|point| {
        let point = view.document_to_window(point);
        egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point)
    })
}

pub(super) fn canvas_crop_handle_positions(
    corners: [egui::Pos2; 4],
) -> [(CanvasCropHandle, egui::Pos2); 8] {
    let midpoint = |a: egui::Pos2, b: egui::Pos2| a + (b - a) * 0.5;
    [
        (CanvasCropHandle::TopLeft, corners[0]),
        (CanvasCropHandle::Top, midpoint(corners[0], corners[1])),
        (CanvasCropHandle::TopRight, corners[1]),
        (CanvasCropHandle::Right, midpoint(corners[1], corners[2])),
        (CanvasCropHandle::BottomRight, corners[2]),
        (CanvasCropHandle::Bottom, midpoint(corners[2], corners[3])),
        (CanvasCropHandle::BottomLeft, corners[3]),
        (CanvasCropHandle::Left, midpoint(corners[3], corners[0])),
    ]
}

pub(super) fn paint_resize_handle_markers(
    painter: &egui::Painter,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) {
    for (_, position) in handles {
        let handle_rect = egui::Rect::from_center_size(*position, egui::Vec2::splat(14.0));
        painter.rect_filled(handle_rect, 3.0, egui::Color32::from_gray(232));
        painter.rect_stroke(
            handle_rect,
            2.0,
            egui::Stroke::new(1.5, egui::Color32::from_gray(72)),
            egui::StrokeKind::Inside,
        );
    }
}

pub(super) fn layer_transform_screen_corners(
    bounds: LayerContentBounds,
    transform: LayerTransform,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [egui::Pos2; 4] {
    layer_transform_document_corners(bounds, transform).map(|point| {
        let point = view.document_to_window(point);
        egui::pos2(point[0] / pixels_per_point, point[1] / pixels_per_point)
    })
}

pub(super) fn layer_transform_document_corners(
    bounds: LayerContentBounds,
    transform: LayerTransform,
) -> [[f32; 2]; 4] {
    let pivot = [
        (bounds.min[0] + bounds.max[0]) * 0.5,
        (bounds.min[1] + bounds.max[1]) * 0.5,
    ];
    let (sin, cos) = transform.rotation.sin_cos();
    [
        bounds.min,
        [bounds.max[0], bounds.min[1]],
        bounds.max,
        [bounds.min[0], bounds.max[1]],
    ]
    .map(|point| {
        let x = (point[0] - pivot[0]) * transform.scale[0];
        let y = (point[1] - pivot[1]) * transform.scale[1];
        [
            pivot[0] + transform.translation[0] + cos * x - sin * y,
            pivot[1] + transform.translation[1] + sin * x + cos * y,
        ]
    })
}

pub(super) fn layer_rotation_handle(corners: [egui::Pos2; 4]) -> egui::Pos2 {
    let top_center = corners[0] + (corners[1] - corners[0]) * 0.5;
    let center = corners[0] + (corners[2] - corners[0]) * 0.5;
    top_center + (top_center - center).normalized() * 36.0
}

pub(super) fn pointer_document_position(
    pointer: egui::Pos2,
    view: PaintViewSnapshot,
    pixels_per_point: f32,
) -> [f32; 2] {
    view.window_to_document([pointer.x * pixels_per_point, pointer.y * pixels_per_point])
}

pub(super) fn canvas_crop_handle_at(
    pointer: egui::Pos2,
    pointer_document: [f32; 2],
    rect: CanvasCropRect,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    resize_handle_at(pointer, handles)
        .or_else(|| resize_edge_at(pointer, handles))
        .or_else(|| {
            rect.contains(pointer_document)
                .then_some(CanvasCropHandle::Move)
        })
}

pub(super) fn resize_handle_at(
    pointer: egui::Pos2,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    const CORNER_HIT_RADIUS: f32 = 28.0;
    const HANDLE_HIT_RADIUS: f32 = 24.0;
    let nearest = |corner_only: bool, radius: f32| {
        handles
            .iter()
            .filter(|(handle, _)| !corner_only || canvas_crop_handle_is_corner(*handle))
            .filter_map(|(handle, position)| {
                let distance = position.distance_sq(pointer);
                (distance <= radius * radius).then_some((*handle, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(handle, _)| handle)
    };
    nearest(true, CORNER_HIT_RADIUS).or_else(|| nearest(false, HANDLE_HIT_RADIUS))
}

pub(super) fn resize_edge_at(
    pointer: egui::Pos2,
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
) -> Option<CanvasCropHandle> {
    const EDGE_HIT_RADIUS: f32 = 14.0;
    [
        (CanvasCropHandle::Top, handles[0].1, handles[2].1),
        (CanvasCropHandle::Right, handles[2].1, handles[4].1),
        (CanvasCropHandle::Bottom, handles[6].1, handles[4].1),
        (CanvasCropHandle::Left, handles[0].1, handles[6].1),
    ]
    .into_iter()
    .map(|(handle, start, end)| (handle, point_segment_distance_sq(pointer, start, end)))
    .filter(|(_, distance)| *distance <= EDGE_HIT_RADIUS * EDGE_HIT_RADIUS)
    .min_by(|left, right| left.1.total_cmp(&right.1))
    .map(|(handle, _)| handle)
}

pub(super) fn layer_transform_handle_at(
    pointer: egui::Pos2,
    corners: [egui::Pos2; 4],
    handles: &[(CanvasCropHandle, egui::Pos2); 8],
    rotation_handle: egui::Pos2,
) -> Option<LayerTransformHandle> {
    (pointer.distance_sq(rotation_handle) <= 18.0 * 18.0)
        .then_some(LayerTransformHandle::Rotate)
        .or_else(|| resize_handle_at(pointer, handles).map(LayerTransformHandle::Scale))
        .or_else(|| resize_edge_at(pointer, handles).map(LayerTransformHandle::Scale))
        .or_else(|| point_in_quad(pointer, corners).then_some(LayerTransformHandle::Move))
}

pub(super) fn point_in_quad(point: egui::Pos2, corners: [egui::Pos2; 4]) -> bool {
    let crosses = std::array::from_fn::<_, 4, _>(|index| {
        let start = corners[index];
        let edge = corners[(index + 1) % 4] - start;
        let offset = point - start;
        edge.x * offset.y - edge.y * offset.x
    });
    crosses.iter().all(|cross| *cross >= 0.0) || crosses.iter().all(|cross| *cross <= 0.0)
}

pub(super) fn layer_transform_from_drag(
    drag: LayerTransformDrag,
    pointer: [f32; 2],
    bounds: LayerContentBounds,
    preserve_aspect: bool,
) -> LayerTransform {
    let pivot = [
        (bounds.min[0] + bounds.max[0]) * 0.5 + drag.start_transform.translation[0],
        (bounds.min[1] + bounds.max[1]) * 0.5 + drag.start_transform.translation[1],
    ];
    match drag.handle {
        LayerTransformHandle::Move => LayerTransform {
            translation: [
                drag.start_transform.translation[0] + pointer[0] - drag.start_pointer[0],
                drag.start_transform.translation[1] + pointer[1] - drag.start_pointer[1],
            ],
            ..drag.start_transform
        },
        LayerTransformHandle::Scale(handle) => {
            let mut scale = drag.start_transform.scale;
            if preserve_aspect {
                let distance = |point: [f32; 2]| (point[0] - pivot[0]).hypot(point[1] - pivot[1]);
                let factor = distance(pointer) / distance(drag.start_pointer).max(f32::EPSILON);
                scale = scale.map(|value| (value * factor).max(0.01));
            } else {
                let delta = [pointer[0] - pivot[0], pointer[1] - pivot[1]];
                let (sin, cos) = drag.start_transform.rotation.sin_cos();
                let local = [
                    cos * delta[0] + sin * delta[1],
                    -sin * delta[0] + cos * delta[1],
                ];
                let half = [
                    (bounds.max[0] - bounds.min[0]) * 0.5,
                    (bounds.max[1] - bounds.min[1]) * 0.5,
                ];
                if matches!(
                    handle,
                    CanvasCropHandle::Left
                        | CanvasCropHandle::TopLeft
                        | CanvasCropHandle::BottomLeft
                ) {
                    scale[0] = (-local[0] / half[0]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Right
                        | CanvasCropHandle::TopRight
                        | CanvasCropHandle::BottomRight
                ) {
                    scale[0] = (local[0] / half[0]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Top | CanvasCropHandle::TopLeft | CanvasCropHandle::TopRight
                ) {
                    scale[1] = (-local[1] / half[1]).max(0.01);
                }
                if matches!(
                    handle,
                    CanvasCropHandle::Bottom
                        | CanvasCropHandle::BottomLeft
                        | CanvasCropHandle::BottomRight
                ) {
                    scale[1] = (local[1] / half[1]).max(0.01);
                }
            }
            LayerTransform {
                scale,
                ..drag.start_transform
            }
        }
        LayerTransformHandle::Rotate => LayerTransform {
            rotation: drag.start_transform.rotation
                + angle_delta(
                    pointer_angle(pivot, pointer),
                    pointer_angle(pivot, drag.start_pointer),
                ),
            ..drag.start_transform
        },
    }
}

pub(super) fn pointer_angle(center: [f32; 2], point: [f32; 2]) -> f32 {
    (point[1] - center[1]).atan2(point[0] - center[0])
}

pub(super) fn angle_delta(angle: f32, origin: f32) -> f32 {
    (angle - origin + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

pub(super) fn canvas_crop_handle_is_corner(handle: CanvasCropHandle) -> bool {
    matches!(
        handle,
        CanvasCropHandle::TopLeft
            | CanvasCropHandle::TopRight
            | CanvasCropHandle::BottomLeft
            | CanvasCropHandle::BottomRight
    )
}

pub(super) fn point_segment_distance_sq(
    point: egui::Pos2,
    start: egui::Pos2,
    end: egui::Pos2,
) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return point.distance_sq(start);
    }
    let projection = ((point - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    point.distance_sq(start + segment * projection)
}

pub(super) fn canvas_crop_cursor(handle: CanvasCropHandle) -> egui::CursorIcon {
    match handle {
        CanvasCropHandle::Move => egui::CursorIcon::Grab,
        CanvasCropHandle::Left | CanvasCropHandle::Right => egui::CursorIcon::ResizeHorizontal,
        CanvasCropHandle::Top | CanvasCropHandle::Bottom => egui::CursorIcon::ResizeVertical,
        CanvasCropHandle::TopLeft | CanvasCropHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
        CanvasCropHandle::TopRight | CanvasCropHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

pub(super) fn layer_transform_cursor(handle: LayerTransformHandle) -> egui::CursorIcon {
    match handle {
        LayerTransformHandle::Move => egui::CursorIcon::Grab,
        LayerTransformHandle::Scale(handle) => canvas_crop_cursor(handle),
        LayerTransformHandle::Rotate => egui::CursorIcon::Crosshair,
    }
}

pub(super) fn canvas_crop_rect_from_drag(
    drag: CanvasCropDrag,
    pointer: [f32; 2],
) -> CanvasCropRect {
    let delta = [
        pointer[0] - drag.start_pointer[0],
        pointer[1] - drag.start_pointer[1],
    ];
    let mut rect = drag.start_rect;
    if drag.handle == CanvasCropHandle::Move {
        rect.min[0] += delta[0];
        rect.max[0] += delta[0];
        rect.min[1] += delta[1];
        rect.max[1] += delta[1];
        return rect;
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Left | CanvasCropHandle::TopLeft | CanvasCropHandle::BottomLeft
    ) {
        rect.min[0] = (drag.start_rect.min[0] + delta[0]).min(drag.start_rect.max[0] - 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Right | CanvasCropHandle::TopRight | CanvasCropHandle::BottomRight
    ) {
        rect.max[0] = (drag.start_rect.max[0] + delta[0]).max(drag.start_rect.min[0] + 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Top | CanvasCropHandle::TopLeft | CanvasCropHandle::TopRight
    ) {
        rect.min[1] = (drag.start_rect.min[1] + delta[1]).min(drag.start_rect.max[1] - 1.0);
    }
    if matches!(
        drag.handle,
        CanvasCropHandle::Bottom | CanvasCropHandle::BottomLeft | CanvasCropHandle::BottomRight
    ) {
        rect.max[1] = (drag.start_rect.max[1] + delta[1]).max(drag.start_rect.min[1] + 1.0);
    }
    rect
}

pub(super) fn canvas_crop_request(
    rect: CanvasCropRect,
    constraints: CanvasSizeConstraints,
) -> Result<CanvasCropRequest, String> {
    if rect
        .min
        .into_iter()
        .chain(rect.max)
        .any(|value| !value.is_finite())
    {
        return Err("crop bounds must be finite".to_owned());
    }
    let left = rect.min[0].round() as i64;
    let top = rect.min[1].round() as i64;
    let right = rect.max[0].round() as i64;
    let bottom = rect.max[1].round() as i64;
    let width = right
        .checked_sub(left)
        .and_then(|width| u32::try_from(width).ok())
        .ok_or_else(|| "crop width must be at least 1 pixel".to_owned())?;
    let height = bottom
        .checked_sub(top)
        .and_then(|height| u32::try_from(height).ok())
        .ok_or_else(|| "crop height must be at least 1 pixel".to_owned())?;
    constraints.validate([width, height])?;
    let origin = [
        i32::try_from(left).map_err(|_| "crop origin is outside the supported range".to_owned())?,
        i32::try_from(top).map_err(|_| "crop origin is outside the supported range".to_owned())?,
    ];
    Ok(CanvasCropRequest {
        size: [width, height],
        origin,
    })
}
