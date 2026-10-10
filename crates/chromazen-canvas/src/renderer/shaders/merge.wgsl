@group(0) @binding(1) var<uniform> layer: LayerSettings;
@group(1) @binding(0) var tileTexture: texture_2d<f32>;

struct LayerSettings {
  opacity: f32,
  blendMode: u32,
};

@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

// Renders into a tile at the same coordinate as the source tile.
@fragment
fn fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
  // Layer textures are premultiplied, so opacity scales every channel.
  return textureLoad(tileTexture, vec2i(position.xy), 0) * layer.opacity;
}

// The target tile's contents before this pass.
@group(2) @binding(0) var backdropTile: texture_2d<f32>;

// `tileTexture` already includes the layer's opacity.
fn blend_tile(position: vec4f, clipped: bool) -> vec4f {
  let texel = vec2i(position.xy);
  let source = textureLoad(tileTexture, texel, 0);
  return blend_premultiplied(layer.blendMode, source, textureLoad(backdropTile, texel, 0), clipped);
}

@fragment
fn fs_blend(@builtin(position) position: vec4f) -> @location(0) vec4f {
  return blend_tile(position, false);
}

@fragment
fn fs_blend_clipped(@builtin(position) position: vec4f) -> @location(0) vec4f {
  return blend_tile(position, true);
}
