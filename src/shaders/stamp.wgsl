@group(0) @binding(0) var brushSampler: sampler;
@group(0) @binding(1) var brushStamp: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> brushes: array<Brush>;
@group(0) @binding(3) var<uniform> paint: Paint;

struct Paint {
  dims: vec2f,
  padding: vec2f,
};

struct Brush {
  center: vec2f,
  halfSize: vec2f,
  color: vec4f,
  bounds: vec4f,
};

struct VertexOut {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
  @location(1) color: vec4f,
};

fn quadCorner(vertexIndex: u32) -> vec2f {
  let corners = array<vec2f, 6>(
    vec2f(-1.0, -1.0),
    vec2f(1.0, -1.0),
    vec2f(-1.0, 1.0),
    vec2f(-1.0, 1.0),
    vec2f(1.0, -1.0),
    vec2f(1.0, 1.0),
  );

  return corners[vertexIndex];
}

@vertex
fn vs(
  @builtin(vertex_index) vertexIndex: u32,
  @builtin(instance_index) instanceIndex: u32,
) -> VertexOut {
  let brush = brushes[instanceIndex];
  let corner = quadCorner(vertexIndex);
  let paintPos = brush.center + corner * brush.halfSize;

  var out: VertexOut;
  out.position = vec4f(
    paintPos.x / paint.dims.x * 2.0 - 1.0,
    1.0 - paintPos.y / paint.dims.y * 2.0,
    0.0,
    1.0,
  );
  out.uv = corner * 0.5 + vec2f(0.5, 0.5);
  out.color = brush.color;
  return out;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4f {
  let mask = textureSample(brushStamp, brushSampler, in.uv).a;
  let alpha = clamp(in.color.a * mask, 0.0, 1.0);
  return vec4f(in.color.rgb, alpha);
}
