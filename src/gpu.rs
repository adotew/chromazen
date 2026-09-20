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
    surface_sampleable: bool,
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
        if !frost_supported {
            log::debug!("using the canvas backdrop for frosted surfaces");
        }
        let surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | if frost_supported {
                wgpu::TextureUsages::TEXTURE_BINDING
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

        Ok(Self {
            surface,
            device,
            queue,
            config,
            frost,
            surface_sampleable: frost_supported,
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

    pub(crate) fn surface_is_sampleable(&self) -> bool {
        self.surface_sampleable
    }

    pub(crate) fn render_frost(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
    ) {
        self.frost.render(&self.device, encoder, source);
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.frost.resize(&self.device, [size.width, size.height]);
        self.reconfigure_surface();
    }

    pub(crate) fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }

    pub(crate) fn reconfigure_surface(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}
