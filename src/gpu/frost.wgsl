@group(0) @binding(0) var inputSampler: sampler;
@group(0) @binding(1) var inputTexture: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let x = f32(index % 2u) * 4.0 - 1.0;
    let y = f32(index / 2u) * 4.0 - 1.0;
    return vec4f(x, y, 0.0, 1.0);
}

@fragment
fn fs_downsample(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let inputDimensions = textureDimensions(inputTexture);
    let inputSize = vec2f(inputDimensions);
    let outputSize = vec2f(
        f32((inputDimensions.x + 3u) / 4u),
        f32((inputDimensions.y + 3u) / 4u),
    );
    let uv = position.xy / outputSize;
    let offset = vec2f(1.0) / inputSize;
    return 0.25 * (
        textureSample(inputTexture, inputSampler, uv + vec2f(-offset.x, -offset.y)) +
        textureSample(inputTexture, inputSampler, uv + vec2f( offset.x, -offset.y)) +
        textureSample(inputTexture, inputSampler, uv + vec2f(-offset.x,  offset.y)) +
        textureSample(inputTexture, inputSampler, uv + vec2f( offset.x,  offset.y))
    );
}

fn gaussian(uv: vec2f, step: vec2f) -> vec4f {
    // A three-sigma, radius-18 Gaussian at quarter resolution gives a smooth
    // 72-physical-pixel support. Adjacent taps are paired with bilinear filtering, preserving
    // every part of the kernel instead of widening the gaps between taps (which creates bands at
    // hard edges).
    var color = textureSample(inputTexture, inputSampler, uv) * 0.066625;
    color += textureSample(inputTexture, inputSampler, uv + step * 1.489585) * 0.128731;
    color += textureSample(inputTexture, inputSampler, uv - step * 1.489585) * 0.128731;
    color += textureSample(inputTexture, inputSampler, uv + step * 3.475714) * 0.112146;
    color += textureSample(inputTexture, inputSampler, uv - step * 3.475714) * 0.112146;
    color += textureSample(inputTexture, inputSampler, uv + step * 5.461880) * 0.087491;
    color += textureSample(inputTexture, inputSampler, uv - step * 5.461880) * 0.087491;
    color += textureSample(inputTexture, inputSampler, uv + step * 7.448104) * 0.061125;
    color += textureSample(inputTexture, inputSampler, uv - step * 7.448104) * 0.061125;
    color += textureSample(inputTexture, inputSampler, uv + step * 9.434408) * 0.038243;
    color += textureSample(inputTexture, inputSampler, uv - step * 9.434408) * 0.038243;
    color += textureSample(inputTexture, inputSampler, uv + step * 11.420811) * 0.021427;
    color += textureSample(inputTexture, inputSampler, uv - step * 11.420811) * 0.021427;
    color += textureSample(inputTexture, inputSampler, uv + step * 13.407333) * 0.010751;
    color += textureSample(inputTexture, inputSampler, uv - step * 13.407333) * 0.010751;
    color += textureSample(inputTexture, inputSampler, uv + step * 15.393994) * 0.004830;
    color += textureSample(inputTexture, inputSampler, uv - step * 15.393994) * 0.004830;
    color += textureSample(inputTexture, inputSampler, uv + step * 17.380810) * 0.001944;
    color += textureSample(inputTexture, inputSampler, uv - step * 17.380810) * 0.001944;
    return color;
}

@fragment
fn fs_horizontal(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(inputTexture));
    return gaussian(position.xy / size, vec2f(1.0 / size.x, 0.0));
}

@fragment
fn fs_vertical(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(inputTexture));
    return gaussian(position.xy / size, vec2f(0.0, 1.0 / size.y));
}
