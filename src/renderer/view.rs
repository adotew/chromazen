const MIN_ZOOM: f32 = 0.01;
const MAX_ZOOM: f32 = 32.0;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintViewSnapshot {
    pub(crate) zoom: f32,
    pub(super) center: [f32; 2],
    pub(super) viewport_center: [f32; 2],
    pub(super) rotation: f32,
    pub(super) flip: [f32; 2],
}

impl PaintViewSnapshot {
    pub(crate) fn document_to_window(self, point: [f32; 2]) -> [f32; 2] {
        let delta = [
            (point[0] - self.center[0]) * self.zoom,
            (point[1] - self.center[1]) * self.zoom,
        ];
        let oriented = orient(delta, self.rotation, self.flip);
        [
            self.viewport_center[0] + oriented[0],
            self.viewport_center[1] + oriented[1],
        ]
    }

    pub(crate) fn window_to_document(self, point: [f32; 2]) -> [f32; 2] {
        let delta = [
            point[0] - self.viewport_center[0],
            point[1] - self.viewport_center[1],
        ];
        let document_delta = inverse_orient(delta, self.rotation, self.flip);
        [
            self.center[0] + document_delta[0] / self.zoom,
            self.center[1] + document_delta[1] / self.zoom,
        ]
    }

    pub(crate) fn window_delta_to_document(self, delta: [f32; 2]) -> [f32; 2] {
        let delta = inverse_orient(delta, self.rotation, self.flip);
        [delta[0] / self.zoom, delta[1] / self.zoom]
    }

    pub(crate) fn rotation(self) -> f32 {
        self.rotation
    }

    pub(crate) fn document_axes_in_window(self) -> ([f32; 2], [f32; 2]) {
        (
            orient([1.0, 0.0], self.rotation, self.flip),
            orient([0.0, 1.0], self.rotation, self.flip),
        )
    }

    pub(crate) fn window_to_document_rows(self) -> ([f32; 4], [f32; 4]) {
        let origin = self.window_to_document([0.0, 0.0]);
        let x_step = self.window_delta_to_document([1.0, 0.0]);
        let y_step = self.window_delta_to_document([0.0, 1.0]);
        (
            [x_step[0], y_step[0], origin[0], 0.0],
            [x_step[1], y_step[1], origin[1], 0.0],
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintView {
    zoom: f32,
    center: [f32; 2],
    surface_size: [f32; 2],
    rotation: f32,
    flip: [f32; 2],
}

impl Default for PaintView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: [0.0, 0.0],
            surface_size: [1.0, 1.0],
            rotation: 0.0,
            flip: [1.0, 1.0],
        }
    }
}

impl PaintView {
    pub(crate) fn zoom(&self) -> f32 {
        self.zoom
    }

    pub(crate) fn snapshot(&self) -> PaintViewSnapshot {
        PaintViewSnapshot {
            zoom: self.zoom,
            center: self.center,
            viewport_center: [self.surface_size[0] * 0.5, self.surface_size[1] * 0.5],
            rotation: self.rotation,
            flip: self.flip,
        }
    }

    pub(crate) fn set_surface_size(&mut self, surface_size: [u32; 2]) {
        self.surface_size = [surface_size[0] as f32, surface_size[1] as f32];
    }

    pub(crate) fn fit_to_screen(&mut self, surface_size: [u32; 2], document_size: [u32; 2]) {
        self.set_surface_size(surface_size);
        let width = document_size[0] as f32;
        let height = document_size[1] as f32;
        let (sin, cos) = self.rotation.sin_cos();
        let oriented_width = width * cos.abs() + height * sin.abs();
        let oriented_height = width * sin.abs() + height * cos.abs();
        self.zoom = (surface_size[0] as f32 / oriented_width)
            .min(surface_size[1] as f32 / oriented_height)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = [width * 0.5, height * 0.5];
    }

    pub(crate) fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        let old = self.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new - old).abs() <= f32::EPSILON {
            return;
        }
        let document_point = self.snapshot().window_to_document(cursor);
        self.zoom = new;
        let viewport_center = self.snapshot().viewport_center;
        let screen_delta = [
            cursor[0] - viewport_center[0],
            cursor[1] - viewport_center[1],
        ];
        let document_delta = inverse_orient(screen_delta, self.rotation, self.flip);
        self.center = [
            document_point[0] - document_delta[0] / new,
            document_point[1] - document_delta[1] / new,
        ];
    }

    pub(crate) fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        let delta = self.snapshot().window_delta_to_document(delta);
        self.center[0] -= delta[0];
        self.center[1] -= delta[1];
    }

    pub(crate) fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        self.snapshot().window_to_document(point)
    }

    pub(crate) fn set_rotation_around(&mut self, radians: f32, anchor: [f32; 2]) -> bool {
        let radians = normalize_angle(radians);
        if (self.rotation - radians).abs() <= f32::EPSILON {
            return false;
        }
        let anchor_in_window = self.snapshot().document_to_window(anchor);
        self.rotation = radians;
        self.keep_anchor_at_window_point(anchor, anchor_in_window);
        true
    }

    pub(crate) fn rotate_by_around(&mut self, radians: f32, anchor: [f32; 2]) -> bool {
        self.set_rotation_around(self.rotation + radians, anchor)
    }

    pub(crate) fn reset_rotation_around(&mut self, anchor: [f32; 2]) -> bool {
        self.set_rotation_around(0.0, anchor)
    }

    pub(crate) fn toggle_flip_horizontal_around(&mut self, anchor: [f32; 2]) {
        let anchor_in_window = self.snapshot().document_to_window(anchor);
        self.flip[0] = -self.flip[0];
        self.keep_anchor_at_window_point(anchor, anchor_in_window);
    }

    pub(crate) fn toggle_flip_vertical_around(&mut self, anchor: [f32; 2]) {
        let anchor_in_window = self.snapshot().document_to_window(anchor);
        self.flip[1] = -self.flip[1];
        self.keep_anchor_at_window_point(anchor, anchor_in_window);
    }

    fn keep_anchor_at_window_point(&mut self, anchor: [f32; 2], window_point: [f32; 2]) {
        let viewport_center = [self.surface_size[0] * 0.5, self.surface_size[1] * 0.5];
        let screen_delta = [
            window_point[0] - viewport_center[0],
            window_point[1] - viewport_center[1],
        ];
        let document_delta = inverse_orient(screen_delta, self.rotation, self.flip);
        self.center = [
            anchor[0] - document_delta[0] / self.zoom,
            anchor[1] - document_delta[1] / self.zoom,
        ];
    }

    pub(crate) fn reset_orientation(&mut self) {
        self.rotation = 0.0;
        self.flip = [1.0, 1.0];
    }
}

fn orient(point: [f32; 2], rotation: f32, flip: [f32; 2]) -> [f32; 2] {
    let point = [point[0] * flip[0], point[1] * flip[1]];
    let (sin, cos) = rotation.sin_cos();
    [
        point[0] * cos - point[1] * sin,
        point[0] * sin + point[1] * cos,
    ]
}

fn inverse_orient(point: [f32; 2], rotation: f32, flip: [f32; 2]) -> [f32; 2] {
    let (sin, cos) = rotation.sin_cos();
    let unrotated = [
        point[0] * cos + point[1] * sin,
        -point[0] * sin + point[1] * cos,
    ];
    [unrotated[0] * flip[0], unrotated[1] * flip[1]]
}

fn normalize_angle(radians: f32) -> f32 {
    (radians + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(rotation: f32, flip: [f32; 2]) -> PaintViewSnapshot {
        PaintViewSnapshot {
            zoom: 2.0,
            center: [50.0, 40.0],
            viewport_center: [400.0, 300.0],
            rotation,
            flip,
        }
    }

    #[test]
    fn document_and_window_coordinates_round_trip_when_rotated_and_flipped() {
        for rotation in [0.0, 0.37, std::f32::consts::FRAC_PI_2, -2.1] {
            for flip in [[1.0, 1.0], [-1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]] {
                let view = view(rotation, flip);
                let document = [72.0, -13.0];
                let round_trip = view.window_to_document(view.document_to_window(document));
                assert!((round_trip[0] - document[0]).abs() < 0.0001);
                assert!((round_trip[1] - document[1]).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn ninety_degree_rotation_maps_document_axes_to_window_axes() {
        let view = view(std::f32::consts::FRAC_PI_2, [1.0, 1.0]);
        assert_eq!(view.document_to_window([60.0, 40.0]), [400.0, 320.0]);
        assert_eq!(view.document_to_window([50.0, 50.0]), [380.0, 300.0]);
    }

    #[test]
    fn cursor_centered_zoom_preserves_the_document_point() {
        let mut view = PaintView::default();
        view.set_surface_size([800, 600]);
        view.center = [200.0, 100.0];
        view.rotation = 0.7;
        view.flip = [-1.0, 1.0];
        let cursor = [175.0, 480.0];
        let before = view.window_to_document(cursor);
        view.apply_zoom_at(1.1, cursor);
        let after = view.window_to_document(cursor);
        assert!((before[0] - after[0]).abs() < 0.0001);
        assert!((before[1] - after[1]).abs() < 0.0001);
    }

    #[test]
    fn fit_accounts_for_rotated_document_bounds() {
        let mut view = PaintView {
            rotation: std::f32::consts::FRAC_PI_2,
            ..PaintView::default()
        };
        view.fit_to_screen([800, 400], [200, 100]);
        assert_eq!(view.zoom, 2.0);
        assert_eq!(view.center, [100.0, 50.0]);
    }

    #[test]
    fn rotation_keeps_the_requested_document_anchor_stationary() {
        let mut view = PaintView {
            zoom: 1.7,
            center: [30.0, 20.0],
            surface_size: [800.0, 600.0],
            ..PaintView::default()
        };
        let canvas_center = [100.0, 50.0];
        let before = view.snapshot().document_to_window(canvas_center);

        assert!(view.set_rotation_around(0.73, canvas_center));

        let after = view.snapshot().document_to_window(canvas_center);
        assert!((before[0] - after[0]).abs() < 0.0001);
        assert!((before[1] - after[1]).abs() < 0.0001);
    }

    #[test]
    fn flipping_keeps_the_requested_document_anchor_stationary() {
        let mut view = PaintView {
            zoom: 0.8,
            center: [-40.0, 120.0],
            surface_size: [900.0, 500.0],
            rotation: 0.4,
            ..PaintView::default()
        };
        let canvas_center = [200.0, 150.0];
        let before = view.snapshot().document_to_window(canvas_center);

        view.toggle_flip_horizontal_around(canvas_center);
        view.toggle_flip_vertical_around(canvas_center);

        let after = view.snapshot().document_to_window(canvas_center);
        assert!((before[0] - after[0]).abs() < 0.0001);
        assert!((before[1] - after[1]).abs() < 0.0001);
    }

    #[test]
    fn angle_normalization_avoids_unbounded_rotation() {
        assert!(normalize_angle(TAU).abs() < 0.000001);
        assert!(
            (normalize_angle(std::f32::consts::PI * 2.5) - std::f32::consts::FRAC_PI_2).abs()
                < 0.0001
        );
    }
}
