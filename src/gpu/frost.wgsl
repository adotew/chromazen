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
    // A three-sigma, radius-24 Gaussian at quarter resolution gives a smooth
    // 96-physical-pixel support. Adjacent taps are paired with bilinear filtering, preserving
    // every part of the kernel instead of widening the gaps between taps (which creates bands at
    // hard edges).
    var color = textureSample(inputTexture, inputSampler, uv) * 0.049977;
    color += textureSample(inputTexture, inputSampler, uv + step * 1.494141) * 0.098027;
    color += textureSample(inputTexture, inputSampler, uv - step * 1.494141) * 0.098027;
    color += textureSample(inputTexture, inputSampler, uv + step * 3.486332) * 0.090688;
    color += textureSample(inputTexture, inputSampler, uv - step * 3.486332) * 0.090688;
    color += textureSample(inputTexture, inputSampler, uv + step * 5.478529) * 0.078834;
    color += textureSample(inputTexture, inputSampler, uv - step * 5.478529) * 0.078834;
    color += textureSample(inputTexture, inputSampler, uv + step * 7.470737) * 0.064394;
    color += textureSample(inputTexture, inputSampler, uv - step * 7.470737) * 0.064394;
    color += textureSample(inputTexture, inputSampler, uv + step * 9.462959) * 0.049423;
    color += textureSample(inputTexture, inputSampler, uv - step * 9.462959) * 0.049423;
    color += textureSample(inputTexture, inputSampler, uv + step * 11.455199) * 0.035644;
    color += textureSample(inputTexture, inputSampler, uv - step * 11.455199) * 0.035644;
    color += textureSample(inputTexture, inputSampler, uv + step * 13.447460) * 0.024155;
    color += textureSample(inputTexture, inputSampler, uv - step * 13.447460) * 0.024155;
    color += textureSample(inputTexture, inputSampler, uv + step * 15.439747) * 0.015381;
    color += textureSample(inputTexture, inputSampler, uv - step * 15.439747) * 0.015381;
    color += textureSample(inputTexture, inputSampler, uv + step * 17.432063) * 0.009203;
    color += textureSample(inputTexture, inputSampler, uv - step * 17.432063) * 0.009203;
    color += textureSample(inputTexture, inputSampler, uv + step * 19.424412) * 0.005174;
    color += textureSample(inputTexture, inputSampler, uv - step * 19.424412) * 0.005174;
    color += textureSample(inputTexture, inputSampler, uv + step * 21.416797) * 0.002733;
    color += textureSample(inputTexture, inputSampler, uv - step * 21.416797) * 0.002733;
    color += textureSample(inputTexture, inputSampler, uv + step * 23.409221) * 0.001357;
    color += textureSample(inputTexture, inputSampler, uv - step * 23.409221) * 0.001357;
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
