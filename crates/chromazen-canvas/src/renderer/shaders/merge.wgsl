@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var paintTexture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var<uniform> layer: LayerSettings;

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

@vertex
fn vs(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

@fragment
fn fs_group(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let window = vec3f(position.xy, 1.0);
  let document = vec2f(dot(view.documentFromWindowX.xyz, window), dot(view.documentFromWindowY.xyz, window));
  if (any(document < vec2f(0.0)) || any(document >= view.paintDims)) {
    return vec4f(0.0);
  }
  return textureSampleLevel(paintTexture, paintSampler, document / view.paintDims, 0.0) * layer.opacity;
}

@fragment
fn fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let dimensions = vec2f(textureDimensions(paintTexture));
  let uv = position.xy / dimensions;
  // Layer textures are premultiplied, so opacity scales every channel.
  return textureSampleLevel(paintTexture, paintSampler, uv, 0.0) * layer.opacity;
}
