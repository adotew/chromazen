use std::{collections::VecDeque, time::Duration};

use super::StrokePoint;

const CENTRIPETAL_ALPHA: f32 = 0.5;
const PARAMETER_EPSILON: f32 = 1.0e-4;
const MIN_RADIUS_DELTA_PX: f32 = 0.4;
const MIN_OPACITY_DELTA: f32 = 0.025;

const CURVE_FLATNESS_PX: f32 = 0.35;
const MAX_ADAPTIVE_DEPTH: usize = 10;
const MAX_CURVE_SAMPLES: usize = 96;

// One Euro parameters use logical screen points: Hz and inverse points, respectively.
const MIN_FILTER_CUTOFF_HZ: f32 = 1.0;
const FILTER_BETA: f32 = 0.05;
const DERIVATIVE_FILTER_CUTOFF_HZ: f32 = 1.0;
const FALLBACK_SAMPLE_INTERVAL: Duration = Duration::from_micros(8_333);

#[derive(Debug)]
pub struct StrokePositionFilter {
    filtered_position: [f32; 2],
    filtered_velocity: [f32; 2],
    previous_time: Duration,
    sample_interval: Duration,
    min_cutoff_hz: f32,
    beta: f32,
    initialized: bool,
}

impl Default for StrokePositionFilter {
    fn default() -> Self {
        Self {
            filtered_position: [0.0; 2],
            filtered_velocity: [0.0; 2],
            previous_time: Duration::ZERO,
            sample_interval: FALLBACK_SAMPLE_INTERVAL,
            min_cutoff_hz: MIN_FILTER_CUTOFF_HZ,
            beta: FILTER_BETA,
            initialized: false,
        }
    }
}

impl StrokePositionFilter {
    pub fn reset(&mut self, position: [f32; 2], time: Duration) -> [f32; 2] {
        self.filtered_position = position;
        self.filtered_velocity = [0.0; 2];
        self.previous_time = time;
        self.sample_interval = FALLBACK_SAMPLE_INTERVAL;
        self.initialized = true;
        position
    }

    pub fn filter(&mut self, position: [f32; 2], time: Duration) -> [f32; 2] {
        if !self.initialized {
            return self.reset(position, time);
        }

        if let Some(elapsed) = time
            .checked_sub(self.previous_time)
            .filter(|elapsed| !elapsed.is_zero())
        {
            self.sample_interval = elapsed;
        }
        self.previous_time = time;
        let dt = self.sample_interval.as_secs_f32();
        let derivative_alpha = low_pass_alpha(dt, DERIVATIVE_FILTER_CUTOFF_HZ);
        for (axis, raw) in position.into_iter().enumerate() {
            let velocity = (raw - self.filtered_position[axis]) / dt;
            self.filtered_velocity[axis] =
                low_pass(self.filtered_velocity[axis], velocity, derivative_alpha);
        }

        let speed = self.filtered_velocity[0].hypot(self.filtered_velocity[1]);
        let cutoff = self.min_cutoff_hz + self.beta * speed;
        let alpha = low_pass_alpha(dt, cutoff);
        for (filtered, raw) in self.filtered_position.iter_mut().zip(position) {
            *filtered = low_pass(*filtered, raw, alpha);
        }
        self.filtered_position
    }
}

#[derive(Clone, Copy, Debug)]
struct CurveInterval {
    u0: f32,
    u1: f32,
    depth: usize,
    error: f32,
}

#[derive(Clone, Debug, Default)]
pub struct StrokeSmoother {
    points: VecDeque<StrokePoint>,
    first_segment_emitted: bool,
    latest_raw_point: Option<StrokePoint>,
}

impl StrokeSmoother {
    pub fn begin(&mut self, point: StrokePoint) {
        self.reset();
        self.latest_raw_point = Some(point);
        self.points.push_back(point);
    }

    pub fn push(&mut self, point: StrokePoint) -> Vec<StrokePoint> {
        self.latest_raw_point = Some(point);
        if self.coalesce_stationary_duplicate(point) {
            return Vec::new();
        }

        self.points.push_back(point);
        self.emit_available_segment()
    }

    pub fn preview_through(&self, raw_point: StrokePoint) -> Vec<StrokePoint> {
        let mut preview_smoother = self.clone();
        preview_smoother.finish_at(raw_point)
    }

    pub fn finish_at(&mut self, point: StrokePoint) -> Vec<StrokePoint> {
        if let Some(last) = self.points.back_mut() {
            *last = point;
        }
        self.latest_raw_point = Some(point);
        self.finish()
    }

    pub fn finish(&mut self) -> Vec<StrokePoint> {
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
            2 => smoothed.extend(sample_segment(
                extrapolate_before(self.points[0], self.points[1]),
                self.points[0],
                self.points[1],
                extrapolate_after(self.points[0], self.points[1]),
            )),
            len if self.first_segment_emitted => {
                let previous = self.points[len - 3];
                let from = self.points[len - 2];
                let to = self.points[len - 1];
                smoothed.extend(sample_segment(
                    previous,
                    from,
                    to,
                    extrapolate_after(from, to),
                ));
            }
            _ => {}
        }

        self.reset();
        smoothed
    }

    pub fn reset(&mut self) {
        self.points.clear();
        self.first_segment_emitted = false;
        self.latest_raw_point = None;
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
                sample_segment(
                    extrapolate_before(self.points[0], self.points[1]),
                    self.points[0],
                    self.points[1],
                    self.points[2],
                )
            }
            4.. => {
                let smoothed = sample_segment(
                    self.points[0],
                    self.points[1],
                    self.points[2],
                    self.points[3],
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

fn low_pass(previous: f32, value: f32, alpha: f32) -> f32 {
    previous + alpha * (value - previous)
}

fn low_pass_alpha(dt: f32, cutoff_hz: f32) -> f32 {
    1.0 / (1.0 + 1.0 / (std::f32::consts::TAU * cutoff_hz * dt))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests;
