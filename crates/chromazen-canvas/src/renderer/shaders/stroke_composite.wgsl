@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var strokeMask: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var<uniform> stroke: Stroke;
@group(0) @binding(4) var<uniform> layerSettings: LayerSettings;
@group(0) @binding(5) var<uniform> clippingBaseSettings: LayerSettings;
@group(0) @binding(6) var previewMask: texture_2d<f32>;
// The painted layer's tile. Commit passes bind only the tile uniform because they render into
// the tile.
@group(1) @binding(0) var layerTile: texture_2d<f32>;
@group(1) @binding(1) var<uniform> tile: Tile;
// The clipping base tile at the same coordinate.
@group(2) @binding(0) var clippingBaseTile: texture_2d<f32>;

// Must match `tiles::TILE_SIZE`.
const TILE_SIZE: i32 = 512;

struct LayerSettings {
  opacity: f32,
};

struct View {
  documentFromWindowX: vec4f,
  documentFromWindowY: vec4f,
  paintDims: vec2f,
  padding: vec2f,
  backgroundColor: vec4f,
};

struct Stroke {
  color: vec4f,
};

struct Tile {
  origin: vec2f,
  padding: vec2f,
};

@vertex
fn vs_preview(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn document_position(pos: vec4f) -> vec2f {
  let window = vec3f(pos.xy, 1.0);
  return vec2f(
    dot(view.documentFromWindowX.xyz, window),
    dot(view.documentFromWindowY.xyz, window),
  );
}

fn is_outside_canvas(uv: vec2f) -> bool {
  return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

// Stroke masks cover the whole document and are sampled in document UV space.
fn combined_preview_coverage(uv: vec2f) -> f32 {
  let committed = textureSampleLevel(strokeMask, paintSampler, uv, 0.0).r;
  let preview = textureSampleLevel(previewMask, paintSampler, uv, 0.0).r;
  return max(committed, preview);
}

fn preview_brush(layer: vec4f, uv: vec2f) -> vec4f {
  let coverage = combined_preview_coverage(uv);
  let source = stroke.color * coverage;
  // The composed layer is premultiplied, so opacity scales every channel.
  return (source + layer * (1.0 - source.a)) * layerSettings.opacity;
}

fn preview_eraser(layer: vec4f, uv: vec2f) -> vec4f {
  let coverage = combined_preview_coverage(uv);
  return layer * (1.0 - coverage) * layerSettings.opacity;
}

// Window-space previews draw one tile under a scissor rect that may overlap its neighbors.
fn window_tile_texel(document: vec2f) -> vec2i {
  return vec2i(floor(document)) - vec2i(tile.origin);
}

fn is_outside_tile(texel: vec2i) -> bool {
  return any(texel < vec2i(0)) || any(texel >= vec2i(TILE_SIZE));
}

@fragment
fn fs_preview_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let document = document_position(pos);
  let uv = document / view.paintDims;
  let texel = window_tile_texel(document);
  if (is_outside_canvas(uv) || is_outside_tile(texel)) {
    return vec4f(0.0);
  }
  return preview_brush(textureLoad(layerTile, texel, 0), uv);
}

@fragment
fn fs_preview_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let document = document_position(pos);
  let uv = document / view.paintDims;
  let texel = window_tile_texel(document);
  if (is_outside_canvas(uv) || is_outside_tile(texel)) {
    return vec4f(0.0);
  }
  return preview_eraser(textureLoad(layerTile, texel, 0), uv);
}

@vertex
fn vs_group(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

// Clipping groups are composed one tile at a time in tile space.
fn group_uv(pos: vec4f) -> vec2f {
  return (pos.xy + tile.origin) / view.paintDims;
}

fn clipped_group_source(source: vec4f, texel: vec2i) -> vec4f {
  let baseAlpha = textureLoad(clippingBaseTile, texel, 0).a * clippingBaseSettings.opacity;
  return vec4f(source.rgb * baseAlpha, source.a);
}

@fragment
fn fs_group_preview_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return preview_brush(textureLoad(layerTile, vec2i(pos.xy), 0), group_uv(pos));
}

@fragment
fn fs_group_preview_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return preview_eraser(textureLoad(layerTile, vec2i(pos.xy), 0), group_uv(pos));
}

@fragment
fn fs_group_preview_clipped_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let texel = vec2i(pos.xy);
  let source = preview_brush(textureLoad(layerTile, texel, 0), group_uv(pos));
  return clipped_group_source(source, texel);
}

@fragment
fn fs_group_preview_clipped_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let texel = vec2i(pos.xy);
  let source = preview_eraser(textureLoad(layerTile, texel, 0), group_uv(pos));
  return clipped_group_source(source, texel);
}

@vertex
fn vs_commit(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

// Commit passes render into a layer tile.
fn committed_coverage(pos: vec4f) -> f32 {
  return textureLoad(strokeMask, vec2i(pos.xy) + vec2i(tile.origin), 0).r;
}

@fragment
fn fs_commit_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return stroke.color * committed_coverage(pos);
}

@fragment
fn fs_commit_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return vec4f(0.0, 0.0, 0.0, committed_coverage(pos));
}
