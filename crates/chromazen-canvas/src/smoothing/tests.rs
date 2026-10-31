use super::*;

fn point(x: f32, y: f32) -> StrokePoint {
    StrokePoint {
        x,
        y,
        radius: 10.0,
        opacity: 1.0,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

fn max_polyline_error(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    emitted: &[StrokePoint],
) -> f32 {
    let mut polyline = Vec::with_capacity(emitted.len() + 1);
    polyline.push([p1.x, p1.y]);
    polyline.extend(emitted.iter().map(|point| [point.x, point.y]));

    (0..=4096)
        .map(|index| {
            let u = index as f32 / 4096.0;
            let curve_point = curve_position(p0, p1, p2, p3, u);
            polyline
                .windows(2)
                .map(|segment| distance_to_line_segment(curve_point, segment[0], segment[1]))
                .fold(f32::INFINITY, f32::min)
        })
        .fold(0.0, f32::max)
}

#[test]
fn position_filter_reduces_stationary_jitter() {
    let mut filter = StrokePositionFilter::default();
    filter.reset([0.0; 2], Duration::ZERO);
    let mut raw_squared_error = 0.0;
    let mut filtered_squared_error = 0.0;
    let mut noise_state = 0x1234_5678_u32;

    for index in 1..=240 {
        noise_state = noise_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let noise = (noise_state >> 8) as f32 / 8_388_607.5 - 1.0;
        let raw = [noise, -noise];
        let filtered = filter.filter(raw, FALLBACK_SAMPLE_INTERVAL * index);
        raw_squared_error += raw[0] * raw[0] + raw[1] * raw[1];
        filtered_squared_error += filtered[0] * filtered[0] + filtered[1] * filtered[1];
    }

    assert!(filtered_squared_error < raw_squared_error * 0.25);
}

#[test]
fn duplicate_filter_timestamps_remain_finite() {
    let mut filter = StrokePositionFilter::default();
    filter.reset([0.0; 2], Duration::ZERO);

    let filtered = filter.filter([10.0, 5.0], Duration::ZERO);

    assert!(filtered.into_iter().all(f32::is_finite));
}

#[test]
fn position_filter_is_stable_across_sample_rates() {
    fn replay(sample_rate: u32) -> f32 {
        let mut filter = StrokePositionFilter::default();
        filter.reset([0.0; 2], Duration::ZERO);
        let samples = sample_rate / 2;
        let mut output = [0.0; 2];
        for index in 1..=samples {
            let seconds = index as f32 / sample_rate as f32;
            output = filter.filter([seconds * 1_000.0, 0.0], Duration::from_secs_f32(seconds));
        }
        output[0]
    }

    assert!((replay(60) - replay(240)).abs() < 2.0);
}

#[test]
fn finish_at_replaces_the_filtered_tip() {
    let mut smoother = StrokeSmoother::default();
    smoother.begin(point(0.0, 0.0));
    smoother.push(point(8.0, 0.0));
    smoother.push(point(10.0, 0.0));

    let output = smoother.finish_at(point(12.0, 0.0));

    assert_eq!(output.len(), 1);
    assert_eq!(output[0].x, 12.0);
}

#[test]
fn preview_through_does_not_advance_the_smoother() {
    let mut smoother = StrokeSmoother::default();
    smoother.begin(point(0.0, 0.0));
    smoother.push(point(8.0, 2.0));
    smoother.push(point(16.0, 8.0));
    let mut control = smoother.clone();

    let preview = smoother.preview_through(point(24.0, 10.0));
    let actual = smoother.push(point(24.0, 10.0));
    let expected = control.push(point(24.0, 10.0));

    assert_eq!(preview.last().map(|point| point.x), Some(24.0));
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().zip(expected).all(|(actual, expected)| {
        close(actual.x, expected.x)
            && close(actual.y, expected.y)
            && close(actual.radius, expected.radius)
            && close(actual.opacity, expected.opacity)
    }));
}

#[test]
fn catmull_rom_segment_hits_endpoints() {
    let p0 = point(0.0, 0.0);
    let p1 = point(10.0, 0.0);
    let p2 = point(20.0, 10.0);
    let p3 = point(30.0, 10.0);

    let start = centripetal_catmull_rom_position(p0, p1, p2, p3, 0.0);
    let end = centripetal_catmull_rom_position(p0, p1, p2, p3, 1.0);

    assert!(close(start[0], p1.x));
    assert!(close(start[1], p1.y));
    assert!(close(end[0], p2.x));
    assert!(close(end[1], p2.y));
}

#[test]
fn adjacent_segments_have_continuous_tangent_directions() {
    let points = [
        point(-30.0, 20.0),
        point(0.0, 0.0),
        point(35.0, 50.0),
        point(120.0, 70.0),
        point(180.0, 10.0),
    ];
    let epsilon = 1.0e-3;
    let knot = curve_position(points[0], points[1], points[2], points[3], 1.0);
    let before = curve_position(points[0], points[1], points[2], points[3], 1.0 - epsilon);
    let after = curve_position(points[1], points[2], points[3], points[4], epsilon);
    let left = [knot[0] - before[0], knot[1] - before[1]];
    let right = [after[0] - knot[0], after[1] - knot[1]];
    let cross = (left[0] * right[1] - left[1] * right[0]).abs();
    let lengths = left[0].hypot(left[1]) * right[0].hypot(right[1]);
    let dot = left[0] * right[0] + left[1] * right[1];

    assert!(cross / lengths < 0.01);
    assert!(dot > 0.0);
}

#[test]
fn smoother_waits_for_one_future_point_then_flushes_end() {
    let mut smoother = StrokeSmoother::default();
    let p0 = point(0.0, 0.0);
    let p1 = point(10.0, 0.0);
    let p2 = point(20.0, 10.0);

    smoother.begin(p0);
    assert!(smoother.push(p1).is_empty());

    let first = smoother.push(p2);
    let first_end = first.last().expect("first segment should be emitted");
    assert!(close(first_end.x, p1.x));
    assert!(close(first_end.y, p1.y));

    let final_segment = smoother.finish();
    let final_end = final_segment
        .last()
        .expect("final segment should be flushed");
    assert!(close(final_end.x, p2.x));
    assert!(close(final_end.y, p2.y));
}

#[test]
fn subpixel_movements_create_control_points() {
    let mut smoother = StrokeSmoother::default();
    let start = point(0.0, 0.0);
    let first = point(0.4, 0.0);
    let second = point(0.8, 0.0);
    smoother.begin(start);

    assert!(smoother.push(first).is_empty());
    let emitted = smoother.push(second);

    let first_end = emitted.last().expect("subpixel segment should be emitted");
    assert!(close(first_end.x, first.x));
    let final_segment = smoother.finish();
    let final_end = final_segment.last().expect("tail should be emitted");
    assert!(close(final_end.x, second.x));
}

#[test]
fn short_backtracking_preserves_the_turning_point() {
    let mut smoother = StrokeSmoother::default();
    let start = point(0.0, 0.0);
    let turn = point(1.0, 0.0);
    smoother.begin(start);

    assert!(smoother.push(turn).is_empty());
    let outward_segment = smoother.push(start);
    let outward_end = outward_segment
        .last()
        .expect("short outward segment should be emitted");
    assert!(close(outward_end.x, turn.x));
    assert!(close(outward_end.y, turn.y));

    let return_segment = smoother.finish();
    let return_end = return_segment
        .last()
        .expect("short return segment should be emitted");
    assert!(close(return_end.x, start.x));
    assert!(close(return_end.y, start.y));
}

#[test]
fn curved_small_events_are_not_collapsed_into_one_chord() {
    let mut smoother = StrokeSmoother::default();
    let start = point(0.0, 0.0);
    let first_control = point(1.6, 0.0);
    smoother.begin(start);

    let mut emitted = Vec::new();
    for raw in [
        point(0.4, 0.0),
        point(0.8, 0.0),
        point(1.2, 0.0),
        first_control,
        point(1.9, 0.3),
        point(2.2, 0.6),
        point(2.5, 0.9),
        point(2.5, 1.3),
        point(2.4, 1.7),
        point(2.2, 2.1),
        point(1.9, 2.4),
    ] {
        emitted.extend(smoother.push(raw));
    }
    emitted.extend(smoother.finish());

    assert!(
        emitted
            .iter()
            .any(|point| close(point.x, first_control.x) && close(point.y, first_control.y))
    );
    assert!(emitted.iter().any(|point| point.y > 2.0));
}

#[test]
fn coalesced_stationary_endpoint_is_flushed_exactly() {
    let mut smoother = StrokeSmoother::default();
    let p0 = point(0.0, 0.0);
    let p1 = point(10.0, 0.0);
    let mut p1_latest = p1;
    p1_latest.radius += 0.2;

    smoother.begin(p0);
    assert!(smoother.push(p1).is_empty());
    assert!(smoother.push(p1_latest).is_empty());

    let final_segment = smoother.finish();
    let final_end = final_segment
        .last()
        .expect("coalesced endpoint should be flushed");
    assert!(close(final_end.x, p1_latest.x));
    assert!(close(final_end.y, p1_latest.y));
    assert!(close(final_end.radius, p1_latest.radius));
}

#[test]
fn cumulative_pressure_changes_create_a_control_point() {
    let mut smoother = StrokeSmoother::default();
    let p0 = point(0.0, 0.0);
    let p1 = point(10.0, 0.0);
    smoother.begin(p0);
    assert!(smoother.push(p1).is_empty());

    for radius_delta in [0.1, 0.2, 0.3] {
        let mut pressure_point = p1;
        pressure_point.radius += radius_delta;
        assert!(smoother.push(pressure_point).is_empty());
    }

    let mut accepted_pressure_point = p1;
    accepted_pressure_point.radius += 0.5;
    assert!(!smoother.push(accepted_pressure_point).is_empty());
}

#[test]
fn duplicate_points_do_not_emit_nans() {
    let p = point(5.0, 5.0);
    let emitted = sample_segment(p, p, p, p);

    assert!(!emitted.is_empty());
    assert!(emitted.iter().all(|point| {
        point.x.is_finite()
            && point.y.is_finite()
            && point.radius.is_finite()
            && point.opacity.is_finite()
    }));
}

#[test]
fn pressure_values_are_linearly_interpolated_and_clamped() {
    let mut p1 = point(0.0, 0.0);
    let mut p2 = point(16.0, 0.0);
    p1.radius = -4.0;
    p1.opacity = -1.0;
    p2.radius = 8.0;
    p2.opacity = 2.0;

    let emitted = sample_segment(
        extrapolate_before(p1, p2),
        p1,
        p2,
        extrapolate_after(p1, p2),
    );

    assert!(
        emitted
            .iter()
            .all(|point| point.radius >= 0.0 && (0.0..=1.0).contains(&point.opacity))
    );
}

#[test]
fn straight_segments_emit_only_endpoint() {
    let emitted = sample_segment(
        point(0.0, 0.0),
        point(10.0, 0.0),
        point(1000.0, 0.0),
        point(1010.0, 0.0),
    );

    assert_eq!(emitted.len(), 1);
    assert!(close(emitted[0].x, 1000.0));
    assert!(close(emitted[0].y, 0.0));
}

#[test]
fn sparse_fast_turn_emits_a_curve_instead_of_one_chord() {
    let p0 = point(-100.0, 0.0);
    let p1 = point(0.0, 0.0);
    let p2 = point(100.0, 0.0);
    let p3 = point(200.0, 100.0);
    let emitted = sample_segment(p0, p1, p2, p3);

    assert!(emitted.len() > 1);
    assert!(
        emitted[..emitted.len() - 1]
            .iter()
            .any(|point| point.y.abs() > 0.01)
    );
    assert!(close(emitted.last().expect("endpoint").x, p2.x));
    assert!(close(emitted.last().expect("endpoint").y, p2.y));
}

#[test]
fn smoothing_strength_blends_linear_and_curved_positions() {
    let p0 = point(-100.0, 0.0);
    let p1 = point(0.0, 0.0);
    let p2 = point(100.0, 0.0);
    let p3 = point(200.0, 100.0);
    let u = 0.5;

    let linear = stroke_point_at(p0, p1, p2, p3, 0.0, u);
    let half = stroke_point_at(p0, p1, p2, p3, 0.5, u);
    let full = stroke_point_at(p0, p1, p2, p3, 1.0, u);

    assert!(close(linear.x, 50.0));
    assert!(close(linear.y, 0.0));
    assert!(full.y.abs() > 0.01);
    assert!(close(half.x, lerp(linear.x, full.x, 0.5)));
    assert!(close(half.y, lerp(linear.y, full.y, 0.5)));
}

#[test]
fn normal_canvas_curve_respects_flatness_tolerance() {
    let controls = [
        point(0.0, 0.0),
        point(0.0, 4000.0),
        point(4000.0, 4000.0),
        point(4000.0, 0.0),
    ];
    let emitted = sample_segment(controls[0], controls[1], controls[2], controls[3]);
    let error = max_polyline_error(controls[0], controls[1], controls[2], controls[3], &emitted);

    assert!(error <= CURVE_FLATNESS_PX, "flattening error: {error}");
}

#[test]
fn balanced_sampling_degrades_evenly_when_capped() {
    let controls = [
        point(0.0, 0.0),
        point(0.0, 10_000.0),
        point(10_000.0, 10_000.0),
        point(10_000.0, 0.0),
    ];
    let params = adaptive_sample_parameters(controls[0], controls[1], controls[2], controls[3]);
    let emitted = sample_segment(controls[0], controls[1], controls[2], controls[3]);
    let error = max_polyline_error(controls[0], controls[1], controls[2], controls[3], &emitted);

    assert_eq!(params.len(), MAX_CURVE_SAMPLES);
    assert!(params.windows(2).all(|window| window[0] < window[1]));
    assert!(close(*params.last().expect("final parameter"), 1.0));
    assert!(error < 1.0, "capped flattening error: {error}");
}
