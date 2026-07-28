@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var paintTexture: texture_2d<f32>;
@group(0) @binding(3) var<uniform> layer: LayerSettings;

struct LayerSettings {
  opacity: f32,
};

@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let dimensions = vec2f(textureDimensions(paintTexture));
  let uv = position.xy / dimensions;
  // Layer textures are premultiplied, so opacity scales every channel.
  return textureSampleLevel(paintTexture, paintSampler, uv, 0.0) * layer.opacity;
}
