use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use winit::{dpi::PhysicalSize, window::Window};

mod resources;
mod stamps;
mod view;

use self::{
    resources::RenderResources,
    stamps::{MAX_STAMPS_PER_FRAME, StampQueue},
    view::PaintView,
};
use crate::{
    config::LoadedBrushPreset,
    gpu::GpuContext,
    paint::{BrushSpacing, StrokePoint},
};

const DEFAULT_CANVAS_WIDTH: u32 = 4000;
const DEFAULT_CANVAS_HEIGHT: u32 = 4000;
const DOCUMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PaintUniform {
    dims: [f32; 2],
    padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    scale: [f32; 2],
    offset: [f32; 2],
    paint_dims: [f32; 2],
    padding: [f32; 2],
}

pub struct PaintRenderer {
    gpu: GpuContext,
    document_size: [u32; 2],
    resources: RenderResources,
    stamp_queue: StampQueue,
    view: PaintView,
}

impl PaintRenderer {
    pub async fn new(
        window: Arc<Window>,
        brush_preset: &LoadedBrushPreset,
    ) -> Result<Self, String> {
        let gpu = GpuContext::new(window).await?;
        let device = gpu.device();
        let queue = gpu.queue();
        let surface_format = gpu.surface_format();

        let document_size = [DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT];
        let resources = RenderResources::new(
            device,
            queue,
            document_size,
            surface_format,
            brush_preset.stamp_image.as_ref(),
        )?;

        let stamp_aspect = brush_preset
            .stamp_image
            .as_ref()
            .map_or(1.0, |image| image.width() as f32 / image.height() as f32);
        let mut renderer = Self {
            gpu,
            document_size,
            resources,
            stamp_queue: StampQueue::new(stamp_aspect),
            view: PaintView::default(),
        };
        renderer.fit_to_screen();
        renderer.clear_canvas();
        Ok(renderer)
    }

    pub fn device(&self) -> &wgpu::Device {
        self.gpu.device()
    }
    pub fn queue(&self) -> &wgpu::Queue {
        self.gpu.queue()
    }
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.gpu.surface_format()
    }
    pub fn surface_size(&self) -> [u32; 2] {
        self.gpu.surface_size()
    }
    pub fn zoom(&self) -> f32 {
        self.view.zoom()
    }
    pub fn has_pending_stamps(&self) -> bool {
        self.stamp_queue.has_pending()
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
    }

    pub fn fit_to_screen(&mut self) {
        self.view
            .fit_to_screen(self.surface_size(), self.document_size);
    }

    pub fn apply_zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        self.view.apply_zoom_at(factor, cursor);
    }

    pub fn pan_by_window_delta(&mut self, delta: [f32; 2]) {
        self.view.pan_by_window_delta(delta);
    }

    pub fn window_to_document(&self, point: [f32; 2]) -> [f32; 2] {
        self.view.window_to_document(point)
    }

    pub fn begin_stroke(&mut self) {
        self.stamp_queue.reset_spacing();
    }

    pub fn end_stroke(&mut self) {
        self.stamp_queue.reset_spacing();
    }

    pub fn queue_stamp(&mut self, point: StrokePoint, color: [f32; 4]) -> bool {
        self.stamp_queue
            .queue_point(point, color, self.document_size[0], self.document_size[1])
    }

    pub fn stamp_line(
        &mut self,
        from: StrokePoint,
        to: StrokePoint,
        color: [f32; 4],
        spacing: BrushSpacing,
    ) -> usize {
        self.stamp_queue.stamp_line(
            from,
            to,
            color,
            spacing,
            self.document_size[0],
            self.document_size[1],
        )
    }

    pub fn clear_canvas(&mut self) {
        self.stamp_queue.clear();
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("clear canvas encoder"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.resources.paint_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
    }

    pub fn acquire_frame(&self) -> wgpu::CurrentSurfaceTexture {
        self.gpu.acquire_frame()
    }

    pub fn reconfigure_surface(&self) {
        self.gpu.reconfigure_surface();
    }

    pub fn render_to_view(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        self.flush_stamps(encoder);
        self.write_view_uniform();

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.5,
                        b: 0.5,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.resources.blit_pipeline);
        pass.set_bind_group(0, &self.resources.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn flush_stamps(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let raw = self.stamp_queue.drain_raw(
            self.document_size[0],
            self.document_size[1],
            MAX_STAMPS_PER_FRAME,
        );
        let count = raw.len();
        if count == 0 {
            return;
        }

        self.gpu
            .queue()
            .write_buffer(&self.resources.stamp_buffer, 0, bytemuck::cast_slice(&raw));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.resources.paint_texture_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.resources.stamp_pipeline);
        pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    fn write_view_uniform(&self) {
        self.gpu.queue().write_buffer(
            &self.resources.view_uniform_buffer,
            0,
            bytemuck::bytes_of(&ViewUniform {
                scale: [1.0 / self.view.zoom(), 1.0 / self.view.zoom()],
                offset: self.view.offset(),
                paint_dims: [self.document_size[0] as f32, self.document_size[1] as f32],
                padding: [0.0, 0.0],
            }),
        );
    }
}
