use std::sync::Arc;

use winit::{dpi::PhysicalSize, window::Window};

mod frost;

use frost::FrostRenderer;

pub(crate) struct GpuContext {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    frost: FrostRenderer,
    fallback_frame: Option<FallbackFrame>,
}

struct FallbackFrame {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    blitter: wgpu::util::TextureBlitter,
}

impl FallbackFrame {
    fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        let (texture, view) = create_frame_texture(device, size, format);
        Self {
            _texture: texture,
            view,
            blitter: wgpu::util::TextureBlitter::new(device, format),
        }
    }

    fn resize(&mut self, device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) {
        let (texture, view) = create_frame_texture(device, size, format);
        self._texture = texture;
        self.view = view;
    }
}

fn create_frame_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fallback frame texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

impl GpuContext {
    pub(crate) async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|err| format!("failed to create surface: {err}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|err| format!("failed to find a suitable GPU adapter: {err}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("chromazen device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: Default::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| format!("failed to create device: {err}"))?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = egui_wgpu::preferred_framebuffer_format(&caps.formats)
            .unwrap_or_else(|_| caps.formats[0]);
        let frost_supported = caps.usages.contains(wgpu::TextureUsages::TEXTURE_BINDING);
        let frame_copy_supported = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if !frost_supported {
            log::debug!("using an offscreen frame for frosted surfaces");
        }
        let surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | if frost_supported {
                wgpu::TextureUsages::TEXTURE_BINDING
            } else {
                wgpu::TextureUsages::empty()
            }
            | if frame_copy_supported {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            };
        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![surface_format],
            // Balance latency and throughput; Metal maps this to three drawables.
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let frost = FrostRenderer::new(&device, [config.width, config.height]);
        let fallback_frame = (!frost_supported || !frame_copy_supported)
            .then(|| FallbackFrame::new(&device, [config.width, config.height], config.format));

        Ok(Self {
            surface,
            device,
            queue,
            config,
            frost,
            fallback_frame,
        })
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub(crate) fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub(crate) fn surface_size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    pub(crate) fn frost_view(&self) -> &wgpu::TextureView {
        self.frost.view()
    }

    pub(crate) fn frost_generation(&self) -> u64 {
        self.frost.generation()
    }

    pub(crate) fn fallback_frame(&self) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
        self.fallback_frame
            .as_ref()
            .map(|frame| (&frame._texture, &frame.view))
    }

    pub(crate) fn render_frost(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
    ) {
        self.frost.render(&self.device, encoder, source);
    }

    pub(crate) fn blit_fallback_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        if let Some(frame) = &self.fallback_frame {
            frame
                .blitter
                .copy(&self.device, encoder, &frame.view, target);
        }
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.frost.resize(&self.device, [size.width, size.height]);
        if let Some(frame) = &mut self.fallback_frame {
            frame.resize(&self.device, [size.width, size.height], self.config.format);
        }
        self.reconfigure_surface();
    }

    pub(crate) fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }

    pub(crate) fn reconfigure_surface(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}
