use chromazen::renderer::PaintRenderer;
use tauri::{EventLoopMessage, Webview};
use tauri_runtime_wry::{
    Context, EventLoopIterationContext, Message, Plugin, PluginBuilder, WebContextStore,
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
}

impl RawPaintPluginBuilder {
    pub(crate) fn new(paint: PaintRenderer, controls: Webview) -> Self {
        Self { paint, controls }
    }
}

impl PluginBuilder<EventLoopMessage> for RawPaintPluginBuilder {
    type Plugin = RawPaintPlugin;

    fn build(self, _context: Context<EventLoopMessage>) -> Self::Plugin {
        RawPaintPlugin {
            paint: self.paint,
            controls: self.controls,
            tao_window_id: None,
            redraw_pending: true,
        }
    }
}

pub(crate) struct RawPaintPlugin {
    paint: PaintRenderer,
    controls: Webview,
    tao_window_id: Option<WindowId>,
    redraw_pending: bool,
}

impl Plugin<EventLoopMessage> for RawPaintPlugin {
    fn on_event(
        &mut self,
        event: &Event<Message<EventLoopMessage>>,
        _event_loop: &EventLoopWindowTarget<Message<EventLoopMessage>>,
        _proxy: &EventLoopProxy<Message<EventLoopMessage>>,
        _control_flow: &mut ControlFlow,
        context: EventLoopIterationContext<'_, EventLoopMessage>,
        _web_context: &WebContextStore,
    ) -> bool {
        match event {
            Event::WindowEvent {
                window_id, event, ..
            } if self.is_paint_window(*window_id, &context) => {
                match event {
                    WindowEvent::Resized(size) => {
                        self.paint.resize([size.width, size.height]);
                        self.redraw_pending = true;
                    }
                    WindowEvent::CursorMoved { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::KeyboardInput { .. } => {
                        self.redraw_pending = true;
                    }
                    _ => {}
                }
            }
            Event::MainEventsCleared if self.redraw_pending => {
                self.render();
                self.redraw_pending = self.paint.has_pending_stamps();
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
        }
        is_paint_window
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
        self.paint
            .queue()
            .submit(std::iter::once(encoder.finish()));
        frame.present();

        // Keep the bounded webview alive for the lifetime of the native runtime plugin.
        let _ = &self.controls;
    }
}
