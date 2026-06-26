#![cfg_attr(target_arch = "spirv", no_std)]
#![allow(clippy::manual_saturating_arithmetic)]

use spirv_std::glam::{UVec3, Vec3};
// Real trait provides f32 methods (exp, ln, powf, …) in no_std SPIR-V context.
// The compiler sees no explicit `Real::` calls but methods are used implicitly
// via UFCS resolution.
#[allow(unused)]
use spirv_std::num_traits::real::Real;
use spirv_std::spirv;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct GalaxyUniform {
    pub disk_scale_length: f32,
    pub disk_scale_height: f32,
    pub disk_central_density: f32,
    pub arm_count: u32,
    pub arm_pitch: f32,
    pub arm_concentration: f32,
    pub arm_strength: f32,
    pub bulge_radius: f32,
    pub bulge_central_density: f32,
    pub halo_radius: f32,
    pub halo_central_density: f32,
    pub halo_slope: f32,
    pub image_width: u32,
    pub image_height: u32,
    pub extent: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub exposure: f32,
    pub log_contrast: f32,
}

const PI: f32 = core::f32::consts::PI;
const TAU: f32 = 2.0 * PI;

fn rem_euclid(x: f32, y: f32) -> f32 {
    let r = x % y;
    if r < 0.0 { r + y } else { r }
}

// ── Column density (stars / ly²) ───────────────────────────
// Integrated over the vertical (y) axis for top-down rendering.

fn column_density(x: f32, z: f32, p: &GalaxyUniform) -> f32 {
    let r = (x * x + z * z).sqrt();

    let disk_col = disk_column(r, p) * arm_modulation_2d(x, z, r, p);
    let bulge_col = bulge_column(r, p);
    let halo_col = halo_column(r, p);

    disk_col + bulge_col + halo_col
}

/// Disk column density:  ∫ sech²(y/H) dy = 2H.
fn disk_column(r: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    2.0 * p.disk_scale_height * p.disk_central_density * radial
}

/// Logarithmic spiral arm modulation.
///
/// Each arm follows θ = cot(φ)·ln(1 + r/h_r)  where φ = `arm_pitch`
/// is the true pitch angle (angle between spiral tangent and the
/// tangent circle).  The `1 +` avoids a singularity at the origin
/// where the bulge dominates anyway.
fn arm_modulation_2d(x: f32, z: f32, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 || p.disk_scale_length <= 0.0 {
        return 1.0;
    }
    let theta = x.atan2(z);
    // Logarithmic spiral: θ = cot(φ) × ln(r/r₀)
    // Guard r > 0 to avoid ln(0); use ln(1 + r/hr) for smooth centre
    let cot_phi = 1.0 / p.arm_pitch.tan();
    let log_spiral = theta - cot_phi * (1.0 + r / p.disk_scale_length).ln();

    let arm_width = 1.0 / p.arm_concentration;
    let mut min_dtheta = PI;

    for k in 0..p.arm_count {
        let phase = log_spiral + TAU * (k as f32) / (p.arm_count as f32);
        let dtheta = rem_euclid(phase, TAU);
        let dtheta = if dtheta > PI { dtheta - TAU } else { dtheta };
        min_dtheta = min_dtheta.min(dtheta.abs());
    }

    let arg = min_dtheta / arm_width;
    1.0 + p.arm_strength * (-0.5 * arg * arg).exp()
}



/// Bulge column (Plummer sphere): ∫₋∞⁺∞ (1 + R²/a² + y²/a²)^(-2.5) dy
/// Closed form: (4/3) * a * ρ₀ * (1 + R²/a²)^(-2)
fn bulge_column(r: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = r / p.bulge_radius;
    (4.0 / 3.0) * p.bulge_radius * p.bulge_central_density * (1.0 + x * x).powf(-2.0)
}

/// Halo column (power-law sphere). Rough analytic approximation:
/// ∫₋∞⁺∞ (1 + √(R²+y²)/R_h)^s dy  ≈  π · R_h · (1 + R/R_h)^(s+1)
fn halo_column(r: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 || r < 1e-6 {
        return p.halo_radius * p.halo_central_density * PI;
    }
    let x = r / p.halo_radius;
    PI * p.halo_radius * p.halo_central_density * (1.0 + x).powf(p.halo_slope + 1.0)
}



// ═══════════════════════════════════════════════════════════
//  Unified scene render (single pass)
// ═══════════════════════════════════════════════════════════

#[spirv(compute(threads(8, 8, 1)))]
pub fn render_scene(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] rgba: &mut [u32],
) {
    let px = id.x;
    let py = id.y;
    if px >= params.image_width || py >= params.image_height {
        return;
    }

    let buf_stride = params.image_width.div_ceil(64u32) * 64u32;
    let idx = (py * buf_stride + px) as usize;

    // ── compute column density glow ──
    let extent_x = params.extent;
    let extent_y = params.extent * (params.image_height as f32 / params.image_width as f32);
    let wx = (px as f32 / params.image_width as f32 - 0.5) * extent_x + params.center_x;
    let wz = -(py as f32 / params.image_height as f32 - 0.5) * extent_y + params.center_y;

    let col_dens = column_density(wx, wz, params);
    let gray = col_dens * 0.5;
    let rgb = Vec3::new(gray, gray, gray);

    // ── tone-map (luminance-based, preserves chromaticity) ───
    // Applying ln() per-channel destroys colour ratios.
    // Instead, compress *luminance* with the log-stretch, then
    // multiply back the original chromaticity.
    let epsilon: f32 = 1e-12;
    let lum = 0.299 * rgb.x + 0.587 * rgb.y + 0.114 * rgb.z;
    let log_lum = (lum + epsilon).ln();
    let t = (log_lum * params.log_contrast + params.exposure).clamp(0.0, 1.0);

    let inv_lum = 1.0 / lum.max(epsilon);
    let r = (rgb.x * inv_lum * t).clamp(0.0, 1.0);
    let g = (rgb.y * inv_lum * t).clamp(0.0, 1.0);
    let b = (rgb.z * inv_lum * t).clamp(0.0, 1.0);

    let r8 = (r * 255.0) as u32;
    let g8 = (g * 255.0) as u32;
    let b8 = (b * 255.0) as u32;
    // Pack into native RGBA byte order: R at byte 0, G at byte 1,
    // B at byte 2, A at byte 3.  This matches Rgba8Unorm texture format
    // expected by copy_buffer_to_texture and the PNG export.
    rgba[idx] = r8 | g8 << 8 | b8 << 16 | 0xff_00_00_00;
}
