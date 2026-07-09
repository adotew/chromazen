use std::collections::VecDeque;

use crate::brush::StrokePoint;

const CENTRIPETAL_ALPHA: f32 = 0.5;
const PARAMETER_EPSILON: f32 = 1.0e-4;
const CURVE_SAMPLE_STEP_PX: f32 = 8.0;
const MIN_CURVE_SAMPLES: usize = 4;
const MAX_CURVE_SAMPLES: usize = 64;

#[derive(Debug, Default)]
pub(crate) struct StrokeSmoother {
    points: VecDeque<StrokePoint>,
    first_segment_emitted: bool,
}

impl StrokeSmoother {
    pub(crate) fn begin(&mut self, point: StrokePoint) {
        self.reset();
        self.points.push_back(point);
    }

    pub(crate) fn push(&mut self, point: StrokePoint) -> Vec<StrokePoint> {
        self.points.push_back(point);

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

    pub(crate) fn finish(&mut self) -> Vec<StrokePoint> {
        let smoothed = match self.points.len() {
            0 | 1 => Vec::new(),
            2 => sample_segment(
                extrapolate_before(self.points[0], self.points[1]),
                self.points[0],
                self.points[1],
                extrapolate_after(self.points[0], self.points[1]),
            ),
            len if self.first_segment_emitted => {
                let previous = self.points[len - 3];
                let from = self.points[len - 2];
                let to = self.points[len - 1];
                sample_segment(previous, from, to, extrapolate_after(from, to))
            }
            _ => Vec::new(),
        };
        self.reset();
        smoothed
    }

    pub(crate) fn reset(&mut self) {
        self.points.clear();
        self.first_segment_emitted = false;
    }
}

fn sample_segment(
    p0: StrokePoint,
    p1: StrokePoint,
    p2: StrokePoint,
    p3: StrokePoint,
) -> Vec<StrokePoint> {
    let samples = sample_count(p1, p2);
    let mut smoothed = Vec::with_capacity(samples);

    for sample in 1..=samples {
        let u = sample as f32 / samples as f32;
        let position = centripetal_catmull_rom_position(p0, p1, p2, p3, u);
        smoothed.push(StrokePoint {
            x: position[0],
            y: position[1],
            radius: lerp(p1.radius, p2.radius, u).max(0.0),
            opacity: lerp(p1.opacity, p2.opacity, u).clamp(0.0, 1.0),
        });
    }

    smoothed
}

fn sample_count(p1: StrokePoint, p2: StrokePoint) -> usize {
    ((distance(p1, p2) / CURVE_SAMPLE_STEP_PX).ceil() as usize)
        .clamp(MIN_CURVE_SAMPLES, MAX_CURVE_SAMPLES)
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
    fn duplicate_points_do_not_emit_nans() {
        let mut smoother = StrokeSmoother::default();
        let p = point(5.0, 5.0);

        smoother.begin(p);
        let emitted = [smoother.push(p), smoother.push(p), smoother.finish()].concat();

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
    fn sample_count_is_capped() {
        assert_eq!(
            sample_count(point(0.0, 0.0), point(10000.0, 0.0)),
            MAX_CURVE_SAMPLES
        );
    }
}
