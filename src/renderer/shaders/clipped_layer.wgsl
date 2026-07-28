@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var layerTexture: texture_2d<f32>;
@group(0) @binding(2) var baseTexture: texture_2d<f32>;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> layer: LayerSettings;
@group(0) @binding(5) var<uniform> base: LayerSettings;

struct LayerSettings {
  opacity: f32,
};

struct View {
  scale: vec2f,
  offset: vec2f,
  paintDims: vec2f,
  padding: vec2f,
  backgroundColor: vec4f,
};

@vertex
fn vs(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

@vertex
fn vs_merge(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = (pos.xy * view.scale + view.offset) / view.paintDims;
  if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0))) {
    return vec4f(0.0);
  }
  let mask = textureSampleLevel(baseTexture, paintSampler, uv, 0.0).a * base.opacity;
  return textureSampleLevel(layerTexture, paintSampler, uv, 0.0) * layer.opacity * mask;
}

@fragment
fn fs_merge(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = pos.xy / vec2f(textureDimensions(layerTexture));
  let mask = textureSampleLevel(baseTexture, paintSampler, uv, 0.0).a * base.opacity;
  return textureSampleLevel(layerTexture, paintSampler, uv, 0.0) * layer.opacity * mask;
}
