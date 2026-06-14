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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milky_way_params_are_finite() {
        let p = GalaxyParams::milky_way();
        assert!(p.disk_scale_length.is_finite() && p.disk_scale_length > 0.0);
        assert!(p.disk_scale_height.is_finite() && p.disk_scale_height > 0.0);
        assert!(p.disk_central_density.is_finite() && p.disk_central_density > 0.0);
        assert!(p.arm_count > 0);
        assert!(p.arm_pitch.is_finite());
        assert!(p.arm_concentration.is_finite() && p.arm_concentration > 0.0);
        assert!(p.arm_strength.is_finite());
        assert!(p.bulge_radius.is_finite() && p.bulge_radius > 0.0);
        assert!(p.bulge_central_density.is_finite() && p.bulge_central_density > 0.0);
        assert!(p.halo_radius.is_finite() && p.halo_radius > 0.0);
        assert!(p.halo_central_density.is_finite());
        assert!(p.halo_slope.is_finite() && p.halo_slope < 0.0);
    }

    #[test]
    fn milky_way_params_clone_is_equal() {
        let p1 = GalaxyParams::milky_way();
        let p2 = p1.clone();
        // Compare fields individually since we didn't derive PartialEq
        assert_eq!(
            p1.disk_scale_length.to_bits(),
            p2.disk_scale_length.to_bits()
        );
        assert_eq!(
            p1.disk_central_density.to_bits(),
            p2.disk_central_density.to_bits()
        );
        assert_eq!(p1.arm_count, p2.arm_count);
        assert_eq!(p1.arm_pitch.to_bits(), p2.arm_pitch.to_bits());
        assert_eq!(p1.bulge_radius.to_bits(), p2.bulge_radius.to_bits());
        assert_eq!(p1.halo_slope.to_bits(), p2.halo_slope.to_bits());
    }

    #[test]
    fn milky_way_params_cast_to_f32_is_finite() {
        let p = GalaxyParams::milky_way();
        // Verify all fields can be cast to f32 without overflow/inf
        assert!((p.disk_scale_length as f32).is_finite());
        assert!((p.disk_scale_height as f32).is_finite());
        assert!((p.disk_central_density as f32).is_finite());
        assert!((p.arm_pitch as f32).is_finite());
        assert!((p.arm_concentration as f32).is_finite());
        assert!((p.arm_strength as f32).is_finite());
        assert!((p.bulge_radius as f32).is_finite());
        assert!((p.bulge_central_density as f32).is_finite());
        assert!((p.halo_radius as f32).is_finite());
        assert!((p.halo_central_density as f32).is_finite());
        // halo_slope is negative; casting to f32 is fine
        assert!((p.halo_slope as f32).is_finite());
    }
}
