// Must match the `BlendMode` discriminants in `blend.rs`.
const BLEND_MULTIPLY: u32 = 1u;
const BLEND_OVERLAY: u32 = 2u;

fn blend_channels(mode: u32, backdrop: vec3f, source: vec3f) -> vec3f {
  switch mode {
    case BLEND_MULTIPLY: {
      return backdrop * source;
    }
    case BLEND_OVERLAY: {
      let screened = 1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source);
      return select(screened, 2.0 * backdrop * source, backdrop <= vec3f(0.5));
    }
    default: {
      return source;
    }
  }
}

fn blend_premultiplied(mode: u32, source: vec4f, backdrop: vec4f, clipped: bool) -> vec4f {
  var blended = vec3f(0.0);
  if (source.a > 0.0 && backdrop.a > 0.0) {
    let unpremultiplied = blend_channels(
      mode,
      min(backdrop.rgb / backdrop.a, vec3f(1.0)),
      min(source.rgb / source.a, vec3f(1.0)),
    );
    blended = source.a * backdrop.a * unpremultiplied;
  }
  let covered = backdrop.rgb * (1.0 - source.a) + blended;
  if (clipped) {
    return vec4f(covered, backdrop.a);
  }
  return vec4f(
    covered + source.rgb * (1.0 - backdrop.a),
    source.a + backdrop.a * (1.0 - source.a),
  );
}
