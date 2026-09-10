#![cfg(target_arch = "wasm32")]

use std::time::Duration;

use chromazen_canvas::{
    BrushSpacing, Canvas, PaintTool, StrokePoint, StrokePositionFilter, StrokeSmoother,
};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

const DOCUMENT_SIZE: [u32; 2] = [3000, 4000];
const BRUSH_STAMP_SIZE: u32 = 64;

#[wasm_bindgen]
pub struct WebCanvas {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    canvas: Canvas,
    tool: PaintTool,
    color: [f32; 4],
    brush_size: f32,
    scale: f32,
    drawing: bool,
    last_point: Option<StrokePoint>,
    last_raw_point: Option<StrokePoint>,
    position_filter: StrokePositionFilter,
    smoother: StrokeSmoother,
}

#[wasm_bindgen]
impl WebCanvas {
    pub async fn create(
        element: HtmlCanvasElement,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<WebCanvas, JsValue> {
        let width = width.max(1);
        let height = height.max(1);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(element))
            .map_err(js_error)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(js_error)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("chromazen web device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: Default::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(js_error)?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![format],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let canvas = Canvas::new(
            device.clone(),
            queue.clone(),
            format,
            [width, height],
            DOCUMENT_SIZE,
            &round_brush_stamp(),
        )
        .map_err(js_error)?;

        Ok(Self {
            surface,
            device,
            queue,
            config,
            canvas,
            tool: PaintTool::Brush,
            color: [0.08, 0.08, 0.07, 1.0],
            brush_size: 48.0,
            scale: scale.max(1.0),
            drawing: false,
            last_point: None,
            last_raw_point: None,
            position_filter: StrokePositionFilter::default(),
            smoother: StrokeSmoother::default(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.scale = scale.max(1.0);
        self.surface.configure(&self.device, &self.config);
        self.canvas.resize([width, height]);
    }

    #[wasm_bindgen(js_name = beginStroke)]
    pub fn begin_stroke(&mut self, x: f32, y: f32, pressure: f32, time_ms: f64) -> bool {
        if self.drawing || !valid_sample(x, y, pressure, time_ms) {
            return false;
        }
        let point = self.stroke_point(x, y, pressure);
        if !self.canvas.begin_stroke(self.tool, point, self.color, 1.0) {
            return false;
        }

        self.position_filter.reset([x, y], sample_time(time_ms));
        self.smoother.begin(point);
        self.last_point = Some(point);
        self.last_raw_point = Some(point);
        self.drawing = true;
        self.tool == PaintTool::Smudge || self.canvas.queue_stamp(point)
    }

    #[wasm_bindgen(js_name = pushStrokeSamples)]
    pub fn push_stroke_samples(&mut self, samples: &[f32]) -> bool {
        if !self.drawing {
            return false;
        }

        let mut changed = false;
        for sample in samples.chunks_exact(4) {
            let [x, y, pressure, time_ms] = [sample[0], sample[1], sample[2], sample[3]];
            if !valid_sample(x, y, pressure, f64::from(time_ms)) {
                continue;
            }
            let raw_point = self.stroke_point(x, y, pressure);
            self.last_raw_point = Some(raw_point);
            let filtered = self
                .position_filter
                .filter([x, y], sample_time(f64::from(time_ms)));
            let point = self.stroke_point(filtered[0], filtered[1], pressure);
            let points = self.smoother.push(point);
            changed |= self.queue_points(points);
        }
        changed
    }

    #[wasm_bindgen(js_name = endStroke)]
    pub fn end_stroke(&mut self) -> bool {
        if !self.drawing {
            return false;
        }
        let points = match self.last_raw_point {
            Some(point) => self.smoother.finish_at(point),
            None => self.smoother.finish(),
        };
        let changed = self.queue_points(points);
        self.canvas.end_stroke();
        self.drawing = false;
        self.last_point = None;
        self.last_raw_point = None;
        changed
    }

    #[wasm_bindgen(js_name = setTool)]
    pub fn set_tool(&mut self, tool: u8) -> Result<(), JsValue> {
        self.finish_stroke();
        self.tool = match tool {
            0 => PaintTool::Brush,
            1 => PaintTool::Eraser,
            2 => PaintTool::Smudge,
            _ => return Err(JsValue::from_str("unknown paint tool")),
        };
        Ok(())
    }

    #[wasm_bindgen(js_name = setBrushSize)]
    pub fn set_brush_size(&mut self, size: f32) {
        if size.is_finite() {
            self.brush_size = size.clamp(1.0, 300.0);
        }
    }

    #[wasm_bindgen(js_name = setColor)]
    pub fn set_color(&mut self, red: u8, green: u8, blue: u8) {
        self.color = [
            f32::from(red) / 255.0,
            f32::from(green) / 255.0,
            f32::from(blue) / 255.0,
            1.0,
        ];
    }

    pub fn undo(&mut self) -> bool {
        self.finish_stroke();
        self.canvas.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.finish_stroke();
        self.canvas.redo()
    }

    pub fn clear(&mut self) -> bool {
        self.finish_stroke();
        self.canvas.clear_selected_layer()
    }

    pub fn render(&mut self) -> bool {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return true;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return false,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("chromazen web frame"),
            });
        self.canvas.render_to_view(&mut encoder, &view, None);
        self.queue.submit([encoder.finish()]);
        frame.present();
        self.canvas.has_pending_stamps()
    }
}

impl WebCanvas {
    fn stroke_point(&self, x: f32, y: f32, pressure: f32) -> StrokePoint {
        let pressure = pressure.clamp(0.0, 1.0);
        let position = self
            .canvas
            .window_to_document([x * self.scale, y * self.scale]);
        StrokePoint {
            x: position[0],
            y: position[1],
            radius: self.brush_size * (0.25 + pressure * 0.75) * 0.5,
            opacity: 0.12 + pressure * 0.88,
        }
    }

    fn queue_points(&mut self, points: Vec<StrokePoint>) -> bool {
        let mut changed = false;
        for point in points {
            if let Some(previous) = self.last_point {
                changed |= self
                    .canvas
                    .stamp_line(previous, point, BrushSpacing::default())
                    > 0;
            } else {
                changed |= self.canvas.queue_stamp(point);
            }
            self.last_point = Some(point);
        }
        changed
    }

    fn finish_stroke(&mut self) {
        if self.drawing {
            self.end_stroke();
        }
    }
}

fn sample_time(milliseconds: f64) -> Duration {
    Duration::from_secs_f64(milliseconds.clamp(0.0, 86_400_000.0) / 1_000.0)
}

fn valid_sample(x: f32, y: f32, pressure: f32, time_ms: f64) -> bool {
    x.is_finite() && y.is_finite() && pressure.is_finite() && time_ms.is_finite()
}

fn round_brush_stamp() -> image::RgbaImage {
    let center = (BRUSH_STAMP_SIZE as f32 - 1.0) * 0.5;
    image::RgbaImage::from_fn(BRUSH_STAMP_SIZE, BRUSH_STAMP_SIZE, |x, y| {
        let distance = (x as f32 - center).hypot(y as f32 - center);
        let alpha = ((center + 0.5 - distance).clamp(0.0, 1.0) * 255.0) as u8;
        image::Rgba([255, 255, 255, alpha])
    })
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
