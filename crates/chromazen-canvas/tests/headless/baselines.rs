//! Raster oracles for the tiled-storage migration. Integer-aligned square dabs
//! make edge coverage exact; only UNORM blend/filter rounding permits one byte
//! of error. No checked-in, adapter-specific screenshots are needed.
use std::time::Instant;

use chromazen_canvas::{Canvas, LayerTransform, PaintTool, StrokePoint};
use image::{Rgba, RgbaImage};

// Crosses 256, 512 and 1024 boundaries, with partial edge tiles and padded rows.
const SIZE: [u32; 2] = [1031, 773];

pub(super) fn run(device: &wgpu::Device, queue: &wgpu::Queue) {
    let start = Instant::now();
    brush_eraser_and_snapshot(device, queue);
    tiled_fragment_budget_preserves_maximum_coverage(device, queue);
    smudge_matches_sequential_source(device, queue);
    transform_resize_and_duplicate(device, queue);
    clipped_merge_preserves_pixels_and_history(device, queue);
    memory_accounts_for_history_and_readback(device, queue);
    sparse_history_captures_only_first_touched_tiles(device, queue);
    tile_readback_is_cropped_frozen_and_budgeted(device, queue);
    eprintln!("large-canvas raster baselines: {:?}", start.elapsed());
}

fn canvas(device: &wgpu::Device, queue: &wgpu::Queue) -> Canvas {
    Canvas::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        super::RENDER_SIZE,
        SIZE,
        &RgbaImage::from_pixel(1, 1, Rgba([255; 4])),
        [0.16; 3],
    )
    .unwrap()
}

fn pixels(canvas: &Canvas) -> Vec<RgbaImage> {
    canvas
        .begin_document_layer_readback()
        .unwrap()
        .finish()
        .unwrap()
        .into_iter()
        .map(|(_, image)| image)
        .collect()
}

fn load(canvas: &mut Canvas, image: RgbaImage) {
    canvas
        .load_document(&canvas.document_snapshot(), vec![image])
        .unwrap();
}

fn point(x: f32, y: f32, radius: f32) -> StrokePoint {
    StrokePoint {
        x,
        y,
        radius,
        opacity: 1.0,
    }
}

fn stroke(canvas: &mut Canvas, tool: PaintTool, points: &[StrokePoint], opacity: f32) {
    assert!(canvas.begin_stroke(tool, points[0], [1.0, 0.0, 0.0, 1.0], opacity));
    for &point in points {
        assert!(canvas.queue_stamp(point));
    }
    canvas.end_stroke();
}

fn assert_pixels(actual: &RgbaImage, expected: &RgbaImage, rounding: u8) {
    assert_eq!(actual.dimensions(), expected.dimensions());
    for ((x, y, actual), expected) in actual.enumerate_pixels().zip(expected.pixels()) {
        assert!(
            actual
                .0
                .into_iter()
                .zip(expected.0)
                .all(|(a, b)| a.abs_diff(b) <= rounding),
            "pixel ({x}, {y}): {actual:?}, expected {expected:?} (rounding {rounding})"
        );
    }
}

fn brush_eraser_and_snapshot(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    let dabs = [
        point(256.0, 256.0, 12.0),
        point(512.0, 512.0, 20.0),
        point(520.0, 512.0, 20.0),
        point(1024.0, 768.0, 20.0),
    ];
    stroke(&mut canvas, PaintTool::Brush, &dabs, 0.5);
    let expected = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        let covered = dabs.iter().any(|p| {
            (x as f32) >= p.x - p.radius
                && (x as f32) < p.x + p.radius
                && (y as f32) >= p.y - p.radius
                && (y as f32) < p.y + p.radius
        });
        if covered {
            Rgba([128, 0, 0, 128])
        } else {
            Rgba([0; 4])
        }
    });
    // Overlapping dabs in one stroke must not accumulate opacity.
    assert_pixels(&pixels(&canvas)[0], &expected, 1);
    assert!(canvas.undo());
    assert!(pixels(&canvas)[0].as_raw().iter().all(|&v| v == 0));
    assert!(canvas.redo());
    let painted = pixels(&canvas).remove(0);
    let snapshot = canvas.begin_document_layer_readback().unwrap();
    stroke(
        &mut canvas,
        PaintTool::Eraser,
        &[point(512.0, 512.0, 8.0)],
        1.0,
    );
    let mut erased = painted.clone();
    for y in 504..520 {
        for x in 504..520 {
            erased.put_pixel(x, y, Rgba([0; 4]));
        }
    }
    assert_pixels(&pixels(&canvas)[0], &erased, 0);
    assert_pixels(&snapshot.finish().unwrap()[0].1, &painted, 0);
    assert!(canvas.undo());
    assert_pixels(&pixels(&canvas)[0], &painted, 0);
    assert!(canvas.redo());
    assert_pixels(&pixels(&canvas)[0], &erased, 0);
}

fn tiled_fragment_budget_preserves_maximum_coverage(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    let dab = point(512.0, 512.0, 5.0);
    assert!(canvas.begin_stroke(PaintTool::Brush, dab, [1.0, 0.0, 0.0, 1.0], 0.5));
    for _ in 0..300 {
        assert!(canvas.queue_stamp(dab));
    }
    super::render_pixels(device, queue, &mut canvas, None);
    assert!(canvas.has_pending_stamps());
    assert_eq!(canvas.work_counters().committed_dabs, 256);
    assert_eq!(canvas.work_counters().tile_fragments, 1024);
    canvas.end_stroke();
    assert!(!canvas.has_pending_stamps());
    assert_eq!(canvas.work_counters().committed_dabs, 300);
    assert_eq!(canvas.work_counters().tile_fragments, 1200);
    let image = &pixels(&canvas)[0];
    for (x, y, pixel) in image.enumerate_pixels() {
        let expected = if (507..517).contains(&x) && (507..517).contains(&y) {
            Rgba([128, 0, 0, 128])
        } else {
            Rgba([0; 4])
        };
        assert!(
            pixel
                .0
                .into_iter()
                .zip(expected.0)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "({x}, {y}): {pixel:?}, expected {expected:?}"
        );
    }
    assert!(canvas.undo());
    assert!(pixels(&canvas)[0].as_raw().iter().all(|&value| value == 0));
    assert!(canvas.redo());
    assert_pixels(&pixels(&canvas)[0], image, 0);
}

fn smudge_matches_sequential_source(device: &wgpu::Device, queue: &wgpu::Queue) {
    let source = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        let alpha = if x < 512 { 192 } else { 96 };
        Rgba([((x + y) % u32::from(alpha)) as u8, 0, 0, alpha])
    });
    for flush_each_dab in [false, true] {
        let mut canvas = canvas(device, queue);
        load(&mut canvas, source.clone());
        let mut expected = source.clone();
        let origin = point(500.0, 512.0, 12.0);
        assert!(canvas.begin_stroke(PaintTool::Smudge, origin, [0.0; 4], 1.0));
        for x in [504, 508, 512, 516, 520] {
            assert!(canvas.queue_stamp(point(x as f32, 512.0, 12.0)));
            let before = expected.clone();
            for py in 500..524 {
                for px in x - 12..x + 12 {
                    let target = before.get_pixel(px, py);
                    let source = before.get_pixel(px - 4, py);
                    expected.put_pixel(
                        px,
                        py,
                        Rgba(std::array::from_fn(|c| {
                            (f32::from(target[c]) * 0.65 + f32::from(source[c]) * 0.35).round()
                                as u8
                        })),
                    );
                }
            }
            if flush_each_dab {
                super::render_pixels(device, queue, &mut canvas, None);
            }
        }
        canvas.end_stroke();
        let result = pixels(&canvas).remove(0);
        assert_pixels(&result, &expected, 1);
        assert!(canvas.undo());
        assert_pixels(&pixels(&canvas)[0], &source, 0);
        assert!(canvas.redo());
        assert_pixels(&pixels(&canvas)[0], &result, 0);
    }
}

fn transform_resize_and_duplicate(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    let source = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        if (500..524).contains(&x) && (504..520).contains(&y) {
            Rgba([(x % 128) as u8, (y % 128) as u8, 0, 255])
        } else {
            Rgba([0; 4])
        }
    });
    load(&mut canvas, source.clone());
    let bounds = canvas.read_selected_layer_content_bounds().unwrap();
    assert_eq!(bounds.min, [500.0, 504.0]);
    assert_eq!(bounds.max, [524.0, 520.0]);
    let transform = LayerTransform {
        translation: [17.0, -9.0],
        ..Default::default()
    };
    assert!(canvas.update_layer_transform(transform));
    assert!(canvas.cancel_layer_transform());
    assert_pixels(&pixels(&canvas)[0], &source, 0);
    assert!(canvas.update_layer_transform(transform));
    assert!(canvas.commit_layer_transform());
    let translated = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        if x >= 17 && y + 9 < SIZE[1] {
            *source.get_pixel(x - 17, y + 9)
        } else {
            Rgba([0; 4])
        }
    });
    assert_pixels(&pixels(&canvas)[0], &translated, 0);
    assert!(canvas.undo());
    assert_pixels(&pixels(&canvas)[0], &source, 0);
    assert!(canvas.redo());
    assert_pixels(&pixels(&canvas)[0], &translated, 0);

    assert!(canvas.duplicate_selected_layer());
    assert!(canvas.clear_selected_layer());
    let layers = pixels(&canvas);
    assert_pixels(&layers[0], &translated, 0);
    assert!(layers[1].as_raw().iter().all(|&v| v == 0));
    assert!(canvas.undo());
    assert_pixels(&pixels(&canvas)[1], &translated, 0);

    // Non-tile-aligned crop origin, larger destination and two layers.
    let resized = [1040, 780];
    assert!(canvas.resize_canvas(resized, [-7, 5]).unwrap());
    let expected = RgbaImage::from_fn(resized[0], resized[1], |x, y| {
        if x >= 7 && x - 7 < SIZE[0] && y + 5 < SIZE[1] {
            *translated.get_pixel(x - 7, y + 5)
        } else {
            Rgba([0; 4])
        }
    });
    for layer in pixels(&canvas) {
        assert_pixels(&layer, &expected, 0);
    }
    assert!(canvas.undo());
    assert_eq!(canvas.document_size(), SIZE);
    for layer in pixels(&canvas) {
        assert_pixels(&layer, &translated, 0);
    }
    assert!(canvas.redo());
    for layer in pixels(&canvas) {
        assert_pixels(&layer, &expected, 0);
    }
}

fn clipped_merge_preserves_pixels_and_history(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    assert!(canvas.add_layer());
    let mut document = canvas.document_snapshot();
    document.layers[1].clipped = true;
    document.layers[1].opacity = 50;
    let base = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        if (250..530).contains(&x) && (250..530).contains(&y) {
            Rgba([128, 0, 0, 128])
        } else {
            Rgba([0; 4])
        }
    });
    let top = RgbaImage::from_pixel(SIZE[0], SIZE[1], Rgba([0, 0, 128, 128]));
    canvas
        .load_document(&document, vec![base.clone(), top.clone()])
        .unwrap();
    let before_display = super::render_pixels(device, queue, &mut canvas, None);
    assert!(canvas.merge_layer_down(document.layers[1].id));
    let merged = pixels(&canvas).remove(0);
    let expected = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| {
        if base.get_pixel(x, y)[3] == 0 {
            Rgba([0; 4])
        } else {
            Rgba([96, 0, 32, 128])
        }
    });
    assert_pixels(&merged, &expected, 1);
    let after_display = super::render_pixels(device, queue, &mut canvas, None);
    assert!(
        before_display
            .iter()
            .zip(after_display)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    assert!(canvas.undo());
    let restored = pixels(&canvas);
    assert_pixels(&restored[0], &base, 0);
    assert_pixels(&restored[1], &top, 0);
    assert_eq!(canvas.document_snapshot(), document);
    assert!(canvas.redo());
    assert_pixels(&pixels(&canvas)[0], &merged, 0);
}

fn tile_readback_is_cropped_frozen_and_budgeted(device: &wgpu::Device, queue: &wgpu::Queue) {
    use chromazen_canvas::{
        LayerId,
        tiles::{DEFAULT_TILE_SIZE, TileCoord, TileGrid},
    };
    let mut canvas = canvas(device, queue);
    let source = RgbaImage::from_fn(SIZE[0], SIZE[1], |x, y| Rgba([x as u8, y as u8, 137, 255]));
    load(&mut canvas, source.clone());
    let layer = canvas.document_snapshot().selected_layer;
    let grid = TileGrid::new(SIZE, DEFAULT_TILE_SIZE).unwrap();
    for region in grid.intersecting([0; 2], SIZE.map(i64::from)) {
        let readback = canvas
            .begin_layer_tile_readback(layer, region.coord)
            .unwrap();
        let size = grid.extent(region.coord).unwrap();
        let origin = grid.origin(region.coord).unwrap();
        assert_eq!(
            canvas.memory_usage().readbacks,
            u64::from((size[0] * 4).div_ceil(256) * 256) * u64::from(size[1])
        );
        let actual = readback.finish().unwrap();
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].0, layer);
        assert_pixels(
            &actual[0].1,
            &image::imageops::crop_imm(&source, origin[0], origin[1], size[0], size[1]).to_image(),
            0,
        );
        assert_eq!(canvas.memory_usage().readbacks, 0);
    }
    let edge = TileCoord { x: 2, y: 1 };
    let frozen = canvas.begin_layer_tile_readback(layer, edge).unwrap();
    assert!(canvas.clear_selected_layer());
    assert_pixels(
        &frozen.finish().unwrap()[0].1,
        &image::imageops::crop_imm(&source, 1024, 512, 7, 261).to_image(),
        0,
    );
    assert!(canvas.undo());
    assert!(canvas.begin_layer_tile_readback(LayerId(0), edge).is_err());
    assert!(
        canvas
            .begin_layer_tile_readback(layer, TileCoord { x: u32::MAX, y: 0 })
            .is_err()
    );
    assert!(canvas.begin_stroke(PaintTool::Brush, point(10.0, 10.0, 2.0), [1.0; 4], 1.0));
    assert!(canvas.begin_layer_tile_readback(layer, edge).is_err());
    canvas.end_stroke();
    let full = TileCoord { x: 0, y: 0 };
    let mut held: Vec<_> = (0..16)
        .map(|_| canvas.begin_layer_tile_readback(layer, full).unwrap())
        .collect();
    assert_eq!(canvas.memory_usage().readbacks, 16 * 1024 * 1024);
    assert!(canvas.begin_layer_tile_readback(layer, full).is_err());
    held.pop().unwrap().finish().unwrap();
    assert!(
        canvas
            .begin_layer_tile_readback(layer, full)
            .unwrap()
            .finish()
            .is_ok()
    );
    drop(held);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(canvas.memory_usage().readbacks, 0);
}

fn sparse_history_captures_only_first_touched_tiles(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    let near = point(16.0, 16.0, 2.0);
    let far = point(1028.0, 768.0, 2.0);
    assert!(canvas.begin_stroke(PaintTool::Brush, near, [1.0, 0.0, 0.0, 1.0], 1.0));
    assert_eq!(canvas.memory_usage().history, 0);
    assert!(canvas.queue_stamp(near));
    super::render_pixels(device, queue, &mut canvas, None);
    let first_tile_bytes = 512 * 512 * 4;
    assert_eq!(canvas.memory_usage().history, first_tile_bytes);
    assert!(canvas.queue_stamp(near));
    assert!(canvas.queue_stamp(far));
    super::render_pixels(device, queue, &mut canvas, None);
    let touched_bytes = first_tile_bytes + 7 * 261 * 4;
    assert_eq!(canvas.memory_usage().history, touched_bytes);
    canvas.end_stroke();
    assert_eq!(canvas.memory_usage().history, touched_bytes);
    let painted = pixels(&canvas).remove(0);
    assert!(canvas.undo());
    assert!(pixels(&canvas)[0].as_raw().iter().all(|&v| v == 0));
    // The alternate state replaces the retained version, without keeping a
    // document-sized mirror or a rectangle spanning the two distant dabs.
    let undo_bytes = touched_bytes;
    assert_eq!(canvas.memory_usage().history, undo_bytes);
    assert!(canvas.redo());
    assert_eq!(canvas.memory_usage().history, undo_bytes);
    assert_pixels(&pixels(&canvas)[0], &painted, 0);
    assert!(canvas.undo());
    stroke(&mut canvas, PaintTool::Brush, &[far], 1.0);
    assert!(!canvas.can_redo());
    assert_eq!(canvas.memory_usage().history, 7 * 261 * 4);
    assert_eq!(pixels(&canvas)[0].get_pixel(16, 16).0, [0; 4]);
    assert!(canvas.undo());
    assert!(pixels(&canvas)[0].as_raw().iter().all(|&v| v == 0));
}

fn memory_accounts_for_history_and_readback(device: &wgpu::Device, queue: &wgpu::Queue) {
    let mut canvas = canvas(device, queue);
    let initial = canvas.memory_usage();
    let layer_bytes = u64::from(SIZE[0]) * u64::from(SIZE[1]) * 4;
    assert_eq!(initial.layers, layer_bytes + 128 * 128 * 4 + 16);
    assert_eq!(initial.history, 0);
    assert_eq!(
        initial.scratch,
        layer_bytes * 2 + layer_bytes / 2 + 64 * 64 * 4
    );
    assert_eq!(initial.brush, 4);
    assert_eq!(initial.readbacks, 0);
    assert!(canvas.add_layer());
    assert_eq!(canvas.memory_usage().layers, initial.layers * 2);
    assert_eq!(canvas.memory_usage().history, initial.history);
    assert!(canvas.undo());
    assert_eq!(canvas.memory_usage().layers, initial.layers);
    assert_eq!(
        canvas.memory_usage().history,
        initial.history + initial.layers
    );
    assert!(canvas.redo());
    let readback = canvas.begin_document_layer_readback().unwrap();
    let padded_bytes = u64::from((SIZE[0] * 4).div_ceil(256) * 256) * u64::from(SIZE[1]) * 2;
    assert_eq!(canvas.memory_usage().readbacks, padded_bytes);
    assert_eq!(canvas.work_counters().readback_bytes, padded_bytes);
    readback.finish().unwrap();
    assert_eq!(canvas.memory_usage().readbacks, 0);
    let abandoned = canvas.begin_document_layer_readback().unwrap();
    drop(abandoned);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(canvas.memory_usage().readbacks, 0);
    stroke(
        &mut canvas,
        PaintTool::Brush,
        &[point(512.0, 512.0, 20.0)],
        1.0,
    );
    let work = canvas.work_counters();
    assert_eq!(work.committed_dabs, 1);
    assert_eq!(work.tile_fragments, 4);
    assert_eq!(work.paint_passes, 3);
    eprintln!("baseline {:?}; work {work:?}", canvas.memory_usage());
}
