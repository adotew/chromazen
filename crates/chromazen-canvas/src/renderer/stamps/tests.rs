use super::*;
use crate::StrokeSmoother;

fn point(x: f32, y: f32) -> StrokePoint {
    StrokePoint {
        x,
        y,
        radius: 10.0,
        opacity: 1.0,
    }
}

fn transported_impulse_displacement(step: usize, strength: f32) -> f32 {
    const SAMPLE_COUNT: usize = 256;
    const START: usize = 64;
    const TRAVEL: usize = 24;

    assert_eq!(TRAVEL % step, 0);
    let mut samples = vec![0.0_f32; SAMPLE_COUNT];
    samples[START] = 1.0;
    for _ in 0..TRAVEL / step {
        let previous = samples.clone();
        for (index, sample) in samples.iter_mut().enumerate() {
            let source = index
                .checked_sub(step)
                .map_or(0.0, |source_index| previous[source_index]);
            *sample = previous[index] * (1.0 - strength) + source * strength;
        }
    }

    let mass: f32 = samples.iter().sum();
    let centroid = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| index as f32 * sample)
        .sum::<f32>()
        / mass;
    centroid - START as f32
}

#[test]
fn shifted_source_transport_is_stable_across_step_sizes() {
    let expected = 0.35 * 24.0;

    for step in [1, 2, 4] {
        let displacement = transported_impulse_displacement(step, 0.35);
        assert!((displacement - expected).abs() < 1.0e-4);
    }
}

#[test]
fn travel_scaled_strength_collapses_with_finer_steps() {
    const RADIUS: f32 = 8.0;
    const MAX_ADVECTION: f32 = 0.35;

    let fine = transported_impulse_displacement(1, MAX_ADVECTION / RADIUS);
    let coarse = transported_impulse_displacement(4, MAX_ADVECTION * 4.0 / RADIUS);

    assert!(fine < coarse * 0.3);
}

#[test]
fn stamp_raw_matches_shader_layout() {
    assert_eq!(std::mem::size_of::<StampRaw>(), 64);
}

#[test]
fn stamp_spacing_has_a_one_pixel_floor() {
    let spacing = BrushSpacing::default();
    assert_eq!(stamp_spacing(0.5, spacing), 1.0);
    assert_eq!(stamp_spacing(20.0, spacing), 1.0);
    assert_eq!(stamp_spacing(100.0, spacing), 3.0);
}

#[test]
fn stamp_bounds_follow_image_aspect_ratio() {
    assert_eq!(stamp_half_size(10.0, 2.0), (10.0, 5.0));
    assert_eq!(stamp_half_size(10.0, 0.5), (5.0, 10.0));
}

#[test]
fn smudge_opacity_scales_source_displacement() {
    let mut raw = StampRaw {
        center: [20.0, 30.0],
        half_size: [10.0; 2],
        color: [0.0; 4],
        bounds: [10.0, 20.0, 30.0, 40.0],
        source_center: [10.0, 50.0],
        padding: [0.0; 2],
    };

    raw.scale_source_offset(0.5);

    assert_eq!(raw.source_center, [15.0, 40.0]);
}

#[test]
fn target_rect_uses_clipped_stamp_bounds() {
    let raw = stamp_to_raw(
        Stamp {
            x: 95.0,
            y: 95.0,
            radius: 10.0,
            rgba: [0.0; 4],
            source_center: [50.0, 50.0],
        },
        1.0,
        100,
        100,
    );
    assert_eq!(
        raw.target_rect(),
        TextureRect {
            x: 85,
            y: 85,
            width: 15,
            height: 15,
        }
    );
}

#[test]
fn accepted_stamps_accumulate_clipped_dirty_bounds() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(5.0, 5.0));
    assert!(queue.queue_point(point(5.0, 5.0), [0.0; 4], 100, 100));
    assert!(queue.queue_point(point(95.0, 95.0), [0.0; 4], 100, 100));
    assert_eq!(
        queue.end_stroke().map(|(rect, _)| rect),
        Some(TextureRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        })
    );
    assert_eq!(queue.end_stroke(), None);
}

#[test]
fn diagonal_stamps_dirty_only_the_tiles_they_touch() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(100.0, 100.0));
    assert!(queue.queue_point(point(100.0, 100.0), [0.0; 4], 1024, 1024));
    assert!(queue.queue_point(point(900.0, 900.0), [0.0; 4], 1024, 1024));

    let (rect, tiles) = queue.end_stroke().expect("stroke dirtied the document");
    assert_eq!(tile_spans(rect).count(), 4);
    assert_eq!(
        tiles.into_iter().collect::<Vec<_>>(),
        [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 1 }]
    );
}

#[test]
fn stamp_across_a_tile_edge_dirties_both_tiles() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(512.0, 100.0));
    assert!(queue.queue_point(point(512.0, 100.0), [0.0; 4], 1024, 1024));

    let (_, tiles) = queue.end_stroke().expect("stroke dirtied the document");
    assert_eq!(
        tiles.into_iter().collect::<Vec<_>>(),
        [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
    );
}

#[test]
fn off_canvas_stamps_leave_dirty_bounds_empty() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(-20.0, -20.0));
    assert!(!queue.queue_point(point(-20.0, -20.0), [0.0; 4], 100, 100));
    assert_eq!(queue.end_stroke(), None);
}

#[test]
fn rejected_stamps_advance_smudge_source_without_dirtying() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(5.0, 5.0));
    assert!(!queue.queue_point(point(-20.0, -20.0), [0.0; 4], 100, 100));
    assert!(queue.pending.is_empty());
    assert_eq!(queue.dirty_rect, None);

    assert!(queue.queue_point(point(5.0, 5.0), [0.0; 4], 100, 100));
    assert_eq!(queue.pending[0].source_center, [-20.0, -20.0]);
}

#[test]
fn reentry_uses_the_previous_generated_center() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(5.0, 5.0));
    assert!(!queue.queue_point(point(-20.0, 5.0), [0.0; 4], 100, 100));
    assert!(queue.queue_point(point(-9.0, 5.0), [0.0; 4], 100, 100));

    assert_eq!(queue.pending[0].source_center, [-20.0, 5.0]);
}

#[test]
fn stamps_retain_previous_center_without_dirtying_origin() {
    let mut queue = StampQueue::default();
    queue.begin_stroke(point(90.0, 90.0));
    assert!(queue.queue_point(point(10.0, 10.0), [0.0; 4], 100, 100));
    assert!(queue.queue_point(point(30.0, 10.0), [0.0; 4], 100, 100));

    assert_eq!(queue.pending[0].source_center, [90.0, 90.0]);
    assert_eq!(queue.pending[1].source_center, [10.0, 10.0]);
    assert_eq!(
        queue.end_stroke().map(|(rect, _)| rect),
        Some(TextureRect {
            x: 0,
            y: 0,
            width: 41,
            height: 21,
        })
    );

    queue.begin_stroke(point(50.0, 50.0));
    assert_eq!(queue.end_stroke(), None);
}

#[test]
fn preview_stamps_do_not_advance_committed_spacing() {
    let mut queue = StampQueue::default();
    let start = point(10.0, 10.0);
    let committed = point(20.0, 10.0);
    let preview_end = point(35.0, 15.0);
    let spacing = BrushSpacing {
        ratio: 0.5,
        minimum: 1.0,
    };
    queue.begin_stroke(start);
    queue.stamp_line(start, committed, [0.0; 4], spacing, 100, 100);
    let pending = queue.pending.len();
    let distance = queue.distance_since_last_stamp;
    let center = queue.last_generated_center;
    let dirty = queue.dirty_rect;

    let preview = queue.preview_stamps(committed, [preview_end], [0.0; 4], spacing, 100, 100);

    assert_eq!(preview.last().map(|stamp| stamp.center), Some([35.0, 15.0]));
    assert_eq!(queue.pending.len(), pending);
    assert_eq!(queue.distance_since_last_stamp, distance);
    assert_eq!(queue.last_generated_center, center);
    assert_eq!(queue.dirty_rect, dirty);
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
        queue.stamp_line(
            segment[0],
            segment[1],
            color,
            BrushSpacing::default(),
            1000,
            1000,
        );
    }

    let expected_spacing = stamp_spacing(input[0].radius, BrushSpacing::default());
    assert!(queue.pending.len() > 100);
    assert!(
        queue
            .pending
            .iter()
            .zip(queue.pending.iter().skip(1))
            .all(|(from, to)| (to.x - from.x).hypot(to.y - from.y) <= expected_spacing + 1.0e-3)
    );
}
