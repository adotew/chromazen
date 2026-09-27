@group(0) @binding(0) var previewSampler: sampler;
@group(0) @binding(1) var<uniform> preview: Preview;
@group(1) @binding(0) var tileTexture: texture_2d<f32>;
@group(1) @binding(1) var<uniform> tile: Tile;

// Must match `tiles::TILE_SIZE`.
const TILE_SIZE: f32 = 512.0;

struct Preview {
  previewDims: vec2f,
  documentDims: vec2f,
};

struct Tile {
  origin: vec2f,
  padding: vec2f,
};

@vertex
fn vs_preview(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn preview_uv(pos: vec4f) -> vec2f {
  let scale = min(
    preview.previewDims.x / preview.documentDims.x,
    preview.previewDims.y / preview.documentDims.y,
  );
  let contentDims = preview.documentDims * scale;
  let origin = (preview.previewDims - contentDims) * 0.5;
  return (pos.xy - origin) / contentDims;
}

fn is_outside_document(uv: vec2f) -> bool {
  return any(uv < vec2f(0.0)) || any(uv > vec2f(1.0));
}

// Each tile is drawn separately into a cleared thumbnail, so pixels owned by other tiles are
// discarded rather than overwritten.
@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = preview_uv(pos);
  if is_outside_document(uv) {
    discard;
  }
  let local = uv * preview.documentDims - tile.origin;
  if any(local < vec2f(0.0)) || any(local >= vec2f(TILE_SIZE)) {
    discard;
  }
  return textureSampleLevel(tileTexture, previewSampler, local / TILE_SIZE, 0.0);
}
