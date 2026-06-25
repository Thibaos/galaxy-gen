// ── Bindings ─────────────────────────────────────────────────
struct CameraUniform {
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<storage, read> stars: array<u32>;
@group(0) @binding(2) var<uniform> star_params: StarParams;

struct StarParams {
    brightness: f32,
    aspect: f32,
    star_size: f32,
}

// Each star uses 7 u32 words (packed as f32 bit patterns)
const STAR_STRIDE: u32 = 7u;

// ── Vertex ───────────────────────────────────────────────────

struct VertexInput {
    @builtin(vertex_index) corner: u32,
    @builtin(instance_index) instance: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

// Unit quad corners (triangle strip order to avoid index buffer)
const CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0,  1.0),
);

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let base = 2u + in.instance * STAR_STRIDE;

    // Read star data from storage buffer (first 2 u32 = [count, capacity])
    let px = bitcast<f32>(stars[base + 0u]);
    let py = bitcast<f32>(stars[base + 1u]);
    let pz = bitcast<f32>(stars[base + 2u]);
    let mass = bitcast<f32>(stars[base + 3u]);
    let temp = bitcast<f32>(stars[base + 4u]);
    let lum  = bitcast<f32>(stars[base + 5u]);

    // Star color from temperature (piecewise LUT)
    let col = temperature_to_rgb(temp);

    // Alpha from luminosity (log-compressed), scaled by brightness uniform
    let alpha = clamp(lum * 0.5 / (lum * 0.5 + 1.0), 0.0, 1.0) * star_params.brightness;

    let world_pos = vec3<f32>(px, py, pz);
    let clip_center = camera.view_proj * vec4<f32>(world_pos, 1.0);

    // Point size in clip-space: scale with log-mass, correct X for aspect
    let size_clip = star_params.star_size * (0.008 + 0.025 * log2(max(mass, 0.1) + 0.5));
    var offset = CORNERS[in.corner] * size_clip;
    offset.x /= star_params.aspect;

    var out: VertexOutput;
    out.position = vec4<f32>(
        clip_center.xy + offset * clip_center.w,
        clip_center.z,
        clip_center.w,
    );
    out.color = vec4<f32>(col.r, col.g, col.b, alpha);
    out.uv = CORNERS[in.corner];
    return out;
}

// ── Temperature→RGB LUT ─────────────────────────────────────

const LUT_LEN: u32 = 16u;

const LUT_DATA: array<vec4<f32>, 16> = array<vec4<f32>, 16>(
    vec4<f32>(2300.0, 1.000, 0.745, 0.424),
    vec4<f32>(2600.0, 1.000, 0.765, 0.427),
    vec4<f32>(3060.0, 1.000, 0.800, 0.435),
    vec4<f32>(3400.0, 1.000, 0.808, 0.506),
    vec4<f32>(3750.0, 1.000, 0.765, 0.545),
    vec4<f32>(4400.0, 1.000, 0.847, 0.710),
    vec4<f32>(5240.0, 1.000, 0.933, 0.867),
    vec4<f32>(5770.0, 1.000, 0.961, 0.949),
    vec4<f32>(6540.0, 0.973, 0.969, 1.000),
    vec4<f32>(7220.0, 0.878, 0.898, 1.000),
    vec4<f32>(8180.0, 0.792, 0.843, 1.000),
    vec4<f32>(9700.0, 0.725, 0.788, 1.000),
    vec4<f32>(15200.0, 0.667, 0.749, 1.000),
    vec4<f32>(26500.0, 0.612, 0.698, 1.000),
    vec4<f32>(41400.0, 0.608, 0.690, 1.000),
    vec4<f32>(50000.0, 0.608, 0.690, 1.000),
);

fn temperature_to_rgb(t_kelvin: f32) -> vec3<f32> {
    var t = clamp(t_kelvin, LUT_DATA[0].x, LUT_DATA[LUT_LEN - 1u].x);
    if t <= LUT_DATA[0].x {
        return vec3<f32>(LUT_DATA[0].y, LUT_DATA[0].z, LUT_DATA[0].w);
    }
    for (var i = 0u; i < LUT_LEN - 1u; i++) {
        if t <= LUT_DATA[i + 1u].x {
            let t_lo = LUT_DATA[i].x;
            let t_hi = LUT_DATA[i + 1u].x;
            let frac = (t - t_lo) / (t_hi - t_lo);
            let r = LUT_DATA[i].y + frac * (LUT_DATA[i + 1u].y - LUT_DATA[i].y);
            let g = LUT_DATA[i].z + frac * (LUT_DATA[i + 1u].z - LUT_DATA[i].z);
            let b = LUT_DATA[i].w + frac * (LUT_DATA[i + 1u].w - LUT_DATA[i].w);
            return vec3<f32>(r, g, b);
        }
    }
    let last = LUT_DATA[LUT_LEN - 1u];
    return vec3<f32>(last.y, last.z, last.w);
}

// ── Fragment ─────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft circular profile: falloff sharper near edge for less blur
    let dist = length(in.uv);
    let falloff = 1.0 - smoothstep(0.6, 1.0, dist);
    return vec4<f32>(in.color.rgb, in.color.a * falloff);
}
