#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::glam::{UVec3, Vec3};
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

const PI: f32 = 3.141_592_7;
const TAU: f32 = 2.0 * PI;

fn rem_euclid(x: f32, y: f32) -> f32 {
    let r = x % y;
    if r < 0.0 {
        r + y
    } else {
        r
    }
}

fn density(pos: Vec3, p: &GalaxyUniform) -> f32 {
    let r = (pos.x * pos.x + pos.z * pos.z).sqrt();
    let z = pos.y.abs();

    disk_density(r, z, p) * arm_modulation(pos, r, p)
        + bulge_density(pos.length(), p)
        + halo_density(pos.length(), p)
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

/// Same spiral modulation but takes flat (x,z) instead of Vec3.
fn arm_modulation_2d(x: f32, z: f32, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = x.atan2(z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;

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

fn disk_density(r: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    let zeta = z / p.disk_scale_height;
    let sech = 1.0 / zeta.cosh();
    p.disk_central_density * radial * sech * sech
}

fn arm_modulation(pos: Vec3, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = pos.x.atan2(pos.z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;

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

fn bulge_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = dist / p.bulge_radius;
    p.bulge_central_density * (1.0 + x * x).powf(-2.5)
}

fn halo_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 || dist < 1e-6 {
        return p.halo_central_density;
    }
    let x = dist / p.halo_radius;
    p.halo_central_density * (1.0 + x).powf(p.halo_slope)
}

// ═══════════════════════════════════════════════════════════
//  Stars: per-pixel procedural star rendering
// ═══════════════════════════════════════════════════════════

/// PCG-style hash for deterministic per-pixel PRNG.
fn hash3(mut x: u32, y: u32, seed: u32) -> u32 {
    x = x.wrapping_mul(0xcc9e2d51).wrapping_add(y);
    x = x.rotate_left(15);
    x = x.wrapping_mul(0x1b873593);
    x ^= seed;
    x ^= x >> 16;
    x = x.wrapping_mul(0x85ebca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2ae35);
    x ^= x >> 16;
    x
}

/// Fast LCG returning a float in [0, 1).
fn randf(rng: &mut u32) -> f32 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    // Use 24 bits for good precision.
    (*rng >> 8) as f32 / 16777215.0
}

/// Poisson-draw the number of stars for this pixel.
/// Standard Knuth algorithm; fine when λ is small (≪ 30).
fn poisson(lambda: f32, rng: &mut u32) -> u32 {
    if lambda <= 0.0 {
        return 0;
    }
    let l = (-lambda).exp();
    let mut k: u32 = 0;
    let mut p: f32 = 1.0;
    loop {
        k += 1;
        p *= randf(rng);
        if p <= l {
            break;
        }
    }
    k - 1
}

/// Sample stellar mass from a Kroupa IMF (upper part:  m⁻²·³  for  0.5–100 M☉).
fn sample_imf(rng: &mut u32) -> f32 {
    let m_min: f32 = 0.5;
    let m_max: f32 = 100.0;
    let alpha: f32 = 2.3;
    let u = randf(rng);
    // Inverse CDF for power law  p(m) ∝ m^(-α):
    let e = 1.0 - alpha;
    let m_min_pow = m_min.powf(e);
    let m_max_pow = m_max.powf(e);
    (m_min_pow + u * (m_max_pow - m_min_pow)).powf(1.0 / e)
}

/// Mass–luminosity (main-sequence approximation in solar units).
fn mass_to_lum(m: f32) -> f32 {
    // L ∝ M³·⁵ for  0.5–2 M☉,  L ∝ M  for massive stars.
    // Use a smooth-ish broken power law.
    if m < 2.0 {
        m.powf(3.5)
    } else {
        //  L = L_break * (M / M_break)^1.0
        let l_break: f32 = 2.0f32.powf(3.5);
        l_break * (m / 2.0)
    }
}

/// Effective temperature from mass (main sequence, rough).
fn mass_to_temp(m: f32) -> f32 {
    // T ∝ M^0.5  with  T_sun ≈ 5778 K.
    5778.0 * m.sqrt()
}

/// Blackbody colour  →  linear RGB.
/// Based on Tanner Helland's approximation.
fn temperature_to_rgb(t_kelvin: f32) -> Vec3 {
    let t = (t_kelvin / 100.0).clamp(10.0, 400.0);

    let r = if t <= 66.0 {
        1.0
    } else {
        let v = 1.292_936_2 * (t - 60.0).powf(-0.133_204_76);
        v.clamp(0.0, 1.0)
    };

    let g = if t <= 66.0 {
        let v = 0.390_081_58 * t.ln() - 0.631_841_4;
        v.clamp(0.0, 1.0)
    } else {
        let v = 1.129_608_6 * (t - 60.0).powf(-0.075_514_846);
        v.clamp(0.0, 1.0)
    };

    let b = if t <= 66.0 {
        if t <= 19.0 {
            0.0
        } else {
            let v = 0.543_206_8 * (t - 10.0).ln() - 1.196_251_4;
            v.clamp(0.0, 1.0)
        }
    } else {
        1.0
    };

    Vec3::new(r, g, b)
}

// ═══════════════════════════════════════════════════════════
//  Unified scene render (single pass)
//  Replaces the old three-pass pipeline (density → normalize →
//  stars) with one compute shader that handles everything.
// ═══════════════════════════════════════════════════════════

/// Light-weighted mean luminosity per star from Kroupa IMF (L☉).
const MEAN_IMF_LUM: f32 = 6.64;

/// Light-weighted mean spectral colour (linear RGB).
/// Slightly blue-tinted — massive stars dominate the integrated light.
const MEAN_SPECTRAL_R: f32 = 0.55;
const MEAN_SPECTRAL_G: f32 = 0.60;
const MEAN_SPECTRAL_B: f32 = 0.72;

/// Below this λ, render individual stars with Poisson sampling.
const LAMBDA_INDIVIDUAL: f32 = 2.0;

/// Above this λ, switch to analytic expected light.
const LAMBDA_ANALYTIC: f32 = 20.0;

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

    let idx = (py * params.image_width + px) as usize;

    // ── world position ─────────────────────────────────
    let wx = (px as f32 / params.image_width as f32 - 0.5) * params.extent + params.center_x;
    let wz = -(py as f32 / params.image_height as f32 - 0.5) * params.extent + params.center_y;

    // ── expected star count for this column ────────────
    let col_dens = column_density(wx, wz, params);
    let pixel_area =
        (params.extent / params.image_width as f32) * (params.extent / params.image_height as f32);
    let lambda = col_dens * pixel_area;

    // ── seed RNG from world position (stars follow the galaxy) ─
    let cell: f32 = 10.0; // 0.1 LY cells
    let seed_offset: f32 = 1_000_000.0;
    let qx = ((wx + seed_offset) * cell) as u32;
    let qz = ((wz + seed_offset) * cell) as u32;
    let mut rng = hash3(qx, qz, 42u32);

    // ── star light ─────────────────────────────────────
    let rgb: Vec3;

    if lambda < LAMBDA_INDIVIDUAL {
        // Individual Poisson-sampled stars.
        let n = poisson(lambda, &mut rng);
        if n == 0 {
            rgba[idx] = 0xff_00_00_00;
            return;
        }
        let mut acc = Vec3::new(0.0, 0.0, 0.0);
        for _ in 0..n {
            let mass = sample_imf(&mut rng);
            let lum = mass_to_lum(mass);
            let temp = mass_to_temp(mass);
            acc += temperature_to_rgb(temp) * lum;
        }
        rgb = acc;
    } else if lambda < LAMBDA_ANALYTIC {
        // Blend individual → analytic.
        let n = poisson(lambda, &mut rng);
        let mut star_acc = Vec3::new(0.0, 0.0, 0.0);
        for _ in 0..n {
            let mass = sample_imf(&mut rng);
            let lum = mass_to_lum(mass);
            let temp = mass_to_temp(mass);
            star_acc += temperature_to_rgb(temp) * lum;
        }
        let analytic =
            Vec3::new(MEAN_SPECTRAL_R, MEAN_SPECTRAL_G, MEAN_SPECTRAL_B) * lambda * MEAN_IMF_LUM;
        let t = (lambda - LAMBDA_INDIVIDUAL) / (LAMBDA_ANALYTIC - LAMBDA_INDIVIDUAL);
        let t_s = t * t * (3.0 - 2.0 * t); // smoothstep
        rgb = star_acc * (1.0 - t_s) + analytic * t_s;
    } else {
        // Analytic expected light (acts as a density field).
        rgb = Vec3::new(MEAN_SPECTRAL_R, MEAN_SPECTRAL_G, MEAN_SPECTRAL_B) * lambda * MEAN_IMF_LUM;
    }

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
    rgba[idx] = r8 << 16 | g8 << 8 | b8 | 0xff_00_00_00;
}
