@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> layer: LayerSettings;
@group(1) @binding(0) var tileTexture: texture_2d<f32>;
@group(1) @binding(1) var<uniform> tile: Tile;

// Must match `tiles::TILE_SIZE`.
const TILE_SIZE: i32 = 512;

struct LayerSettings {
  opacity: f32,
  blendMode: u32,
};

struct View {
  documentFromWindowX: vec4f,
  documentFromWindowY: vec4f,
  paintDims: vec2f,
  padding: vec2f,
  backgroundColor: vec4f,
};

struct Tile {
  origin: vec2f,
  padding: vec2f,
};

@vertex
fn vs(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
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

fn is_outside_canvas(document: vec2f) -> bool {
  let uv = document / view.paintDims;
  return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

@fragment
fn fs_background(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  if (is_outside_canvas(document_position(pos))) {
    return vec4f(0.0);
  }
  return view.backgroundColor;
}

fn layer_color(pos: vec4f) -> vec4f {
  let document = document_position(pos);
  if (is_outside_canvas(document)) {
    return vec4f(0.0);
  }
  // Scissor rects of neighboring tiles overlap, so each tile draws only its own texels.
  let texel = vec2i(floor(document)) - vec2i(tile.origin);
  if (any(texel < vec2i(0)) || any(texel >= vec2i(TILE_SIZE))) {
    return vec4f(0.0);
  }
  return textureLoad(tileTexture, texel, 0);
}

@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  // Paint textures are premultiplied, so opacity scales every channel.
  return layer_color(pos) * layer.opacity;
}

// A copy of the canvas under the current scissor rect, in window pixels.
@group(2) @binding(0) var backdropTexture: texture_2d<f32>;

// Replaces the target, so texels outside this tile must reproduce the backdrop unchanged.
@fragment
fn fs_blend_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let backdrop = textureLoad(backdropTexture, vec2i(pos.xy), 0);
  return blend_premultiplied(layer.blendMode, layer_color(pos), backdrop, false);
}
