@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var layerTexture: texture_2d<f32>;
@group(0) @binding(2) var strokeMask: texture_2d<f32>;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> stroke: Stroke;

struct View {
  scale: vec2f,
  offset: vec2f,
  paintDims: vec2f,
  padding: vec2f,
  backgroundColor: vec4f,
};

struct Stroke {
  color: vec4f,
};

@vertex
fn vs_preview(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn paintUv(pos: vec4f) -> vec2f {
  return (pos.xy * view.scale + view.offset) / view.paintDims;
}

fn outsideCanvas(uv: vec2f) -> bool {
  return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

@fragment
fn fs_preview_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = paintUv(pos);
  if (outsideCanvas(uv)) {
    return vec4f(0.0);
  }

  let layer = textureSampleLevel(layerTexture, paintSampler, uv, 0.0);
  let coverage = textureSampleLevel(strokeMask, paintSampler, uv, 0.0).r;
  let source = stroke.color * coverage;
  return source + layer * (1.0 - source.a);
}

@fragment
fn fs_preview_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = paintUv(pos);
  if (outsideCanvas(uv)) {
    return vec4f(0.0);
  }

  let layer = textureSampleLevel(layerTexture, paintSampler, uv, 0.0);
  let coverage = textureSampleLevel(strokeMask, paintSampler, uv, 0.0).r;
  return layer * (1.0 - coverage);
}

@vertex
fn vs_commit(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn committedCoverage(pos: vec4f) -> f32 {
  return textureLoad(strokeMask, vec2i(pos.xy), 0).r;
}

@fragment
fn fs_commit_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  return stroke.color * committedCoverage(pos);
}
