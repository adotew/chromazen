use super::history::TextureRect;

const SUBSAMPLES: usize = 4;

#[derive(Debug)]
pub(crate) struct SelectionMask {
    pub(crate) rect: TextureRect,
    pub(crate) coverage: Vec<u8>,
}

impl SelectionMask {
    pub(crate) fn at(&self, x: u32, y: u32) -> u8 {
        let rect = self.rect;
        if x < rect.x || y < rect.y || x >= rect.x + rect.width || y >= rect.y + rect.height {
            return 0;
        }
        self.coverage[((y - rect.y) * rect.width + (x - rect.x)) as usize]
    }
}

struct Edge {
    top: [f32; 2],
    bottom: [f32; 2],
    winding: i32,
}

impl Edge {
    fn x_at(&self, y: f32) -> f32 {
        let t = (y - self.top[1]) / (self.bottom[1] - self.top[1]);
        self.top[0] + t * (self.bottom[0] - self.top[0])
    }
}

pub(crate) fn rasterize(polygon: &[[f32; 2]], size: [u32; 2]) -> Option<SelectionMask> {
    if polygon.len() < 3
        || polygon
            .iter()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
    {
        return None;
    }
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for point in polygon {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    let x0 = min[0].floor().max(0.0) as u32;
    let y0 = min[1].floor().max(0.0) as u32;
    let x1 = (max[0].ceil().max(0.0) as u32).min(size[0]);
    let y1 = (max[1].ceil().max(0.0) as u32).min(size[1]);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let rect = TextureRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    };

    let mut edges: Vec<_> = polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .filter(|(from, to)| from[1] != to[1])
        .map(|(&from, &to)| {
            let local = |point: [f32; 2]| [point[0] - x0 as f32, point[1]];
            if from[1] < to[1] {
                Edge {
                    top: local(from),
                    bottom: local(to),
                    winding: 1,
                }
            } else {
                Edge {
                    top: local(to),
                    bottom: local(from),
                    winding: -1,
                }
            }
        })
        .collect();
    edges.sort_by(|a, b| a.top[1].total_cmp(&b.top[1]));

    let width = rect.width as usize;
    let mut coverage = vec![0; width * rect.height as usize];
    let mut row = vec![0.0_f32; width];
    let mut runs = vec![0.0_f32; width + 1];
    let mut active: Vec<&Edge> = Vec::new();
    let mut next_edge = 0;
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    let weight = 1.0 / SUBSAMPLES as f32;
    let mut any_coverage = false;

    for (row_index, y) in (y0..y1).enumerate() {
        row.fill(0.0);
        runs.fill(0.0);
        for sample in 0..SUBSAMPLES {
            let sample_y = y as f32 + (sample as f32 + 0.5) * weight;
            while next_edge < edges.len() && edges[next_edge].top[1] <= sample_y {
                active.push(&edges[next_edge]);
                next_edge += 1;
            }
            active.retain(|edge| edge.bottom[1] > sample_y);
            crossings.clear();
            crossings.extend(
                active
                    .iter()
                    .map(|edge| (edge.x_at(sample_y), edge.winding)),
            );
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut winding = 0;
            let mut span_start = 0.0;
            for &(x, direction) in &crossings {
                let was_inside = winding != 0;
                winding += direction;
                if !was_inside && winding != 0 {
                    span_start = x;
                } else if was_inside && winding == 0 {
                    add_span(&mut row, &mut runs, span_start, x, weight);
                }
            }
        }

        let mut run = 0.0;
        let output = &mut coverage[row_index * width..(row_index + 1) * width];
        for ((value, partial), delta) in output.iter_mut().zip(&row).zip(&runs) {
            run += delta;
            *value = ((partial + run).clamp(0.0, 1.0) * 255.0).round() as u8;
            any_coverage |= *value != 0;
        }
    }

    any_coverage.then_some(SelectionMask { rect, coverage })
}

fn add_span(row: &mut [f32], runs: &mut [f32], start: f32, end: f32, weight: f32) {
    let width = row.len() as f32;
    let (start, end) = (start.clamp(0.0, width), end.clamp(0.0, width));
    if end <= start {
        return;
    }
    let first = start.floor() as usize;
    let last = end.floor() as usize;
    if first == last {
        row[first] += (end - start) * weight;
        return;
    }
    row[first] += (first as f32 + 1.0 - start) * weight;
    runs[first + 1] += weight;
    runs[last] -= weight;
    if last < row.len() {
        row[last] += (end - last as f32) * weight;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_is_filled_inside_and_empty_outside() {
        let mask = rasterize(
            &[[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]],
            [64, 64],
        )
        .expect("square covers pixels");
        assert_eq!(
            mask.rect,
            TextureRect {
                x: 10,
                y: 10,
                width: 10,
                height: 10
            }
        );
        assert_eq!(mask.at(10, 10), 255);
        assert_eq!(mask.at(19, 19), 255);
        assert_eq!(mask.at(9, 15), 0);
        assert_eq!(mask.at(20, 15), 0);
    }

    #[test]
    fn edges_crossing_pixels_are_antialiased() {
        let mask = rasterize(
            &[[0.0, 0.0], [10.5, 0.0], [10.5, 10.0], [0.0, 10.0]],
            [64, 64],
        )
        .expect("rectangle covers pixels");
        assert_eq!(mask.at(9, 5), 255);
        assert!((127..=128).contains(&mask.at(10, 5)));
    }

    #[test]
    fn overlapping_loops_use_the_nonzero_rule() {
        let square = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
        let twice: Vec<_> = square.iter().chain(&square).copied().collect();
        let mask = rasterize(&twice, [16, 16]).expect("square covers pixels");
        assert_eq!(mask.at(4, 4), 255);
    }

    #[test]
    fn coverage_is_clipped_to_the_document() {
        let mask = rasterize(
            &[[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]],
            [4, 4],
        )
        .expect("polygon overlaps the document");
        assert_eq!(
            mask.rect,
            TextureRect {
                x: 0,
                y: 0,
                width: 4,
                height: 4
            }
        );
        assert!(mask.coverage.iter().all(|&value| value == 255));
    }

    #[test]
    fn degenerate_or_outside_polygons_select_nothing() {
        assert!(rasterize(&[[0.0, 0.0], [5.0, 5.0]], [16, 16]).is_none());
        assert!(rasterize(&[[0.0, 0.0], [5.0, 5.0], [10.0, 10.0]], [16, 16]).is_none());
        assert!(rasterize(&[[20.0, 20.0], [30.0, 20.0], [30.0, 30.0]], [16, 16]).is_none());
        assert!(rasterize(&[[0.0, 0.0], [f32::NAN, 5.0], [10.0, 10.0]], [16, 16]).is_none());
    }
}
