@group(0) @binding(0) var<uniform> layer: LayerSettings;
@group(0) @binding(1) var<uniform> base: LayerSettings;
@group(1) @binding(0) var layerTexture: texture_2d<f32>;
@group(2) @binding(0) var baseTexture: texture_2d<f32>;

struct LayerSettings {
  opacity: f32,
};

@vertex
fn vs_merge(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

// Renders into a tile at the same coordinate as both source tiles.
@fragment
fn fs_merge(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let texel = vec2i(pos.xy);
  let baseAlpha = textureLoad(baseTexture, texel, 0).a * base.opacity;
  let source = textureLoad(layerTexture, texel, 0) * layer.opacity;
  // Clipped layers are blended inside a transparent clipping group. Mask color by the
  // base alpha, but keep the layer's own alpha as the blend coverage. The pipeline
  // preserves the group's alpha, which is established by the base layer.
  return vec4f(source.rgb * baseAlpha, source.a);
}
