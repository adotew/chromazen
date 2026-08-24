@group(0) @binding(0) var paintSampler: sampler;
@group(0) @binding(1) var paintTex: texture_2d<f32>;
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
fn vs(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn paint_uv(pos: vec4f) -> vec2f {
  let window = vec3f(pos.xy, 1.0);
  let document = vec2f(
    dot(view.documentFromWindowX.xyz, window),
    dot(view.documentFromWindowY.xyz, window),
  );
  return document / view.paintDims;
}

fn is_outside_canvas(uv: vec2f) -> bool {
  return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

@fragment
fn fs_background(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  if (is_outside_canvas(paint_uv(pos))) {
    return vec4f(0.0);
  }
  return view.backgroundColor;
}

@fragment
fn fs_layer(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let uv = paint_uv(pos);
  if (is_outside_canvas(uv)) {
    return vec4f(0.0);
  }
  // Paint textures are premultiplied, so opacity scales every channel.
  return textureSampleLevel(paintTex, paintSampler, uv, 0.0) * layer.opacity;
}
