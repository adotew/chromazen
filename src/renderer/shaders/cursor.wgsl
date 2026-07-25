@group(0) @binding(0) var brushSampler: sampler;
@group(0) @binding(1) var brushStamp: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> cursor: Cursor;

struct Cursor {
  center: vec2f,
  halfSize: vec2f,
  surfaceSize: vec2f,
  padding: vec2f,
};

struct VertexOut {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
  @location(1) uvPerPixel: vec2f,
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
fn vs(@builtin(vertex_index) vertexIndex: u32) -> VertexOut {
  let corner = quadCorner(vertexIndex);
  let halfSize = max(cursor.halfSize, vec2f(0.5));
  let pixelOffset = corner * (halfSize + vec2f(2.0));
  let screenPosition = cursor.center + pixelOffset;

  var out: VertexOut;
  out.position = vec4f(
    screenPosition.x / cursor.surfaceSize.x * 2.0 - 1.0,
    1.0 - screenPosition.y / cursor.surfaceSize.y * 2.0,
    0.0,
    1.0,
  );
  out.uv = pixelOffset / (halfSize * 2.0) + vec2f(0.5);
  out.uvPerPixel = vec2f(0.5) / halfSize;
  return out;
}

fn mask(uv: vec2f) -> f32 {
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
    return 0.0;
  }
  return select(0.0, 1.0, textureSampleLevel(brushStamp, brushSampler, uv, 0.0).a >= 0.08);
}

fn neighborhood(uv: vec2f, delta: vec2f) -> vec2f {
  let a = mask(uv + vec2f(-delta.x, -delta.y));
  let b = mask(uv + vec2f(0.0, -delta.y));
  let c = mask(uv + vec2f(delta.x, -delta.y));
  let d = mask(uv + vec2f(-delta.x, 0.0));
  let e = mask(uv + vec2f(delta.x, 0.0));
  let f = mask(uv + vec2f(-delta.x, delta.y));
  let g = mask(uv + vec2f(0.0, delta.y));
  let h = mask(uv + vec2f(delta.x, delta.y));
  return vec2f(
    min(min(min(a, b), min(c, d)), min(min(e, f), min(g, h))),
    max(max(max(a, b), max(c, d)), max(max(e, f), max(g, h))),
  );
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4f {
  let center = mask(in.uv);
  let near = neighborhood(in.uv, in.uvPerPixel);
  let far = neighborhood(in.uv, in.uvPerPixel * 2.0);
  let inner = center * (1.0 - near.x);
  let outer = (1.0 - center) * far.y;

  if (inner > 0.0) {
    return vec4f(0.92, 0.92, 0.92, 0.92);
  }
  if (outer > 0.0) {
    return vec4f(0.04, 0.04, 0.04, 0.8);
  }
  discard;
}
