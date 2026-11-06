#import types::{PosTexVertex, EffectUniformParams}

// The touch effect's tap ripple (`shin::TouchEffect`, draw `0x7100116070`): a ring refracting the finished frame
// around the tapped point. The quad covers the ripple; its texture position is the screen's uv in the frame.
// params[0] = (centre x, centre y, radius in canvas pixels, progress t in 0..1).

@group(0) @binding(0)
var<uniform> params: EffectUniformParams;

@group(0) @binding(1)
var scene_texture: texture_2d<f32>;
@group(0) @binding(2)
var scene_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.texture_position = input.texture_position;

    return output;
}

const CANVAS: vec2<f32> = vec2<f32>(1920.0, 1080.0);

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let centre = params.params[0].xy;
    let radius = params.params[0].z;
    let t = params.params[0].w;

    let d = length(input.texture_position * CANVAS - centre) / radius;
    let s = sin(6.28318548 * (d - t));
    // the same shift in pixels both ways
    let uv = input.texture_position + s * vec2<f32>(0.004, 0.004 * CANVAS.x / CANVAS.y);
    let edge = min(d, 1.0);

    return (textureSample(scene_texture, scene_sampler, uv) + 0.2 * s) * (1.0 - edge * edge) * (1.0 - t * t);
}
