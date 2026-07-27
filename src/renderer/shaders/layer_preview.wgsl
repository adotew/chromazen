@group(0) @binding(0) var previewSampler: sampler;
@group(0) @binding(1) var layerTexture: texture_2d<f32>;
@group(0) @binding(2) var strokeMask: texture_2d<f32>;
@group(0) @binding(3) var<uniform> stroke: Stroke;
@group(0) @binding(4) var<uniform> preview: Preview;

struct Stroke {
  color: vec4f,
};

struct Preview {
  previewDims: vec2f,
  documentDims: vec2f,
};

@vertex
fn vs_preview(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4f {
  let x = f32(i32(vertexIndex) / 2) * 4.0 - 1.0;
  let y = f32(i32(vertexIndex) & 1) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn previewUv(pos: vec4f) -> vec2f {
  let scale = min(
    preview.previewDims.x / preview.documentDims.x,
    preview.previewDims.y / preview.documentDims.y,
  );
  let contentDims = preview.documentDims * scale;
  let origin = (preview.previewDims - contentDims) * 0.5;
  return (pos.xy - origin) / contentDims;
}

fn outsideDocument(uv: vec2f) -> bool {
  return any(uv < vec2f(0.0)) || any(uv > vec2f(1.0));
}

@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = previewUv(pos);
  if outsideDocument(uv) {
    return vec4f(0.0);
  }
  return textureSampleLevel(layerTexture, previewSampler, uv, 0.0);
}

@fragment
fn fs_brush(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = previewUv(pos);
  if outsideDocument(uv) {
    return vec4f(0.0);
  }
  let base = textureSampleLevel(layerTexture, previewSampler, uv, 0.0);
  let mask = textureSampleLevel(strokeMask, previewSampler, uv, 0.0).r;
  let source = stroke.color * mask;
  return source + base * (1.0 - source.a);
}

@fragment
fn fs_eraser(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = previewUv(pos);
  if outsideDocument(uv) {
    return vec4f(0.0);
  }
  let base = textureSampleLevel(layerTexture, previewSampler, uv, 0.0);
  let mask = textureSampleLevel(strokeMask, previewSampler, uv, 0.0).r;
  return base * (1.0 - mask);
}
