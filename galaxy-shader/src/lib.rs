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
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
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
//  Stars: hashing, IMF, and colour helpers
// ═══════════════════════════════════════════════════════════

/// PCG-style hash for deterministic world-space hashing.
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

/// Mass–luminosity (main-sequence approximation in solar units).
///
/// L ∝ M³·⁵ for 0.5–2 M☉,  L ∝ M for massive stars.
const L_BREAK: f32 = 11.313_708; // 2.0^3.5

fn mass_to_lum(m: f32) -> f32 {
    if m < 2.0 {
        m.powf(3.5)
    } else {
        L_BREAK * (m / 2.0)
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
//  World-space deterministic star grid
//
//  Every 0.1-LY "star cell" gets a deterministic yes/no star
//  decision and a deterministic Kroupa-IMF mass from its integer
//  grid index.  The same world location always produces the
//  same stars — fully zoom-invariant.
//
//  Performance: column density is read ONCE at pixel centre
//  and reused for all cells — galaxy profiles vary on kLY
//  scales, so 0.1-LY variation is negligible.
//
//  Stability: a FIXED-SIZE window centred on the pixel's
//  centre cell is used instead of stride-subsampling.
//  Window bounds shift only when the centre crosses a cell
//  boundary, which is exactly the expected scrolling behaviour.
// ═══════════════════════════════════════════════════════════

/// Size of one star cell in light-years.
const STAR_CELL_SIZE: f32 = 0.1;

/// Precomputed 1.0 / STAR_CELL_SIZE for multiply instead of divide.
const INV_STAR_CELL_SIZE: f32 = 10.0;

/// Max star cells iterated per pixel (prevents GPU timeout
/// at extreme zoom-out in sparse regions).

/// Fixed window side length = √MAX_STAR_CELLS = 16.
const WINDOW_SIDE: u32 = 16;

/// World-coordinate offset that keeps cell indices >= 0.
const STAR_OFFSET: f32 = 1_000_000.0;

/// Map a world coordinate to a non-negative star-cell index.
fn star_cell(w: f32) -> u32 {
    ((w + STAR_OFFSET) * INV_STAR_CELL_SIZE) as u32
}

/// Deterministic check: does this star cell contain at least one star?
fn cell_has_star(cx: u32, cz: u32, star_prob: f32) -> bool {
    let h = hash3(cx, cz, 77u32);
    let r = (h >> 8) as f32 / 16777215.0;
    r < star_prob
}

/// Deterministic IMF mass sample from cell coordinates.
///
/// Full Kroupa (2001) IMF, two segments:
///   Segment 1:  α=1.3,  m ∈ [0.08, 0.5]  M☉  (low-mass dwarfs, 61.38 % of stars)
///   Segment 2:  α=2.3,  m ∈ [0.5,  100]  M☉  (massive stars)
///
/// Inverse-CDF constants are pre-computed so the shader avoids
/// transcendental calls at sample time.

// ── Segment 1:  α=1.3,  e=−0.3 ──
const IMF1_M_MIN_POW: f32 = 2.133_391_9; // 0.08^(−0.3)
const IMF1_RANGE: f32 = -0.902_161_4; // 0.5^(−0.3) − 0.08^(−0.3)
const IMF1_INV_E: f32 = -3.333_333_3; // 1/(−0.3)

// ── Segment 2:  α=2.3,  e=−1.3 ──
const IMF2_M_MIN_POW: f32 = 2.462_288_8; // 0.5^(−1.3)
const IMF2_RANGE: f32 = -2.459_776_8; // 100^(−1.3) − 0.5^(−1.3)
const IMF2_INV_E: f32 = -0.769_230_8; // 1/(−1.3)

/// Random fraction of stars belonging to segment 1, as a u32 threshold.
const IMF_SEG_THRESH: u32 = 2_636_411_560;

fn sample_imf_from_cell(cx: u32, cz: u32) -> f32 {
    let h = hash3(cx, cz, 123u32);
    let seg = h;
    let u = (h >> 8) as f32 / 16777215.0;

    if seg < IMF_SEG_THRESH {
        // Segment 1: 0.08 – 0.5 M☉  (low-mass dwarfs)
        (IMF1_M_MIN_POW + IMF1_RANGE * u).powf(IMF1_INV_E)
    } else {
        // Segment 2: 0.5 – 100 M☉ (massive stars)
        (IMF2_M_MIN_POW + IMF2_RANGE * u).powf(IMF2_INV_E)
    }
}

/// Deterministic star colour + luminosity for a cell.
fn cell_star_light(cx: u32, cz: u32) -> Vec3 {
    let mass = sample_imf_from_cell(cx, cz);
    let lum = mass_to_lum(mass);
    let temp = mass_to_temp(mass);
    temperature_to_rgb(temp) * lum
}

/// Sample the star grid over a pixel's world-space footprint.
/// Column density is pre-computed at pixel centre and assumed
/// constant across the footprint — a < 0.1 % error for typical
/// galaxy profiles.
#[allow(clippy::manual_saturating_arithmetic)]
fn sample_star_grid(wx: f32, wz: f32, pixel_w: f32, pixel_h: f32, col_dens: f32) -> Vec3 {
    let half_w = pixel_w * 0.5;
    let half_h = pixel_h * 0.5;

    // Precompute the star-existence probability once per pixel.
    // P(≥1 star in cell) = 1 − exp(−col_dens × cell_area).
    let lambda_cell = col_dens * STAR_CELL_SIZE * STAR_CELL_SIZE;
    let star_prob = 1.0 - (-lambda_cell).exp();

    // How many star cells does the pixel footprint span?
    let cells_x = ((pixel_w * INV_STAR_CELL_SIZE).ceil() as u32).max(1);
    let cells_z = ((pixel_h * INV_STAR_CELL_SIZE).ceil() as u32).max(1);
    let pixel_cells = cells_x * cells_z;

    // Choose between full-footprint iteration (small pixels)
    // and fixed-window subsampling (large pixels).
    let min_cx: u32;
    let max_cx: u32;
    let min_cz: u32;
    let max_cz: u32;
    let weight: f32;

    if cells_x <= WINDOW_SIDE && cells_z <= WINDOW_SIDE {
        min_cx = star_cell(wx - half_w);
        max_cx = star_cell(wx + half_w);
        min_cz = star_cell(wz - half_h);
        max_cz = star_cell(wz + half_h);
        weight = 1.0;
    } else {
        let cx_center = star_cell(wx);
        let cz_center = star_cell(wz);
        let half = WINDOW_SIDE / 2;
        min_cx = cx_center - half.min(cx_center);
        max_cx = cx_center + half;
        min_cz = cz_center - half.min(cz_center);
        max_cz = cz_center + half;
        let sampled = WINDOW_SIDE * WINDOW_SIDE;
        weight = pixel_cells as f32 / sampled as f32;
    }

    let mut light = Vec3::new(0.0, 0.0, 0.0);
    let mut cx = min_cx;
    while cx <= max_cx {
        let mut cz = min_cz;
        while cz <= max_cz {
            if cell_has_star(cx, cz, star_prob) {
                light += cell_star_light(cx, cz) * weight;
            }
            cz += 1;
        }
        cx += 1;
    }

    light
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

    // ── world position ─────────────────────────────────
    let extent_x = params.extent;
    let extent_y = params.extent * (params.image_height as f32 / params.image_width as f32);
    let wx = (px as f32 / params.image_width as f32 - 0.5) * extent_x + params.center_x;
    let wz = -(py as f32 / params.image_height as f32 - 0.5) * extent_y + params.center_y;

    // ── star light ─────────────────────────────────────
    let col_dens = column_density(wx, wz, params);
    let pixel_w = extent_x / params.image_width as f32;
    let pixel_h = extent_y / params.image_height as f32;

    let rgb = sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens);

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
