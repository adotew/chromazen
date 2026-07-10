use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};

use crate::brush::StrokePoint;

pub(crate) const MAX_STAMPS_PER_FRAME: usize = 1024;
const MIN_STAMP_SPACING: f32 = 1.0;
const STAMP_SPACING_RATIO: f32 = 0.25;
const BRUSH_STAMP_ASPECT: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct StampRaw {
    center: [f32; 2],
    half_size: [f32; 2],
    color: [f32; 4],
    bounds: [f32; 4],
}

#[derive(Clone, Copy)]
struct Stamp {
    x: f32,
    y: f32,
    radius: f32,
    rgba: [f32; 4],
}

#[derive(Default)]
pub(crate) struct StampQueue {
    pending: VecDeque<Stamp>,
    distance_since_last_stamp: f32,
}

impl StampQueue {
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.distance_since_last_stamp = 0.0;
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn reset_spacing(&mut self) {
        self.distance_since_last_stamp = 0.0;
    }

    pub(crate) fn queue_point(
        &mut self,
        point: StrokePoint,
        mut rgba: [f32; 4],
        width: u32,
        height: u32,
    ) -> bool {
        rgba[3] = point.opacity;
        self.queue_stamp(
            Stamp {
                x: point.x,
                y: point.y,
                radius: point.radius,
                rgba,
            },
            width,
            height,
        )
    }

    fn queue_stamp(&mut self, stamp: Stamp, width: u32, height: u32) -> bool {
        let bounds = get_stamp_bounds(stamp.x, stamp.y, stamp.radius, width, height);
        if stamp.x + bounds.half_width < 0.0
            || stamp.y + bounds.half_height < 0.0
            || stamp.x - bounds.half_width >= width as f32
            || stamp.y - bounds.half_height >= height as f32
            || bounds.max_x < bounds.min_x
            || bounds.max_y < bounds.min_y
        {
            return false;
        }

        self.pending.push_back(stamp);
        true
    }

    pub(crate) fn stamp_line(
        &mut self,
        from: StrokePoint,
        to: StrokePoint,
        rgba: [f32; 4],
        width: u32,
        height: u32,
    ) -> usize {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dist = dx.hypot(dy);
        if dist == 0.0 {
            return 0;
        }

        let mut queued = 0;
        let mut travelled = 0.0;
        while travelled < dist {
            let spacing_t = travelled / dist;
            let spacing_radius = lerp(from.radius, to.radius, spacing_t);
            let spacing = get_stamp_spacing(spacing_radius);
            let distance_to_next_stamp = (spacing - self.distance_since_last_stamp).max(0.0);
            let remaining_distance = dist - travelled;

            if distance_to_next_stamp > remaining_distance {
                self.distance_since_last_stamp += remaining_distance;
                return queued;
            }

            travelled += distance_to_next_stamp;
            let t = travelled / dist;
            let radius = lerp(from.radius, to.radius, t);
            let opacity = lerp(from.opacity, to.opacity, t);
            let x = from.x + dx * t;
            let y = from.y + dy * t;
            let mut color = rgba;
            color[3] = opacity;
            if self.queue_stamp(
                Stamp {
                    x,
                    y,
                    radius,
                    rgba: color,
                },
                width,
                height,
            ) {
                queued += 1;
            }
            self.distance_since_last_stamp = 0.0;
        }

        queued
    }

    pub(crate) fn drain_raw(&mut self, width: u32, height: u32, max_count: usize) -> Vec<StampRaw> {
        let count = self.pending.len().min(max_count);
        let mut raw = Vec::with_capacity(count);
        for _ in 0..count {
            let stamp = self.pending.pop_front().expect("count checked");
            raw.push(stamp_to_raw(stamp, width, height));
        }
        raw
    }
}

fn stamp_to_raw(stamp: Stamp, width: u32, height: u32) -> StampRaw {
    let bounds = get_stamp_bounds(stamp.x, stamp.y, stamp.radius, width, height);
    StampRaw {
        center: [stamp.x, stamp.y],
        half_size: [bounds.half_width, bounds.half_height],
        color: stamp.rgba,
        bounds: [
            bounds.min_x as f32,
            bounds.min_y as f32,
            bounds.max_x as f32,
            bounds.max_y as f32,
        ],
    }
}

struct StampBounds {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    half_width: f32,
    half_height: f32,
}

fn get_stamp_half_size(radius: f32) -> (f32, f32) {
    if BRUSH_STAMP_ASPECT >= 1.0 {
        (radius, radius / BRUSH_STAMP_ASPECT)
    } else {
        (radius * BRUSH_STAMP_ASPECT, radius)
    }
}

fn get_stamp_bounds(x: f32, y: f32, radius: f32, width: u32, height: u32) -> StampBounds {
    let (half_width, half_height) = get_stamp_half_size(radius);
    let min_x = 0.max((x - half_width).floor() as i32);
    let max_x = (width as i32 - 1).min((x + half_width).ceil() as i32);
    let min_y = 0.max((y - half_height).floor() as i32);
    let max_y = (height as i32 - 1).min((y + half_height).ceil() as i32);
    StampBounds {
        min_x,
        max_x,
        min_y,
        max_y,
        half_width,
        half_height,
    }
}

fn get_stamp_spacing(radius: f32) -> f32 {
    MIN_STAMP_SPACING.max(radius * STAMP_SPACING_RATIO)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke_smoothing::StrokeSmoother;

    fn point(x: f32, y: f32) -> StrokePoint {
        StrokePoint {
            x,
            y,
            radius: 10.0,
            opacity: 1.0,
        }
    }

    #[test]
    fn stamp_spacing_has_a_one_pixel_floor() {
        assert_eq!(get_stamp_spacing(0.5), 1.0);
        assert_eq!(get_stamp_spacing(20.0), 5.0);
    }

    #[test]
    fn smoothed_polyline_preserves_continuous_dab_spacing() {
        let input = [
            point(100.0, 100.0),
            point(250.0, 100.0),
            point(400.0, 250.0),
            point(550.0, 250.0),
        ];
        let mut smoother = StrokeSmoother::default();
        let mut path = vec![input[0]];
        smoother.begin(input[0]);
        for point in input.into_iter().skip(1) {
            path.extend(smoother.push(point));
        }
        path.extend(smoother.finish());

        let mut queue = StampQueue::default();
        let color = [0.0, 0.0, 0.0, 1.0];
        assert!(queue.queue_point(path[0], color, 1000, 1000));
        for segment in path.windows(2) {
            queue.stamp_line(segment[0], segment[1], color, 1000, 1000);
        }

        let expected_spacing = get_stamp_spacing(input[0].radius);
        assert!(queue.pending.len() > 100);
        assert!(queue.pending.iter().zip(queue.pending.iter().skip(1)).all(
            |(from, to)| (to.x - from.x).hypot(to.y - from.y) <= expected_spacing + 1.0e-3
        ));
    }
}
