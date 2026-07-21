#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerSelection {
    Background,
    Paint(LayerId),
}

pub(crate) struct PaintLayer {
    pub(crate) id: LayerId,
    pub(crate) name: String,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) blit_bind_group: wgpu::BindGroup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayerInfo {
    pub(crate) id: LayerId,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerSnapshot {
    pub(crate) layers: Vec<LayerInfo>,
    pub(crate) selection: LayerSelection,
    pub(crate) background_color: [f32; 4],
}
