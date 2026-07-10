const MIN_ZOOM: f32 = 0.01;
const MAX_ZOOM: f32 = 32.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintView {
    zoom: f32,
    offset: [f32; 2],
}

impl Default for PaintView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: [0.0, 0.0],
        }
    }
}

impl PaintView {
    pub(crate) fn zoom(&self) -> f32 {
        self.zoom
    }

    pub(crate) fn offset(&self) -> [f32; 2] {
        self.offset
    }

    pub(crate) fn fit_to_screen(&mut self, surface_size: [u32; 2], document_size: [u32; 2]) {
        let zoom = (surface_size[0] as f32 / document_size[0] as f32)
            .min(surface_size[1] as f32 / document_size[1] as f32)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom = zoom;
        let visible_width = surface_size[0] as f32 / zoom;
        let visible_height = surface_size[1] as f32 / zoom;
        self.offset = [
            (document_size[0] as f32 - visible_width) * 0.5,
            (document_size[1] as f32 - visible_height) * 0.5,
        ];
    }

    pub(crate) fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        let old = self.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new - old).abs() <= f32::EPSILON {
            return;
        }
        self.zoom = new;
        self.offset[0] += cursor[0] * (1.0 / old - 1.0 / new);
        self.offset[1] += cursor[1] * (1.0 / old - 1.0 / new);
    }

    pub(crate) fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.offset[0] -= delta[0] / self.zoom;
        self.offset[1] -= delta[1] / self.zoom;
    }

    pub(crate) fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        [
            point[0] / self.zoom + self.offset[0],
            point[1] / self.zoom + self.offset[1],
        ]
    }
}
