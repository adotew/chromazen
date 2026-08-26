@group(0) @binding(0) var linearSampler: sampler;
@group(0) @binding(1) var inputTexture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> surface: Surface;

struct Surface {
  size: vec2f,
  padding: vec2f,
};

struct FullscreenVertex {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
};

struct GlassVertex {
  @builtin(position) position: vec4f,
  @location(0) pixelPosition: vec2f,
  @location(1) @interpolate(flat) rect: vec4f,
  @location(2) @interpolate(flat) radius: f32,
  @location(3) @interpolate(flat) tint: vec4f,
};

fn fullscreen_position(index: u32) -> vec2f {
  return vec2f(f32(index % 2u) * 4.0 - 1.0, f32(index / 2u) * 4.0 - 1.0);
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenVertex {
  let position = fullscreen_position(index);
  var out: FullscreenVertex;
  out.position = vec4f(position, 0.0, 1.0);
  out.uv = vec2f(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
  return out;
}

@fragment
fn fs_scene(in: FullscreenVertex) -> @location(0) vec4f {
  return textureSampleLevel(inputTexture, linearSampler, in.uv, 0.0);
}

fn quad_corner(index: u32) -> vec2f {
  let corners = array<vec2f, 6>(
    vec2f(0.0, 0.0),
    vec2f(1.0, 0.0),
    vec2f(0.0, 1.0),
    vec2f(0.0, 1.0),
    vec2f(1.0, 0.0),
    vec2f(1.0, 1.0),
  );
  return corners[index];
}

@vertex
fn vs_glass(
  @builtin(vertex_index) vertexIndex: u32,
  @location(0) rect: vec4f,
  @location(1) radius: f32,
  @location(2) tint: vec4f,
) -> GlassVertex {
  let corner = quad_corner(vertexIndex);
  let pixelPosition = mix(rect.xy, rect.zw, corner);
  var out: GlassVertex;
  out.position = vec4f(
    pixelPosition.x / surface.size.x * 2.0 - 1.0,
    1.0 - pixelPosition.y / surface.size.y * 2.0,
    0.0,
    1.0,
  );
  out.pixelPosition = pixelPosition;
  out.rect = rect;
  out.radius = radius;
  out.tint = tint;
  return out;
}

fn rounded_rect_distance(position: vec2f, rect: vec4f, radius: f32) -> f32 {
  let halfSize = (rect.zw - rect.xy) * 0.5;
  let center = (rect.xy + rect.zw) * 0.5;
  let safeRadius = min(radius, min(halfSize.x, halfSize.y));
  let q = abs(position - center) - (halfSize - vec2f(safeRadius));
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - safeRadius;
}

@fragment
fn fs_glass(in: GlassVertex) -> @location(0) vec4f {
  let distance = rounded_rect_distance(in.pixelPosition, in.rect, in.radius);
  let coverage = 1.0 - smoothstep(-0.75, 0.75, distance);
  let uv = in.pixelPosition / surface.size;
  let blurred = textureSampleLevel(inputTexture, linearSampler, uv, 0.0).rgb;
  let color = mix(blurred, in.tint.rgb, in.tint.a);
  return vec4f(color * coverage, coverage);
}
