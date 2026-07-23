use chromazen::renderer::PaintRenderer;
use tauri::{EventLoopMessage, LogicalPosition, LogicalSize, Rect, Webview};
use tauri_runtime::window::WindowId as RuntimeWindowId;
use tauri_runtime_wry::{
    Context, EventLoopIterationContext, Message, Plugin, PluginBuilder, WebContextStore,
    WindowMessage,
    tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget},
        window::WindowId,
    },
};

const PAINT_WINDOW_LABEL: &str = "main";

pub(crate) struct RawPaintPluginBuilder {
    paint: PaintRenderer,
    controls: Webview,
    controls_width: f64,
    scale_factor: f64,
}

impl RawPaintPluginBuilder {
    pub(crate) fn new(
        paint: PaintRenderer,
        controls: Webview,
        controls_width: f64,
        scale_factor: f64,
    ) -> Self {
        Self {
            paint,
            controls,
            controls_width,
            scale_factor,
        }
    }
}

impl PluginBuilder<EventLoopMessage> for RawPaintPluginBuilder {
    type Plugin = RawPaintPlugin;

    fn build(self, _context: Context<EventLoopMessage>) -> Self::Plugin {
        RawPaintPlugin {
            paint: self.paint,
            controls: self.controls,
            controls_width: self.controls_width,
            scale_factor: self.scale_factor,
            tao_window_id: None,
            runtime_window_id: None,
            redraw_pending: true,
        }
    }
}

pub(crate) struct RawPaintPlugin {
    paint: PaintRenderer,
    controls: Webview,
    controls_width: f64,
    scale_factor: f64,
    tao_window_id: Option<WindowId>,
    runtime_window_id: Option<RuntimeWindowId>,
    redraw_pending: bool,
}

impl Plugin<EventLoopMessage> for RawPaintPlugin {
    fn on_event(
        &mut self,
        event: &Event<Message<EventLoopMessage>>,
        _event_loop: &EventLoopWindowTarget<Message<EventLoopMessage>>,
        proxy: &EventLoopProxy<Message<EventLoopMessage>>,
        _control_flow: &mut ControlFlow,
        context: EventLoopIterationContext<'_, EventLoopMessage>,
        _web_context: &WebContextStore,
    ) -> bool {
        match event {
            Event::WindowEvent {
                window_id, event, ..
            } if self.is_paint_window(*window_id, &context) => match event {
                WindowEvent::Resized(size) => {
                    self.resize([size.width, size.height]);
                    self.redraw_pending = true;
                }
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    new_inner_size,
                } => {
                    self.scale_factor = *scale_factor;
                    self.resize([new_inner_size.width, new_inner_size.height]);
                    self.redraw_pending = true;
                }
                WindowEvent::CursorMoved { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::KeyboardInput { .. } => {
                    self.redraw_pending = true;
                }
                _ => {}
            },
            Event::MainEventsCleared if self.redraw_pending => {
                self.request_redraw(proxy);
            }
            Event::RedrawRequested(window_id)
                if self.is_paint_window(*window_id, &context) && self.redraw_pending =>
            {
                self.render();
                self.redraw_pending = self.paint.has_pending_stamps();
                if self.redraw_pending {
                    self.request_redraw(proxy);
                }
            }
            _ => {}
        }
        false
    }
}

impl RawPaintPlugin {
    fn is_paint_window(
        &mut self,
        window_id: WindowId,
        context: &EventLoopIterationContext<'_, EventLoopMessage>,
    ) -> bool {
        if let Some(paint_window_id) = self.tao_window_id {
            return paint_window_id == window_id;
        }

        let Some(runtime_id) = context.window_id_map.get(&window_id) else {
            return false;
        };
        let windows = context.windows.0.borrow();
        let is_paint_window = windows
            .get(&runtime_id)
            .is_some_and(|window| window.label() == PAINT_WINDOW_LABEL);
        if is_paint_window {
            self.tao_window_id = Some(window_id);
            self.runtime_window_id = Some(runtime_id);
        }
        is_paint_window
    }

    fn resize(&mut self, size: [u32; 2]) {
        self.paint.resize(size);
        let logical_width = size[0] as f64 / self.scale_factor;
        let logical_height = size[1] as f64 / self.scale_factor;
        let controls_width = self.controls_width.min(logical_width);
        if let Err(error) = self.controls.set_bounds(Rect {
            position: LogicalPosition::new(logical_width - controls_width, 0.0).into(),
            size: LogicalSize::new(controls_width, logical_height).into(),
        }) {
            log::warn!("failed to resize controls webview: {error}");
        }
        self.paint.set_canvas_viewport_size([
            canvas_viewport_width(size[0], self.controls_width, self.scale_factor),
            size[1],
        ]);
    }

    fn request_redraw(&self, proxy: &EventLoopProxy<Message<EventLoopMessage>>) {
        let Some(window_id) = self.runtime_window_id else {
            return;
        };
        if let Err(error) =
            proxy.send_event(Message::Window(window_id, WindowMessage::RequestRedraw))
        {
            log::warn!("failed to request native paint redraw: {error}");
        }
    }

    fn render(&mut self) {
        let frame = match self.paint.acquire_frame() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.paint.reconfigure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.paint
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tauri native paint frame"),
                });
        self.paint.render_to_view(&mut encoder, &view);
        self.paint.queue().submit(std::iter::once(encoder.finish()));
        frame.present();

        // Keep the bounded webview alive for the lifetime of the native runtime plugin.
        let _ = &self.controls;
    }
}

fn canvas_viewport_width(surface_width: u32, controls_width: f64, scale_factor: f64) -> u32 {
    let controls_width = (controls_width * scale_factor).round() as u32;
    surface_width.saturating_sub(controls_width).max(1)
}

#[cfg(test)]
mod tests {
    use super::canvas_viewport_width;

    #[test]
    fn viewport_excludes_physical_controls_width() {
        assert_eq!(canvas_viewport_width(1_280, 300.0, 1.0), 980);
        assert_eq!(canvas_viewport_width(2_560, 300.0, 2.0), 1_960);
    }

    #[test]
    fn viewport_never_becomes_zero() {
        assert_eq!(canvas_viewport_width(200, 300.0, 1.0), 1);
    }
}
