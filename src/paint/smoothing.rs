use std::collections::VecDeque;

use super::StrokePoint;

const CENTRIPETAL_ALPHA: f32 = 0.5;
const PARAMETER_EPSILON: f32 = 1.0e-4;
const MIN_RADIUS_DELTA_PX: f32 = 0.4;
const MIN_OPACITY_DELTA: f32 = 0.025;

const CURVE_FLATNESS_PX: f32 = 0.35;
const MAX_ADAPTIVE_DEPTH: usize = 10;
const MAX_CURVE_SAMPLES: usize = 96;

#[derive(Clone, Copy, Debug)]
pub(crate) struct StrokeSmoothingOptions {
    pub enabled: bool,
    pub strength: f32,
}

impl Default for StrokeSmoothingOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 0.8,
        }
    }
}

impl StrokeSmoothingOptions {
    pub(crate) fn is_active(self) -> bool {
        self.enabled && self.strength > f32::EPSILON
    }
}

#[derive(Clone, Copy, Debug)]
struct CurveInterval {
    u0: f32,
    u1: f32,
    depth: usize,
    error: f32,
}

#[derive(Debug)]
pub(crate) struct StrokeSmoother {
    points: VecDeque<StrokePoint>,
    first_segment_emitted: bool,
    latest_raw_point: Option<StrokePoint>,
    strength: f32,
}

impl Default for StrokeSmoother {
    fn default() -> Self {
        Self {
            points: VecDeque::new(),
            first_segment_emitted: false,
            latest_raw_point: None,
            strength: 1.0,
        }
    }
}

impl StrokeSmoother {
    #[cfg(test)]
    pub(crate) fn begin(&mut self, point: StrokePoint) {
        self.begin_with_strength(point, 1.0);
    }

    pub(crate) fn begin_with_strength(&mut self, point: StrokePoint, strength: f32) {
        self.reset();
        self.latest_raw_point = Some(point);
        self.points.push_back(point);
        self.strength = strength.clamp(0.0, 1.0);
    }

    pub(crate) fn push(&mut self, point: StrokePoint) -> Vec<StrokePoint> {
        self.latest_raw_point = Some(point);
        if self.coalesce_stationary_duplicate(point) {
            return Vec::new();
        }

        self.points.push_back(point);
        self.emit_available_segment()
    }

    pub(crate) fn finish(&mut self) -> Vec<StrokePoint> {
        let mut smoothed = Vec::new();

        if let Some(latest_raw_point) = self.latest_raw_point
            && self
                .points
                .back()
                .is_none_or(|&last| !same_stroke_point(last, latest_raw_point))
        {
            self.points.push_back(latest_raw_point);
            smoothed.extend(self.emit_available_segment());
        }

        match self.points.len() {
            0 | 1 => {}
            2 => smoothed.extend(sample_segment_with_strength(
                extrapolate_before(self.points[0], self.points[1]),
                self.points[0],
                self.points[1],
                extrapolate_after(self.points[0], self.points[1]),
                self.strength,
            )),
            len if self.first_segment_emitted => {
                let previous = self.points[len - 3];
                let from = self.points[len - 2];
                let to = self.points[len - 1];
                smoothed.extend(sample_segment_with_strength(
                    previous,
                    from,
                    to,
                    extrapolate_after(from, to),
                    self.strength,
                ));
            }
            _ => {}
        }

        self.reset();
        smoothed
    }

    pub(crate) fn reset(&mut self) {
        self.points.clear();
        self.first_segment_emitted = false;
        self.latest_raw_point = None;
        self.strength = 1.0;
    }

    fn coalesce_stationary_duplicate(&self, point: StrokePoint) -> bool {
        self.points
            .back()
            .is_some_and(|&last| is_redundant_stationary_sample(last, point))
    }

    fn emit_available_segment(&mut self) -> Vec<StrokePoint> {
        match self.points.len() {
            0..=2 => Vec::new(),
            3 if !self.first_segment_emitted => {
                self.first_segment_emitted = true;
                sample_segment_with_strength(
                    extrapolate_before(self.points[0], self.points[1]),
                    self.points[0],
                    self.points[1],
                    self.points[2],
                    self.strength,
                )
            }
            4.. => {
                let smoothed = sample_segment_with_strength(
                    self.points[0],
                    self.points[1],
                    self.points[2],
                    self.points[3],
                    self.strength,
                );
                self.points.pop_front();
                smoothed
            }
            _ => Vec::new(),
        }
    }
}

fn sample_segment_with_strength(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
) -> Vec<StrokePoint> {
    adaptive_sample_parameters(p0, p1, p2, p3)
        .into_iter()
        .map(|u| stroke_point_at(p0, p1, p2, p3, strength, u))
        .collect()
}

#[cfg(test)]
fn sample_segment(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
) -> Vec<StrokePoint> {
    sample_segment_with_strength(p0, p1, p2, p3, 1.0)
}

fn adaptive_sample_parameters(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
) -> Vec<f32> {
    let mut intervals = vec![curve_interval(p0, p1, p2, p3, 0.0, 1.0, 0)];

    while intervals.len() < MAX_CURVE_SAMPLES {
        let Some((index, _)) = intervals
            .iter()
            .enumerate()
            .filter(|(_, interval)| {
                interval.depth < MAX_ADAPTIVE_DEPTH && interval.error > CURVE_FLATNESS_PX
            })
            .max_by(|(_, a), (_, b)| a.error.total_cmp(&b.error))
        else {
            break;
        };

        let interval = intervals[index];
        let mid = (interval.u0 + interval.u1) * 0.5;
        let next_depth = interval.depth + 1;
        intervals[index] = curve_interval(p0, p1, p2, p3, interval.u0, mid, next_depth);
        intervals.insert(
            index + 1,
            curve_interval(p0, p1, p2, p3, mid, interval.u1, next_depth),
        );
    }

    intervals.into_iter().map(|interval| interval.u1).collect()
}

#[allow(clippy::too_many_arguments)]
fn curve_interval(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    u0: f32,
    u1: f32,
    depth: usize,
) -> CurveInterval {
    let start = curve_position(p0, p1, p2, p3, u0);
    let end = curve_position(p0, p1, p2, p3, u1);
    CurveInterval {
        u0,
        u1,
        depth,
        error: curve_flatness(p0, p1, p2, p3, u0, u1, start, end),
    }
}

#[allow(clippy::too_many_arguments)]
fn curve_flatness(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    u0: f32,
    u1: f32,
    start: [f32; 2],
    end: [f32; 2],
) -> f32 {
    let mid = (u0 + u1) * 0.5;
    let quarter = (u0 + mid) * 0.5;
    let three_quarter = (mid + u1) * 0.5;

    [quarter, mid, three_quarter]
        .into_iter()
        .map(|u| distance_to_line_segment(curve_position(p0, p1, p2, p3, u), start, end))
        .fold(0.0, f32::max)
}

fn stroke_point_at(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
    u: f32,
) -> StrokePoint {
    let u = u.clamp(0.0, 1.0);
    let curved = curve_position(p0, p1, p2, p3, u);
    let linear = [lerp(p1.x, p2.x, u), lerp(p1.y, p2.y, u)];
    let strength = strength.clamp(0.0, 1.0);
    let position = [
        lerp(linear[0], curved[0], strength),
        lerp(linear[1], curved[1], strength),
    ];
    StrokePoint {
        x: position[0],
        y: position[1],
        radius: lerp(p1.radius, p2.radius, u).max(0.0),
        opacity: lerp(p1.opacity, p2.opacity, u).clamp(0.0, 1.0),
    }
}

fn curve_position(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    u: f32,
) -> [f32; 2] {
    centripetal_catmull_rom_position(p0, p1, p2, p3, u.clamp(0.0, 1.0))
}

fn centripetal_catmull_rom_position(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    u: f32,
) -> [f32; 2] {
    if u <= 0.0 {
        return [p1.x, p1.y];
    }
    if u >= 1.0 {
        return [p2.x, p2.y];
    }

    let t0 = 0.0;
    let t1 = next_parameter(t0, p0, p1);
    let t2 = next_parameter(t1, p1, p2);
    let t3 = next_parameter(t2, p2, p3);
    let t = lerp(t1, t2, u);

    let a1 = interpolate_position(p0, p1, t0, t1, t);
    let a2 = interpolate_position(p1, p2, t1, t2, t);
    let a3 = interpolate_position(p2, p3, t2, t3, t);

    let b1 = interpolate_xy(a1, a2, t0, t2, t);
    let b2 = interpolate_xy(a2, a3, t1, t3, t);

    interpolate_xy(b1, b2, t1, t2, t)
}

fn next_parameter(previous_t: f32, from: StrokePoint, to: StrokePoint) -> f32 {
    previous_t
        + distance(from, to)
            .max(PARAMETER_EPSILON)
            .powf(CENTRIPETAL_ALPHA)
}

fn interpolate_position(
    from: StrokePoint,
    to: StrokePoint,
    from_t: f32,
    to_t: f32,
    t: f32,
) -> [f32; 2] {
    interpolate_xy([from.x, from.y], [to.x, to.y], from_t, to_t, t)
}

fn interpolate_xy(from: [f32; 2], to: [f32; 2], from_t: f32, to_t: f32, t: f32) -> [f32; 2] {
    let denominator = (to_t - from_t).max(PARAMETER_EPSILON);
    let from_weight = (to_t - t) / denominator;
    let to_weight = (t - from_t) / denominator;
    [
        from[0] * from_weight + to[0] * to_weight,
        from[1] * from_weight + to[1] * to_weight,
    ]
}

fn extrapolate_before(first: StrokePoint, second: StrokePoint) -> StrokePoint {
    StrokePoint {
        x: first.x + (first.x - second.x),
        y: first.y + (first.y - second.y),
        radius: first.radius,
        opacity: first.opacity,
    }
}

fn extrapolate_after(previous: StrokePoint, last: StrokePoint) -> StrokePoint {
    StrokePoint {
        x: last.x + (last.x - previous.x),
        y: last.y + (last.y - previous.y),
        radius: last.radius,
        opacity: last.opacity,
    }
}

fn distance(from: StrokePoint, to: StrokePoint) -> f32 {
    (to.x - from.x).hypot(to.y - from.y)
}

fn distance_xy(from: [f32; 2], to: [f32; 2]) -> f32 {
    (to[0] - from[0]).hypot(to[1] - from[1])
}

fn distance_to_line_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let segment = [end[0] - start[0], end[1] - start[1]];
    let length_squared = segment[0] * segment[0] + segment[1] * segment[1];
    if length_squared <= PARAMETER_EPSILON {
        return distance_xy(point, start);
    }

    let to_point = [point[0] - start[0], point[1] - start[1]];
    let t =
        ((to_point[0] * segment[0] + to_point[1] * segment[1]) / length_squared).clamp(0.0, 1.0);
    let projected = [start[0] + segment[0] * t, start[1] + segment[1] * t];
    distance_xy(point, projected)
}

fn is_redundant_stationary_sample(a: StrokePoint, b: StrokePoint) -> bool {
    a.x == b.x
        && a.y == b.y
        && (a.radius - b.radius).abs() <= MIN_RADIUS_DELTA_PX
        && (a.opacity - b.opacity).abs() <= MIN_OPACITY_DELTA
}

fn same_stroke_point(a: StrokePoint, b: StrokePoint) -> bool {
    a.x == b.x && a.y == b.y && a.radius == b.radius && a.opacity == b.opacity
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
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
        let error =
            max_polyline_error(controls[0], controls[1], controls[2], controls[3], &emitted);

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
        let error =
            max_polyline_error(controls[0], controls[1], controls[2], controls[3], &emitted);

        assert_eq!(params.len(), MAX_CURVE_SAMPLES);
        assert!(params.windows(2).all(|window| window[0] < window[1]));
        assert!(close(*params.last().expect("final parameter"), 1.0));
        assert!(error < 1.0, "capped flattening error: {error}");
    }
}
