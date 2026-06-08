use glam::DVec3;

/// Morgan–Keenan spectral classification
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpectralType {
    O, // > 30 000 K — rare, blue, short-lived
    B, // 10 000–30 000 K
    A, // 7 500–10 000 K
    F, // 6 000–7 500 K
    G, // 5 200–6 000 K  (Sun is G2V)
    K, // 3 700–5 200 K
    M, // < 3 700 K — most common, red dwarfs
}

impl SpectralType {
    /// Peak wavelength in nm (Wien's law approximation)
    pub fn temperature_kelvin(self) -> f64 {
        match self {
            SpectralType::O => 35_000.0,
            SpectralType::B => 20_000.0,
            SpectralType::A => 8_500.0,
            SpectralType::F => 6_500.0,
            SpectralType::G => 5_500.0,
            SpectralType::K => 4_000.0,
            SpectralType::M => 3_000.0,
        }
    }

    /// Approximate bolometric absolute magnitude
    pub fn absolute_magnitude(self) -> f64 {
        match self {
            SpectralType::O => -5.5,
            SpectralType::B => -2.0,
            SpectralType::A => 1.5,
            SpectralType::F => 3.5,
            SpectralType::G => 4.8,
            SpectralType::K => 6.5,
            SpectralType::M => 12.0,
        }
    }

    /// Rough mass in solar masses
    pub fn mass_solar(self) -> f64 {
        match self {
            SpectralType::O => 40.0,
            SpectralType::B => 8.0,
            SpectralType::A => 2.2,
            SpectralType::F => 1.4,
            SpectralType::G => 1.0,
            SpectralType::K => 0.7,
            SpectralType::M => 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Star {
    pub position: DVec3,
    pub spectral_type: SpectralType,
}

#[derive(Debug, Clone)]
pub struct GalaxyParams {
    // ── disk ──────────────────────────────────
    /// Radial scale length (exponential drop-off).
    pub disk_scale_length: f64,
    /// Scale height (vertical sech²).
    pub disk_scale_height: f64,
    /// Central surface density of the disk (stars / ly³ at r=0, z=0).
    pub disk_central_density: f64,

    // ── spiral arms ───────────────────────────
    /// Number of logarithmic spiral arms.
    pub arm_count: u32,
    /// Pitch angle in radians (tightness of the winding).
    pub arm_pitch: f64,
    /// How concentrated each arm is azimuthally (larger = tighter).
    pub arm_concentration: f64,
    /// Relative density enhancement of the arms over the smooth disk.
    pub arm_strength: f64,

    // ── central bulge ─────────────────────────
    /// Scale radius of the bulge (Plummer / spherical).
    pub bulge_radius: f64,
    /// Central density of the bulge (stars / ly³).
    pub bulge_central_density: f64,

    // ── stellar halo ──────────────────────────
    /// Halo core radius.
    pub halo_radius: f64,
    /// Halo central density.
    pub halo_central_density: f64,
    /// Halo outer power-law slope (typically ≈ -3 to -3.5).
    pub halo_slope: f64,
}

impl GalaxyParams {
    pub fn milky_way() -> Self {
        Self {
            disk_scale_length: 8_500.0,
            disk_scale_height: 800.0,
            disk_central_density: 0.1,

            arm_count: 4,
            arm_pitch: 0.2,
            arm_concentration: 4.0,
            arm_strength: 1.5,

            bulge_radius: 2_000.0,
            bulge_central_density: 2.0,

            halo_radius: 15_000.0,
            halo_central_density: 1e-5,
            halo_slope: -3.0,
        }
    }
}

impl GalaxyParams {
    pub fn density(&self, pos: DVec3) -> f64 {
        let r = (pos.x * pos.x + pos.z * pos.z).sqrt();
        let z = pos.y.abs();

        self.disk_density(r, z) * self.arm_modulation(pos, r)
            + self.bulge_density(pos.length())
            + self.halo_density(pos.length())
    }

    fn disk_density(&self, r: f64, z: f64) -> f64 {
        if self.disk_scale_length <= 0.0 || self.disk_scale_height <= 0.0 {
            return 0.0;
        }
        let radial = (-r / self.disk_scale_length).exp();
        let zeta = z / self.disk_scale_height;
        let sech = 1.0 / zeta.cosh();
        self.disk_central_density * radial * sech * sech
    }

    fn arm_modulation(&self, pos: DVec3, r: f64) -> f64 {
        if self.arm_count == 0 || self.arm_strength <= 0.0 {
            return 1.0;
        }
        let theta = pos.x.atan2(pos.z);
        let log_spiral = theta - (r / self.disk_scale_length) * self.arm_pitch;

        let arm_width = 1.0 / self.arm_concentration;
        let mut min_dtheta = std::f64::consts::PI;
        for k in 0..self.arm_count {
            let phase =
                log_spiral + 2.0 * std::f64::consts::PI * (k as f64) / (self.arm_count as f64);
            let dtheta = phase.rem_euclid(2.0 * std::f64::consts::PI);
            let dtheta = if dtheta > std::f64::consts::PI {
                dtheta - 2.0 * std::f64::consts::PI
            } else {
                dtheta
            };
            min_dtheta = min_dtheta.min(dtheta.abs());
        }

        1.0 + self.arm_strength * (-0.5 * (min_dtheta / arm_width).powi(2)).exp()
    }

    fn bulge_density(&self, dist: f64) -> f64 {
        if self.bulge_radius <= 0.0 {
            return 0.0;
        }
        let x = dist / self.bulge_radius;
        self.bulge_central_density * (1.0 + x * x).powf(-2.5)
    }

    fn halo_density(&self, dist: f64) -> f64 {
        if self.halo_radius <= 0.0 || dist < 1e-6 {
            return self.halo_central_density;
        }
        let x = dist / self.halo_radius;
        self.halo_central_density * (1.0 + x).powf(self.halo_slope)
    }
}

// ──────────────────────────────────────────────
//  Deterministic random numbers
// ──────────────────────────────────────────────

/// Tiny PCG-based PRNG — fully deterministic given a `u64` seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(1442695040888963407))
    }

    fn next_u64(&mut self) -> u64 {
        let x = self.0;
        self.0 = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let rot = (x >> 59) as u32;
        ((x >> 18) ^ x).rotate_right(rot)
    }

    /// Uniform in [0, 1).
    fn f64(&mut self) -> f64 {
        // Shift right by 11 → 53-bit integer; divide by 2^53.
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Normal(0,1) via Box–Muller.
    fn normal(&mut self) -> f64 {
        let u = self.f64().max(1e-300);
        let v = self.f64();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// Hash an integer cell coordinate into a `u64` seed.
fn hash_cell(ix: i64, iy: i64, iz: i64) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= ix as u64;
    h = h.wrapping_mul(0x9e3779b97f4a7c15);
    h ^= iy as u64;
    h = h.wrapping_mul(0x9e3779b97f4a7c15);
    h ^= iz as u64;
    h = h.wrapping_mul(0x9e3779b97f4a7c15);
    h
}

// ──────────────────────────────────────────────
//  Star generation inside a cell
// ──────────────────────────────────────────────

/// Deterministically sample stars inside an axis-aligned cube
/// `[origin, origin + (size, size, size)]`.
///
/// The star count follows a Poisson distribution whose mean is the
/// density integral over the cell (approximated by the density at the
/// cell centre × volume).  Individual star positions are hashed from
/// the cell seed + star index so they are repeatable across frames.
pub fn generate_stars_in_cell(
    cell_origin: DVec3,
    cell_size: f64,
    params: &GalaxyParams,
) -> Vec<Star> {
    let center = cell_origin + DVec3::splat(cell_size * 0.5);
    let density = params.density(center);
    let expected = density * cell_size.powi(3);

    if expected <= 0.0 {
        return Vec::new();
    }

    // Determine the actual count with a Poisson draw.
    let cell_hash = hash_cell(
        (cell_origin.x / cell_size).round() as i64,
        (cell_origin.y / cell_size).round() as i64,
        (cell_origin.z / cell_size).round() as i64,
    );
    let mut rng = Rng::new(cell_hash);
    let count = poisson(expected, &mut rng);

    if count == 0 {
        return Vec::new();
    }

    // Generate star positions and types.
    let mut stars = Vec::with_capacity(count as usize);
    for i in 0..count {
        let star_hash = hash_cell(
            (cell_origin.x / cell_size).round() as i64,
            (cell_origin.y / cell_size).round() as i64,
            (cell_origin.z / cell_size).round() as i64,
        )
        .wrapping_add(i as u64);
        let mut rng = Rng::new(star_hash);

        let x = rng.f64();
        let y = rng.f64();
        let z = rng.f64();
        let position = cell_origin + DVec3::new(x, y, z) * cell_size;

        let spectral_type = random_spectral(&mut rng);

        stars.push(Star {
            position,
            spectral_type,
        });
    }
    stars
}

/// Draw from a Poisson distribution with mean `lambda`.
fn poisson(lambda: f64, rng: &mut Rng) -> u32 {
    if lambda <= 0.0 {
        return 0;
    }
    if lambda > 30.0 {
        // Normal approximation for large lambda.
        let n = (lambda + lambda.sqrt() * rng.normal()).round() as i64;
        return n.max(0) as u32;
    }
    let l = (-lambda).exp();
    let mut k: u32 = 0;
    let mut p = 1.0;
    loop {
        k += 1;
        p *= rng.f64();
        if p <= l {
            return k - 1;
        }
    }
}

/// Sample a spectral type from the initial mass function (IMF).
///
/// Uses the Kroupa IMF broken power-law, then maps mass → spectral type.
fn random_spectral(rng: &mut Rng) -> SpectralType {
    let mass = random_mass_kroupa(rng);
    mass_to_spectral_type(mass)
}

/// Kroupa (2001) IMF — probability density ∝ m^{−α} with breaks at 0.08 and 0.5 M☉.
///
/// α = 2.3  for  m > 0.5 M☉
/// α = 1.3  for  0.08 < m ≤ 0.5 M☉
/// α = 0.3  for  0.01 < m ≤ 0.08 M☉
fn random_mass_kroupa(rng: &mut Rng) -> f64 {
    // Pre-computed CDF breakpoints (normalised continuous Kroupa IMF).
    const CDF_BROWN: f64 = 0.372; // P(m ∈ [0.01, 0.08] M☉)
    const CDF_MID: f64 = 0.850; // P(m ∈ [0.01, 0.5 ] M☉)

    let u = rng.f64();

    if u < CDF_BROWN {
        // α = 0.3  from 0.01 → 0.08 M☉  (brown dwarfs)
        let m_min: f64 = 0.01;
        let m_max: f64 = 0.08;
        let uu = u / CDF_BROWN;
        (uu * (m_max.powf(0.7) - m_min.powf(0.7)) + m_min.powf(0.7)).powf(1.0 / 0.7)
    } else if u < CDF_MID {
        // α = 1.3  from 0.08 → 0.5 M☉
        let m_min: f64 = 0.08;
        let m_max: f64 = 0.5;
        let uu = (u - CDF_BROWN) / (CDF_MID - CDF_BROWN);
        (uu * (m_max.powf(-0.3) - m_min.powf(-0.3)) + m_min.powf(-0.3)).powf(-1.0 / 0.3)
    } else {
        // α = 2.3  from 0.5 → 60 M☉
        let m_min: f64 = 0.5;
        let m_max: f64 = 60.0;
        let uu = (u - CDF_MID) / (1.0 - CDF_MID);
        (uu * (m_max.powf(-1.3) - m_min.powf(-1.3)) + m_min.powf(-1.3)).powf(-1.0 / 1.3)
    }
}

fn mass_to_spectral_type(mass_solar: f64) -> SpectralType {
    if mass_solar > 16.0 {
        SpectralType::O
    } else if mass_solar > 2.1 {
        SpectralType::B
    } else if mass_solar > 1.4 {
        SpectralType::A
    } else if mass_solar > 1.04 {
        SpectralType::F
    } else if mass_solar > 0.8 {
        SpectralType::G
    } else if mass_solar > 0.45 {
        SpectralType::K
    } else {
        SpectralType::M
    }
}
