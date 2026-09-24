//! GPU residency for immutable tile versions. The layer/history indexes own
//! versions; this table owns their current GPU representation, not their lifetime.
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use crate::tiles::{TileBounds, TileVersion, TileVersionId};

use super::{DOCUMENT_FORMAT, diagnostics::texture_bytes};

pub(super) type PixelVersion = TileVersion<image::RgbaImage>;
pub(super) type PixelRef = Arc<PixelVersion>;

struct ResidentTile {
    version: Weak<PixelVersion>,
    texture: wgpu::Texture,
}

#[derive(Default)]
pub(super) struct TileTextures {
    resident: HashMap<TileVersionId, ResidentTile>,
}

impl TileTextures {
    /// New unpublished pixels. The caller records all writes before publishing
    /// the returned version in a layer/history snapshot; published pixels must
    /// only be modified by forking into another version.
    pub(super) fn allocate(&mut self, device: &wgpu::Device, extent: [u32; 2]) -> PixelRef {
        let version =
            PixelVersion::new(extent, TileBounds::Unknown).expect("validated tile extent");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas tile version"),
            size: wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DOCUMENT_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.resident.insert(
            version.id(),
            ResidentTile {
                version: Arc::downgrade(&version),
                texture,
            },
        );
        version
    }

    pub(super) fn texture(&self, version: &PixelVersion) -> &wgpu::Texture {
        &self
            .resident
            .get(&version.id())
            .expect("tile version must be resident before encoding")
            .texture
    }

    pub(super) fn collect(&mut self) {
        // Dropping a Rust texture handle does not invalidate commands already
        // encoded/submitted: wgpu retains their resources until GPU completion.
        self.resident
            .retain(|_, tile| tile.version.strong_count() != 0);
    }

    pub(super) fn gpu_payload_bytes(&self) -> u64 {
        self.resident
            .values()
            .map(|tile| texture_bytes(&tile.texture))
            .sum()
    }
}
