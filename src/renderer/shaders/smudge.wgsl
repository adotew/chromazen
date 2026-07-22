@group(0) @binding(0) var brushSampler: sampler;
@group(0) @binding(1) var brushStamp: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> brushes: array<Brush>;
@group(0) @binding(3) var<uniform> paint: Paint;
@group(0) @binding(4) var sourceTexture: texture_2d<f32>;

struct Paint {
  dims: vec2f,
  padding: vec2f,
};

struct Brush {
  center: vec2f,
  halfSize: vec2f,
  color: vec4f,
  bounds: vec4f,
  sourceCenter: vec2f,
  padding: vec2f,
};

struct VertexOut {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
  @location(1) targetPos: vec2f,
  @location(2) sourcePos: vec2f,
  @location(3) strength: f32,
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
  let offset = corner * brush.halfSize;
  let paintPos = brush.center + offset;

  var out: VertexOut;
  out.position = vec4f(
    paintPos.x / paint.dims.x * 2.0 - 1.0,
    1.0 - paintPos.y / paint.dims.y * 2.0,
    0.0,
    1.0,
  );
  out.uv = corner * 0.5 + vec2f(0.5, 0.5);
  out.targetPos = paintPos;
  out.sourcePos = brush.sourceCenter + offset;
  out.strength = brush.color.a;
  return out;
}

fn sampleSource(pos: vec2f) -> vec4f {
  let clampedPos = clamp(pos, vec2f(0.0), paint.dims - vec2f(1.0));
  let uv = (clampedPos + vec2f(0.5)) / paint.dims;
  return textureSampleLevel(sourceTexture, brushSampler, uv, 0.0);
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4f {
  let mask = textureSample(brushStamp, brushSampler, in.uv).a;
  let strength = clamp(in.strength * mask, 0.0, 1.0);
  let targetColor = sampleSource(in.targetPos);
  let draggedColor = sampleSource(in.sourcePos);
  return mix(targetColor, draggedColor, strength);
}
