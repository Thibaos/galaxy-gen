@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let pos = array(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    let uv = array(
        vec2(0.0, 1.0),
        vec2(2.0, 1.0),
        vec2(0.0, -1.0),
    );

    var out: VertexOutput;
    out.position = vec4(pos[vertex_index], 0.0, 1.0);
    out.uv = uv[vertex_index];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(tex, smp, in.uv);
    // Compensate for Bgra8Unorm surface format — swap R↔B.
    // The texture holds native RGBA; the surface stores BGRA bytes
    // so we map our R→surface B, our B→surface R to come out correct.
    return vec4(color.b, color.g, color.r, color.a);
}
