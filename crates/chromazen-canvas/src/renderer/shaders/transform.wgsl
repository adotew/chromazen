@group(0) @binding(0) var sourceTex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> transform: Transform;
@group(1) @binding(1) var<uniform> tile: Tile;

// Maps destination document pixels to source texture pixels. The source texture holds only the
// layer's content bounds, and the translation accounts for its document origin.
struct Transform {
  sourceFromDestinationX: vec4f,
  sourceFromDestinationY: vec4f,
};

struct Tile {
  origin: vec2f,
  padding: vec2f,
};

@vertex
fn vs(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4f {
  let x = f32(idx % 2u) * 4.0 - 1.0;
  let y = f32(idx / 2u) * 4.0 - 1.0;
  return vec4f(x, y, 0.0, 1.0);
}

fn load_or_transparent(point: vec2i) -> vec4f {
  let dims = vec2i(textureDimensions(sourceTex));
  if (any(point < vec2i(0)) || any(point >= dims)) {
    return vec4f(0.0);
  }
  return textureLoad(sourceTex, point, 0);
}

@fragment
fn fs(@builtin(position) position: vec4f) -> @location(0) vec4f {
  let destination = vec3f(position.xy + tile.origin, 1.0);
  let source = vec2f(
    dot(transform.sourceFromDestinationX.xyz, destination),
    dot(transform.sourceFromDestinationY.xyz, destination),
  );

  // Pixel centers are at n + 0.5. Manual bilinear filtering keeps pixels beyond
  // the source content transparent instead of extending edge colors.
  let samplePosition = source - vec2f(0.5);
  let base = vec2i(floor(samplePosition));
  let fraction = fract(samplePosition);
  let top = mix(
    load_or_transparent(base),
    load_or_transparent(base + vec2i(1, 0)),
    fraction.x,
  );
  let bottom = mix(
    load_or_transparent(base + vec2i(0, 1)),
    load_or_transparent(base + vec2i(1, 1)),
    fraction.x,
  );
  return mix(top, bottom, fraction.y);
}
