use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use winit::{dpi::PhysicalSize, window::Window};

mod history;
mod layers;
mod resources;
mod stamps;
mod view;

pub(crate) use self::layers::{LayerId, LayerInfo, LayerSelection, LayerSnapshot};
use self::{
    history::{HistoryTarget, PaintHistory, TextureRect},
    layers::{PaintLayer, insertion_index, layer_name, replacement_index_after_delete},
    resources::RenderResources,
    stamps::{MAX_STAMPS_PER_FRAME, StampQueue, StampRaw},
    view::PaintView,
};
use crate::{
    config::LoadedBrushPreset,
    gpu::GpuContext,
    paint::{BrushSpacing, PaintTool, StrokePoint},
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
    background_color: [f32; 4],
}

pub struct PaintRenderer {
    gpu: GpuContext,
    document_size: [u32; 2],
    resources: RenderResources,
    layers: Vec<PaintLayer>,
    selection: LayerSelection,
    background_color: [f32; 4],
    next_layer_id: u64,
    next_layer_number: u64,
    stamp_queue: StampQueue,
    stroke_tool: PaintTool,
    history: PaintHistory,
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

        let first_layer =
            resources.create_paint_layer(device, document_size, LayerId(1), "Layer 1".to_owned());
        let stamp_aspect = brush_preset
            .stamp_image
            .as_ref()
            .map_or(1.0, |image| image.width() as f32 / image.height() as f32);
        let history = PaintHistory::new(device, document_size);
        let mut renderer = Self {
            gpu,
            document_size,
            resources,
            layers: vec![first_layer],
            selection: LayerSelection::Paint(LayerId(1)),
            background_color: [1.0; 4],
            next_layer_id: 2,
            next_layer_number: 2,
            stamp_queue: StampQueue::new(stamp_aspect),
            stroke_tool: PaintTool::default(),
            history,
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

    pub fn try_set_brush_preset(&mut self, preset: &LoadedBrushPreset) -> Result<bool, String> {
        if self.stamp_queue.has_pending() {
            return Ok(false);
        }
        self.resources.replace_brush_stamp(
            self.gpu.device(),
            self.gpu.queue(),
            preset.stamp_image.as_ref(),
        )?;
        let stamp_aspect = preset
            .stamp_image
            .as_ref()
            .map_or(1.0, |image| image.width() as f32 / image.height() as f32);
        self.stamp_queue.set_stamp_aspect(stamp_aspect);
        Ok(true)
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

    pub fn can_paint(&self) -> bool {
        self.selected_layer_index().is_some()
    }

    pub(crate) fn layer_snapshot(&self) -> LayerSnapshot {
        LayerSnapshot {
            layers: self
                .layers
                .iter()
                .map(|layer| LayerInfo {
                    id: layer.id,
                    name: layer.name.clone(),
                })
                .collect(),
            selection: self.selection,
            background_color: self.background_color,
        }
    }

    pub(crate) fn layer_views(&self) -> impl Iterator<Item = (LayerId, &wgpu::TextureView)> {
        self.layers.iter().map(|layer| (layer.id, &layer.view))
    }

    pub(crate) fn select_layer(&mut self, id: LayerId) -> bool {
        if self.layers.iter().any(|layer| layer.id == id) {
            self.selection = LayerSelection::Paint(id);
            true
        } else {
            false
        }
    }

    pub(crate) fn select_background(&mut self) {
        self.selection = LayerSelection::Background;
    }

    pub(crate) fn set_background_color(&mut self, color: [u8; 3]) {
        self.background_color = opaque_color(color);
    }

    pub(crate) fn commit_background_color(&mut self, before: [u8; 3], after: [u8; 3]) {
        let before = opaque_color(before);
        let after = opaque_color(after);
        self.background_color = after;
        self.history.record_background_color(before, after);
    }

    pub(crate) fn add_layer(&mut self) -> bool {
        if self.history.stroke_active() || self.stamp_queue.has_pending() {
            return false;
        }
        let selection_before = self.selection;
        let index = insertion_index(
            self.selection,
            self.selected_layer_index(),
            self.layers.len(),
        );
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;
        let name = layer_name(self.next_layer_number);
        self.next_layer_number += 1;
        let layer =
            self.resources
                .create_paint_layer(self.gpu.device(), self.document_size, id, name);
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("new layer clear encoder"),
                });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("new layer clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.layers.insert(index, layer);
        self.selection = LayerSelection::Paint(id);
        self.history
            .record_add(id, index, selection_before, self.layer_texture_byte_len());
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
        true
    }

    pub(crate) fn can_delete_selected_layer(&self) -> bool {
        matches!(self.selection, LayerSelection::Paint(_))
            && self.layers.len() > 1
            && !self.history.stroke_active()
            && !self.stamp_queue.has_pending()
    }

    pub(crate) fn delete_selected_layer(&mut self) -> bool {
        if !self.can_delete_selected_layer() {
            return false;
        }
        let selection_before = self.selection;
        let index = self
            .selected_layer_index()
            .expect("selected layer must exist");
        let replacement_index = replacement_index_after_delete(self.layers.len(), index)
            .expect("deletion requires another paint layer");
        let next_id = self.layers[replacement_index].id;
        let layer = self.layers.remove(index);
        self.selection = LayerSelection::Paint(next_id);
        self.history.record_delete(
            layer,
            index,
            selection_before,
            self.selection,
            self.layer_texture_byte_len(),
        );
        true
    }

    pub fn begin_stroke(&mut self, tool: PaintTool, origin: StrokePoint) {
        let Some(layer_index) = self.selected_layer_index() else {
            return;
        };
        let layer_id = self.layers[layer_index].id;
        if !self.history.begin_stroke(layer_id) {
            return;
        }
        self.stroke_tool = tool;
        let needs_history_sync = self.history.layer_needs_sync(layer_id);
        if needs_history_sync || tool == PaintTool::Smudge {
            let mut encoder =
                self.gpu
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("stroke setup encoder"),
                    });
            if needs_history_sync {
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
            }
            if tool == PaintTool::Smudge {
                // Smudge samples this snapshot because the layer cannot be sampled while attached.
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.layers[layer_index].texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.resources.smudge_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: self.document_size[0],
                        height: self.document_size[1],
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.gpu.queue().submit(std::iter::once(encoder.finish()));
        }
        self.stamp_queue.begin_stroke(origin);
    }

    pub fn end_stroke(&mut self) {
        self.flush_all_stamps();
        let Some(rect) = self.stamp_queue.end_stroke() else {
            self.history.end_empty_stroke();
            return;
        };

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("history commit encoder"),
                });
        let layer_index = self
            .selected_layer_index()
            .expect("stroke requires paint layer");
        let layer_id = self.layers[layer_index].id;
        self.history.commit_stroke(
            self.gpu.device(),
            &mut encoder,
            layer_id,
            &self.layers[layer_index].texture,
            rect,
        );
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
    }

    pub fn can_undo(&self) -> bool {
        !self.stamp_queue.has_pending() && self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        !self.stamp_queue.has_pending() && self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo_target() {
            Some(HistoryTarget::Structure) => self.history.undo_structure(
                &mut self.layers,
                &mut self.selection,
                &mut self.background_color,
            ),
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("undo layer must exist");
                let mut encoder =
                    self.gpu
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("undo encoder"),
                        });
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
                self.history
                    .undo_stroke(&mut encoder, &self.layers[layer_index].texture);
                self.gpu.queue().submit(std::iter::once(encoder.finish()));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo_target() {
            Some(HistoryTarget::Structure) => self.history.redo_structure(
                &mut self.layers,
                &mut self.selection,
                &mut self.background_color,
            ),
            Some(HistoryTarget::Stroke(layer_id)) => {
                let layer_index = self
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                    .expect("redo layer must exist");
                let mut encoder =
                    self.gpu
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("redo encoder"),
                        });
                self.history.ensure_layer_synced(
                    &mut encoder,
                    layer_id,
                    &self.layers[layer_index].texture,
                    self.document_size,
                );
                self.history
                    .redo_stroke(&mut encoder, &self.layers[layer_index].texture);
                self.gpu.queue().submit(std::iter::once(encoder.finish()));
                true
            }
            None => false,
        }
    }

    pub fn queue_stamp(&mut self, point: StrokePoint, color: [f32; 4]) -> bool {
        self.can_paint()
            && self.stamp_queue.queue_point(
                point,
                color,
                self.document_size[0],
                self.document_size[1],
            )
    }

    pub fn stamp_line(
        &mut self,
        from: StrokePoint,
        to: StrokePoint,
        color: [f32; 4],
        spacing: BrushSpacing,
    ) -> usize {
        if !self.can_paint() {
            return 0;
        }
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
        self.history.clear();
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("clear canvas encoder"),
                });
        let layer_index = self
            .selected_layer_index()
            .expect("clear requires paint layer");
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.layers[layer_index].view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.history.sync_layer(
            &mut encoder,
            self.layers[layer_index].id,
            &self.layers[layer_index].texture,
            TextureRect {
                x: 0,
                y: 0,
                width: self.document_size[0],
                height: self.document_size[1],
            },
        );
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
        pass.set_pipeline(&self.resources.background_pipeline);
        pass.set_bind_group(0, &self.layers[0].blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.resources.layer_pipeline);
        for layer in &self.layers {
            pass.set_bind_group(0, &layer.blit_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn flush_all_stamps(&mut self) {
        while self.stamp_queue.has_pending() {
            let mut encoder =
                self.gpu
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("stroke flush encoder"),
                    });
            self.flush_stamps(&mut encoder);
            self.gpu.queue().submit(std::iter::once(encoder.finish()));
        }
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

        let layer_index = self
            .selected_layer_index()
            .expect("stamp requires paint layer");
        if self.stroke_tool == PaintTool::Smudge {
            self.flush_smudge_stamps(encoder, layer_index, &raw);
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.layers[layer_index].view,
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
        pass.set_pipeline(match self.stroke_tool {
            PaintTool::Brush => &self.resources.stamp_pipeline,
            PaintTool::Eraser => &self.resources.eraser_pipeline,
            PaintTool::Smudge => unreachable!("smudge stamps use their own passes"),
        });
        pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
        pass.draw(0..6, 0..count as u32);
    }

    fn flush_smudge_stamps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layer_index: usize,
        stamps: &[StampRaw],
    ) {
        for (index, stamp) in stamps.iter().copied().enumerate() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("smudge pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.layers[layer_index].view,
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
                pass.set_pipeline(&self.resources.smudge_pipeline);
                pass.set_bind_group(0, &self.resources.stamp_bind_group, &[]);
                pass.draw(0..6, index as u32..index as u32 + 1);
            }

            // The next dab must sample the result of this one.
            let target = stamp.target_rect();
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.layers[layer_index].texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: target.x,
                        y: target.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.resources.smudge_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: target.x,
                        y: target.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: target.width,
                    height: target.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn layer_texture_byte_len(&self) -> u64 {
        u64::from(self.document_size[0]) * u64::from(self.document_size[1]) * 4
    }

    fn selected_layer_id(&self) -> Option<LayerId> {
        match self.selection {
            LayerSelection::Background => None,
            LayerSelection::Paint(id) => Some(id),
        }
    }

    fn selected_layer_index(&self) -> Option<usize> {
        let id = self.selected_layer_id()?;
        self.layers.iter().position(|layer| layer.id == id)
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
                background_color: self.background_color,
            }),
        );
    }
}

fn opaque_color(color: [u8; 3]) -> [f32; 4] {
    [
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        1.0,
    ]
}
