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
    // ── 3D camera / render mode ──
    pub render_mode: u32, // 0 = 2D (current), 1 = 3D ray-march
    pub camera_x: f32,    // world-space camera position
    pub camera_y: f32,
    pub camera_z: f32,
    pub camera_target_x: f32, // look-at point
    pub camera_target_y: f32,
    pub camera_target_z: f32,
    pub fov_y_deg: f32, // vertical field of view in degrees
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

// ═══════════════════════════════════════════════════════════
//  3D density profiles (used by ray-march path)
// ═══════════════════════════════════════════════════════════

/// Disk 3D density: exponential radial × sech² vertical.
///
/// ρ(R, z) = ρ₀ × exp(−R/h_r) × sech²(z / (2 × h_z))
///
/// sech² is a common fit for isothermal self-gravitating disks
/// (Spitzer 1942, van der Kruit & Searle 1981).
fn disk_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let r = (x * x + z * z).sqrt();
    let sech = 1.0 / (y / (2.0 * p.disk_scale_height)).cosh();
    p.disk_central_density * 0.5 * (-r / p.disk_scale_length).exp() * sech * sech
}

/// Bulge 3D density (Plummer sphere).
///
/// ρ(r) = ρ₀ × (1 + r²/a²)^(−2.5)
fn bulge_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let r2 = x * x + y * y + z * z;
    let x2 = r2 / (p.bulge_radius * p.bulge_radius);
    p.bulge_central_density * (1.0 + x2).powf(-2.5)
}

/// Halo 3D density (power-law sphere).
///
/// ρ(r) = ρ₀ × (1 + r/R_h)^s
fn halo_density_3d(x: f32, y: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 {
        return 0.0;
    }
    let r = (x * x + y * y + z * z).sqrt();
    p.halo_central_density * (1.0 + r / p.halo_radius).powf(p.halo_slope)
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

/// Effective temperature from mass (main sequence).
///
/// Piecewise-log fit to the empirical M–Teff relation from Pecaut &
/// Mamajek (2013, ApJS 208, 9, Table 5) and Eker et al. (2018, MNRAS
/// 479, 5491).  The relation is log₁₀(Teff) = a + b·log₁₀(M) with
/// breakpoints at 0.5, 1.0, 2.4, and 7.0 M☉.
///
/// Reference values (M☉ → K):  0.08→2300, 0.16→3060, 0.50→3750,
/// 0.88→5240, 1.00→5770, 2.00→8180, 2.40→9700, 7.00→22000,
/// 15.0→30000, 60.0→48000.
const T_SOLAR: f32 = 5770.0;
const M_SOLAR: f32 = 1.0;

fn mass_to_temp(m: f32) -> f32 {
    if m <= 0.08 {
        return 2300.0;
    }
    let (ref_m, ref_t, exp) = if m < 0.50 {
        // Very-low-mass segment: T ∝ M^0.53  (anchored at 0.16 M☉ → 3060 K)
        (0.16, 3060.0, 0.53)
    } else if m < 1.0 {
        // Low-mass segment: T ∝ M^0.67  (anchored at 0.88 M☉ → 5240 K)
        (0.88, 5240.0, 0.67)
    } else if m < 2.40 {
        // Solar-mass segment: T ∝ M^0.57  (anchored at 1.0 M☉ → 5770 K)
        (M_SOLAR, T_SOLAR, 0.57)
    } else if m < 7.0 {
        // Intermediate-mass segment: T ∝ M^0.36  (anchored at 2.4 M☉ → 9700 K)
        (2.40, 9700.0, 0.36)
    } else {
        // High-mass segment: T ∝ M^0.20  (anchored at 15.0 M☉ → 30000 K)
        (15.0, 30000.0, 0.20)
    };
    // Cap at 50000 K (O-type stars; color is essentially converged beyond this)
    (ref_t * (m / ref_m).powf(exp)).min(50000.0)
}

// ═══════════════════════════════════════════════════════════
//  Physically-accurate star colour via spectrum-based LUT
//
//  The table is derived from the vendian.org stellar colour
//  datafile (Charity 2001–2002), which averages real stellar
//  spectra (Kurucz, Silva, Pickles) and processes them through
//  CIE 1931 2º CMFs (Judd-Vos), sRGB primaries, and D65
//  whitepoint.  Methodology validated by Harre & Heller (2021).
//
//  Entries are (Teff_K, linear_R, linear_G, linear_B).
//  Colours between entries are linearly interpolated.
// ═══════════════════════════════════════════════════════════

/// Number of entries in the colour lookup table.
const COLOR_LUT_LEN: usize = 16;

/// Temperature→RGB lookup table: (Teff [K], linear R, linear G, linear B).
///
/// Data source: vendian.org starcolor datafile, Main Sequence (Class V).
/// Spectral types mapped to Teff via Pecaut & Mamajek (2013, Table 5).
const COLOR_LUT: [(f32, f32, f32, f32); COLOR_LUT_LEN] = [
    //  Teff(K)    R      G      B      SpT (approx)
    (2300.0, 1.000, 0.745, 0.424),  // M9.5V
    (2600.0, 1.000, 0.765, 0.427),  // M7V  (interpolated)
    (3060.0, 1.000, 0.800, 0.435),  // M5V
    (3400.0, 1.000, 0.808, 0.506),  // M3V
    (3750.0, 1.000, 0.765, 0.545),  // M0V
    (4400.0, 1.000, 0.847, 0.710),  // K4V  (K5 anchor at 4140K)
    (5240.0, 1.000, 0.933, 0.867),  // K0V
    (5770.0, 1.000, 0.961, 0.949),  // G2V (Sun)
    (6540.0, 0.973, 0.969, 1.000),  // F5V
    (7220.0, 0.878, 0.898, 1.000),  // F0V
    (8180.0, 0.792, 0.843, 1.000),  // A5V
    (9700.0, 0.725, 0.788, 1.000),  // A0V
    (15200.0, 0.667, 0.749, 1.000), // B5V
    (26500.0, 0.612, 0.698, 1.000), // B0V
    (41400.0, 0.608, 0.690, 1.000), // O5V
    (50000.0, 0.608, 0.690, 1.000), // O2V (clamped — colour converged)
];

/// Spectrum-based star colour as linear RGB.
///
/// Piecewise-linear interpolation over the baked `COLOR_LUT`.
/// Temperatures outside the covered range are clamped to the nearest endpoint.
fn temperature_to_rgb(t_kelvin: f32) -> Vec3 {
    // Clamp to LUT range
    let t = t_kelvin.clamp(COLOR_LUT[0].0, COLOR_LUT[COLOR_LUT_LEN - 1].0);

    // Find the segment [lo, hi] containing t
    if t <= COLOR_LUT[0].0 {
        return Vec3::new(COLOR_LUT[0].1, COLOR_LUT[0].2, COLOR_LUT[0].3);
    }
    for i in 0..(COLOR_LUT_LEN - 1) {
        if t <= COLOR_LUT[i + 1].0 {
            let t_lo = COLOR_LUT[i].0;
            let t_hi = COLOR_LUT[i + 1].0;
            let frac = (t - t_lo) / (t_hi - t_lo);
            let r = COLOR_LUT[i].1 + frac * (COLOR_LUT[i + 1].1 - COLOR_LUT[i].1);
            let g = COLOR_LUT[i].2 + frac * (COLOR_LUT[i + 1].2 - COLOR_LUT[i].2);
            let b = COLOR_LUT[i].3 + frac * (COLOR_LUT[i + 1].3 - COLOR_LUT[i].3);
            return Vec3::new(r, g, b);
        }
    }
    // t > last entry
    let last = COLOR_LUT[COLOR_LUT_LEN - 1];
    Vec3::new(last.1, last.2, last.3)
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

/// Fixed window side length = 16 cells (256 cells total).
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
//  3D ray-march path
// ═══════════════════════════════════════════════════════════

/// Number of ray-march steps along the camera ray.
const RAY_STEPS: u32 = 64;

/// Maximum ray-march distance in light-years (≈ galaxy far-field radius).
const RAY_MAX_DIST: f32 = 200_000.0;

/// 3D ray-march: returns accumulated emissivity as linear RGB.
///
/// Each step samples the 3D density field, multiplies by an
/// emissivity-per-unit-density factor, and accumulates.
/// The camera ray is constructed from the uniform's camera position,
/// look-at target, and FOV.
fn ray_march_galaxy(px: u32, py: u32, p: &GalaxyUniform) -> Vec3 {
    let aspect = p.image_width as f32 / p.image_height as f32;

    // Camera basis vectors
    let forward_x = p.camera_target_x - p.camera_x;
    let forward_y = p.camera_target_y - p.camera_y;
    let forward_z = p.camera_target_z - p.camera_z;
    let forward_len =
        (forward_x * forward_x + forward_y * forward_y + forward_z * forward_z).sqrt();
    if forward_len < 1e-6 {
        return Vec3::new(0.0, 0.0, 0.0);
    }
    let fx = forward_x / forward_len;
    let fy = forward_y / forward_len;
    let fz = forward_z / forward_len;

    // Right = forward × world_up (0, 1, 0)
    let world_up_x: f32 = 0.0;
    let world_up_y: f32 = 1.0;
    let world_up_z: f32 = 0.0;
    let rx = fy * world_up_z - fz * world_up_y;
    let ry = fz * world_up_x - fx * world_up_z;
    let rz = fx * world_up_y - fy * world_up_x;
    let r_len = (rx * rx + ry * ry + rz * rz).sqrt();
    let (rx, ry, rz) = if r_len < 1e-6 {
        // Camera looking straight up/down — use an arbitrary right vector
        (1.0, 0.0, 0.0)
    } else {
        (rx / r_len, ry / r_len, rz / r_len)
    };

    // Up = right × forward
    let ux = ry * fz - rz * fy;
    let uy = rz * fx - rx * fz;
    let uz = rx * fy - ry * fx;

    // Screen-space to ray direction
    let half_h = (0.5 * p.fov_y_deg).to_radians().tan();
    let half_w = half_h * aspect;
    let sx = (px as f32 / p.image_width as f32 - 0.5) * 2.0;
    let sy = -(py as f32 / p.image_height as f32 - 0.5) * 2.0;

    let dir_x = fx + sx * half_w * rx + sy * half_h * ux;
    let dir_y = fy + sx * half_w * ry + sy * half_h * uy;
    let dir_z = fz + sx * half_w * rz + sy * half_h * uz;
    let dir_len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z).sqrt();
    let dx = dir_x / dir_len;
    let dy = dir_y / dir_len;
    let dz = dir_z / dir_len;

    // ── Ray-march ──
    let dt = RAY_MAX_DIST / RAY_STEPS as f32;
    let mut acc = Vec3::new(0.0, 0.0, 0.0);

    // Start at t = 0 (camera position) or t_near
    let mut t = 0.0;
    for _ in 0..RAY_STEPS {
        let sx_pos = p.camera_x + t * dx;
        let sy_pos = p.camera_y + t * dy;
        let sz_pos = p.camera_z + t * dz;
        t += dt;

        let r = (sx_pos * sx_pos + sz_pos * sz_pos).sqrt();

        // Skip if outside galaxy bounds
        if r > RAY_MAX_DIST * 0.5 {
            continue;
        }

        let dens = disk_density_3d(sx_pos, sy_pos, sz_pos, p)
            * arm_modulation_2d(sx_pos, sz_pos, r, p)
            + bulge_density_3d(sx_pos, sy_pos, sz_pos, p)
            + halo_density_3d(sx_pos, sy_pos, sz_pos, p);

        if dens <= 0.0 {
            continue;
        }

        // Smooth emissivity: average stellar light per unit density.
        // Using a constant warm-white colour (solar-like) scaled by a
        // calibrated factor.  Individual bright star points are rendered
        // separately by the instanced star pass (plan 012).
        const EMISSIVITY: f32 = 1.5;
        acc.x += dens * dt * EMISSIVITY;
        acc.y += dens * dt * EMISSIVITY * 0.9;
        acc.z += dens * dt * EMISSIVITY * 0.7;
    }

    acc
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

    // ── compute rgb based on render mode ──
    let rgb = if params.render_mode == 0 {
        // ── 2D path (current behaviour) ──
        let extent_x = params.extent;
        let extent_y = params.extent * (params.image_height as f32 / params.image_width as f32);
        let pixel_w = extent_x / params.image_width as f32;
        let pixel_h = extent_y / params.image_height as f32;
        let wx = (px as f32 / params.image_width as f32 - 0.5) * extent_x + params.center_x;
        let wz = -(py as f32 / params.image_height as f32 - 0.5) * extent_y + params.center_y;

        let col_dens = column_density(wx, wz, params);
        sample_star_grid(wx, wz, pixel_w, pixel_h, col_dens)
    } else {
        // ── 3D ray-march path ──
        ray_march_galaxy(px, py, params)
    };

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
