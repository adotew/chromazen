@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var layerTexture: texture_2d<f32>;
@group(0) @binding(2) var strokeMask: texture_2d<f32>;
@group(0) @binding(3) var<uniform> stroke: Stroke;
@group(0) @binding(4) var<uniform> preview: Preview;

struct Stroke {
  color: vec4f,
};

struct Preview {
  dims: vec2f,
  padding: vec2f,
};

@vertex
fn vs_preview(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn previewUv(pos: vec4f) -> vec2f {
  return pos.xy / preview.dims;
}

@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return textureSampleLevel(layerTexture, paintSampler, previewUv(pos), 0.0);
}

@fragment
fn fs_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = previewUv(pos);
  let layer = textureSampleLevel(layerTexture, paintSampler, uv, 0.0);
  let coverage = textureSampleLevel(strokeMask, paintSampler, uv, 0.0).r;
  let source = stroke.color * coverage;
  return source + layer * (1.0 - source.a);
}

@fragment
fn fs_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = previewUv(pos);
  let layer = textureSampleLevel(layerTexture, paintSampler, uv, 0.0);
  let coverage = textureSampleLevel(strokeMask, paintSampler, uv, 0.0).r;
  return layer * (1.0 - coverage);
}
