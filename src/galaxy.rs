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
