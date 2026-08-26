@group(0) @binding(0) var linearSampler: sampler;
@group(0) @binding(1) var sourceTexture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> blur: BlurUniform;

struct BlurUniform {
  direction: vec2f,
  scale: f32,
  padding: f32,
};

struct FullscreenVertex {
  @builtin(position) position: vec4f,
  @location(0) uv: vec2f,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenVertex {
  let position = vec2f(f32(index % 2u) * 4.0 - 1.0, f32(index / 2u) * 4.0 - 1.0);
  var out: FullscreenVertex;
  out.position = vec4f(position, 0.0, 1.0);
  out.uv = vec2f(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
  return out;
}

const KERNEL_RADIUS: i32 = 16;

@fragment
fn fs_blur(in: FullscreenVertex) -> @location(0) vec4f {
  let dimensions = vec2f(textureDimensions(sourceTexture));
  let step = blur.direction / dimensions;
  let sigma = max(blur.scale, 0.5);
  let denominator = 2.0 * sigma * sigma;
  var color = vec4f(0.0);
  var weightSum = 0.0;
  for (var offset = -KERNEL_RADIUS; offset <= KERNEL_RADIUS; offset = offset + 1) {
    let distance = f32(offset);
    let weight = exp(-(distance * distance) / denominator);
    color += textureSampleLevel(
      sourceTexture,
      linearSampler,
      in.uv + step * distance,
      0.0,
    ) * weight;
    weightSum += weight;
  }
  return color / weightSum;
}
