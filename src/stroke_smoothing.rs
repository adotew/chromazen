use std::collections::VecDeque;

use crate::brush::StrokePoint;

const CENTRIPETAL_ALPHA: f32 = 0.5;
const PARAMETER_EPSILON: f32 = 1.0e-4;
const PARAM_U_EPSILON: f32 = 1.0e-5;

const MIN_INPUT_DISTANCE_PX: f32 = 0.75;
const MIN_RADIUS_DELTA_PX: f32 = 0.25;
const MIN_OPACITY_DELTA: f32 = 0.015;

const CURVE_FLATNESS_PX: f32 = 0.35;
const MAX_ADAPTIVE_DEPTH: usize = 10;
const MAX_CURVE_SAMPLES: usize = 96;

const POSITION_FILTER_ALPHA: f32 = 0.65;
const PRESSURE_FILTER_ALPHA: f32 = 0.45;

#[derive(Clone, Copy, Debug)]
pub(crate) struct StrokeSmoothingOptions {
    pub enabled: bool,
    pub strength: f32,
    pub jitter_filter: bool,
}

impl Default for StrokeSmoothingOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 0.8,
            jitter_filter: false,
        }
    }
}

impl StrokeSmoothingOptions {
    fn clamped_strength(self) -> f32 {
        if self.enabled {
            self.strength.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct StrokeSmoother {
    points: VecDeque<StrokePoint>,
    first_segment_emitted: bool,
    filtered_point: Option<StrokePoint>,
    last_raw_point: Option<StrokePoint>,
}

impl StrokeSmoother {
    pub(crate) fn begin(&mut self, point: StrokePoint) {
        self.reset();
        self.filtered_point = Some(point);
        self.last_raw_point = Some(point);
        self.points.push_back(point);
    }

    pub(crate) fn push(
        &mut self,
        point: StrokePoint,
        options: StrokeSmoothingOptions,
    ) -> Vec<StrokePoint> {
        let point = self.prepare_point(point, options);
        if self.coalesce_near_duplicate(point) {
            return Vec::new();
        }

        self.points.push_back(point);
        self.emit_available_segment(options.clamped_strength())
    }

    pub(crate) fn finish(&mut self, options: StrokeSmoothingOptions) -> Vec<StrokePoint> {
        if options.jitter_filter {
            self.snap_pending_endpoint_to_raw_input();
        }

        let strength = options.clamped_strength();
        let smoothed = match self.points.len() {
            0 | 1 => Vec::new(),
            2 => sample_segment(
                extrapolate_before(self.points[0], self.points[1]),
                self.points[0],
                self.points[1],
                extrapolate_after(self.points[0], self.points[1]),
                strength,
            ),
            len if self.first_segment_emitted => {
                let previous = self.points[len - 3];
                let from = self.points[len - 2];
                let to = self.points[len - 1];
                sample_segment(previous, from, to, extrapolate_after(from, to), strength)
            }
            _ => Vec::new(),
        };
        self.reset();
        smoothed
    }

    pub(crate) fn reset(&mut self) {
        self.points.clear();
        self.first_segment_emitted = false;
        self.filtered_point = None;
        self.last_raw_point = None;
    }

    fn prepare_point(
        &mut self,
        raw_point: StrokePoint,
        options: StrokeSmoothingOptions,
    ) -> StrokePoint {
        self.last_raw_point = Some(raw_point);

        if !options.jitter_filter {
            self.filtered_point = Some(raw_point);
            return raw_point;
        }

        let filtered = self
            .filtered_point
            .map_or(raw_point, |previous| StrokePoint {
                x: lerp(previous.x, raw_point.x, POSITION_FILTER_ALPHA),
                y: lerp(previous.y, raw_point.y, POSITION_FILTER_ALPHA),
                radius: lerp(previous.radius, raw_point.radius, PRESSURE_FILTER_ALPHA).max(0.0),
                opacity: lerp(previous.opacity, raw_point.opacity, PRESSURE_FILTER_ALPHA)
                    .clamp(0.0, 1.0),
            });
        self.filtered_point = Some(filtered);
        filtered
    }

    fn coalesce_near_duplicate(&mut self, point: StrokePoint) -> bool {
        let Some(&last) = self.points.back() else {
            return false;
        };
        if !is_near_duplicate(last, point) {
            return false;
        }

        if self.pending_endpoint_can_be_replaced() {
            if let Some(last) = self.points.back_mut() {
                *last = point;
            }
        }
        true
    }

    fn pending_endpoint_can_be_replaced(&self) -> bool {
        self.points.len() >= 2
    }

    fn emit_available_segment(&mut self, strength: f32) -> Vec<StrokePoint> {
        match self.points.len() {
            0..=2 => Vec::new(),
            3 if !self.first_segment_emitted => {
                self.first_segment_emitted = true;
                sample_segment(
                    extrapolate_before(self.points[0], self.points[1]),
                    self.points[0],
                    self.points[1],
                    self.points[2],
                    strength,
                )
            }
            4.. => {
                let smoothed = sample_segment(
                    self.points[0],
                    self.points[1],
                    self.points[2],
                    self.points[3],
                    strength,
                );
                self.points.pop_front();
                smoothed
            }
            _ => Vec::new(),
        }
    }

    fn snap_pending_endpoint_to_raw_input(&mut self) {
        let Some(raw_point) = self.last_raw_point else {
            return;
        };
        if self.pending_endpoint_can_be_replaced() {
            if let Some(last) = self.points.back_mut() {
                *last = raw_point;
            }
        }
    }
}

fn sample_segment(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
) -> Vec<StrokePoint> {
    let mut params = Vec::new();
    adaptive_sample_segment(p0, p1, p2, p3, strength, 0.0, 1.0, 0, &mut params);
    ensure_final_sample(&mut params);

    params
        .into_iter()
        .map(|u| stroke_point_at(p0, p1, p2, p3, strength, u))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn adaptive_sample_segment(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
    u0: f32,
    u1: f32,
    depth: usize,
    params: &mut Vec<f32>,
) {
    if params.len() >= MAX_CURVE_SAMPLES {
        return;
    }

    let start = curve_position(p0, p1, p2, p3, strength, u0);
    let end = curve_position(p0, p1, p2, p3, strength, u1);
    let flatness = curve_flatness(p0, p1, p2, p3, strength, u0, u1, start, end);

    if flatness <= CURVE_FLATNESS_PX
        || depth >= MAX_ADAPTIVE_DEPTH
        || params.len() + 1 >= MAX_CURVE_SAMPLES
    {
        push_param(params, u1);
        return;
    }

    let mid = (u0 + u1) * 0.5;
    adaptive_sample_segment(p0, p1, p2, p3, strength, u0, mid, depth + 1, params);
    adaptive_sample_segment(p0, p1, p2, p3, strength, mid, u1, depth + 1, params);
}

#[allow(clippy::too_many_arguments)]
fn curve_flatness(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
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
        .map(|u| distance_to_line_segment(curve_position(p0, p1, p2, p3, strength, u), start, end))
        .fold(0.0, f32::max)
}

fn ensure_final_sample(params: &mut Vec<f32>) {
    if params
        .last()
        .is_some_and(|&last| (last - 1.0).abs() <= PARAM_U_EPSILON)
    {
        return;
    }

    if params.len() >= MAX_CURVE_SAMPLES {
        params.pop();
    }
    params.push(1.0);
}

fn push_param(params: &mut Vec<f32>, u: f32) {
    if params
        .last()
        .is_some_and(|&last| (u - last).abs() <= PARAM_U_EPSILON)
    {
        return;
    }
    params.push(u.clamp(0.0, 1.0));
}

fn stroke_point_at(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
    strength: f32,
    u: f32,
) -> StrokePoint {
    let position = curve_position(p0, p1, p2, p3, strength, u);
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
    strength: f32,
    u: f32,
) -> [f32; 2] {
    let u = u.clamp(0.0, 1.0);
    let linear = [lerp(p1.x, p2.x, u), lerp(p1.y, p2.y, u)];
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return linear;
    }

    let curved = centripetal_catmull_rom_position(p0, p1, p2, p3, u);
    [
        lerp(linear[0], curved[0], strength),
        lerp(linear[1], curved[1], strength),
    ]
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

fn is_near_duplicate(a: StrokePoint, b: StrokePoint) -> bool {
    distance(a, b) <= MIN_INPUT_DISTANCE_PX
        && (a.radius - b.radius).abs() <= MIN_RADIUS_DELTA_PX
        && (a.opacity - b.opacity).abs() <= MIN_OPACITY_DELTA
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

    fn options() -> StrokeSmoothingOptions {
        StrokeSmoothingOptions {
            enabled: true,
            strength: 1.0,
            jitter_filter: false,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
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
    fn smoother_waits_for_one_future_point_then_flushes_end() {
        let mut smoother = StrokeSmoother::default();
        let p0 = point(0.0, 0.0);
        let p1 = point(10.0, 0.0);
        let p2 = point(20.0, 10.0);

        smoother.begin(p0);
        assert!(smoother.push(p1, options()).is_empty());

        let first = smoother.push(p2, options());
        let first_end = first.last().expect("first segment should be emitted");
        assert!(close(first_end.x, p1.x));
        assert!(close(first_end.y, p1.y));

        let final_segment = smoother.finish(options());
        let final_end = final_segment
            .last()
            .expect("final segment should be flushed");
        assert!(close(final_end.x, p2.x));
        assert!(close(final_end.y, p2.y));
    }

    #[test]
    fn near_duplicate_points_replace_unemitted_endpoint() {
        let mut smoother = StrokeSmoother::default();
        let p0 = point(0.0, 0.0);
        let p1 = point(10.0, 0.0);
        let p1_latest = point(10.25, 0.2);

        smoother.begin(p0);
        assert!(smoother.push(p1, options()).is_empty());
        assert!(smoother.push(p1_latest, options()).is_empty());

        let final_segment = smoother.finish(options());
        let final_end = final_segment
            .last()
            .expect("coalesced endpoint should be flushed");
        assert!(close(final_end.x, p1_latest.x));
        assert!(close(final_end.y, p1_latest.y));
    }

    #[test]
    fn pressure_changes_are_not_coalesced_as_duplicates() {
        let mut smoother = StrokeSmoother::default();
        let p0 = point(0.0, 0.0);
        let p1 = point(10.0, 0.0);
        let mut p2 = point(10.1, 0.0);
        p2.radius = p1.radius + MIN_RADIUS_DELTA_PX * 2.0;

        smoother.begin(p0);
        assert!(smoother.push(p1, options()).is_empty());
        assert!(!smoother.push(p2, options()).is_empty());

        let final_segment = smoother.finish(options());
        let final_end = final_segment
            .last()
            .expect("pressure change endpoint should be flushed");
        assert!(close(final_end.x, p2.x));
        assert!(close(final_end.radius, p2.radius));
    }

    #[test]
    fn duplicate_points_do_not_emit_nans() {
        let p = point(5.0, 5.0);
        let emitted = sample_segment(p, p, p, p, 1.0);

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
            1.0,
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
            1.0,
        );

        assert_eq!(emitted.len(), 1);
        assert!(close(emitted[0].x, 1000.0));
        assert!(close(emitted[0].y, 0.0));
    }

    #[test]
    fn curved_segments_emit_more_samples_than_straight_segments() {
        let straight = sample_segment(
            point(0.0, 0.0),
            point(10.0, 0.0),
            point(1000.0, 0.0),
            point(1010.0, 0.0),
            1.0,
        );
        let curved = sample_segment(
            point(0.0, 0.0),
            point(0.0, 100.0),
            point(100.0, 100.0),
            point(100.0, 0.0),
            1.0,
        );

        assert!(curved.len() > straight.len());
        assert!(curved.len() <= MAX_CURVE_SAMPLES);
    }

    #[test]
    fn smoothing_strength_zero_falls_back_to_linear_positions() {
        let p0 = point(0.0, 100.0);
        let p1 = point(0.0, 0.0);
        let p2 = point(100.0, 0.0);
        let p3 = point(100.0, 100.0);

        let midpoint = curve_position(p0, p1, p2, p3, 0.0, 0.5);

        assert!(close(midpoint[0], 50.0));
        assert!(close(midpoint[1], 0.0));
    }

    #[test]
    fn adaptive_sampling_is_capped() {
        let emitted = sample_segment(
            point(0.0, 0.0),
            point(0.0, 5000.0),
            point(5000.0, 5000.0),
            point(5000.0, 0.0),
            1.0,
        );

        assert!(emitted.len() <= MAX_CURVE_SAMPLES);
    }

    #[test]
    fn jitter_filter_snaps_final_endpoint_to_raw_input() {
        let mut smoother = StrokeSmoother::default();
        let mut options = options();
        options.jitter_filter = true;

        let p0 = point(0.0, 0.0);
        let p1 = point(100.0, 0.0);
        let p2 = point(200.0, 0.0);

        smoother.begin(p0);
        assert!(smoother.push(p1, options).is_empty());
        smoother.push(p2, options);

        let final_segment = smoother.finish(options);
        let final_end = final_segment
            .last()
            .expect("final raw endpoint should be emitted");
        assert!(close(final_end.x, p2.x));
        assert!(close(final_end.y, p2.y));
    }
}
