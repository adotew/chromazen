@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var paintTex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> view: View;

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

fn paintUv(pos: vec4f) -> vec2f {
  return (pos.xy * view.scale + view.offset) / view.paintDims;
}

fn outsideCanvas(uv: vec2f) -> bool {
  return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

@fragment
fn fs_background(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  if (outsideCanvas(paintUv(pos))) {
    return vec4f(0.0);
  }
  return view.backgroundColor;
}

@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = paintUv(pos);
  if (outsideCanvas(uv)) {
    return vec4f(0.0);
  }
  return textureSampleLevel(paintTex, paintSampler, uv, 0.0);
}
