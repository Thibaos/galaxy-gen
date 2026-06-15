#[derive(Debug, Clone)]
pub struct GalaxyParams {
    // ── disk ──────────────────────────────────
    /// Radial scale length (exponential drop-off).
    pub disk_scale_length: f64,
    /// Scale height (vertical sech²).
    pub disk_scale_height: f64,
    /// Midplane stellar number density at the galactic centre (stars / ly³).
    /// Derived from Milky Way central mass surface density (~650 M☉/pc²)
    /// divided by the full Kroupa IMF mean stellar mass (≈7.4 M☉).
    pub disk_central_density: f64,

    // ── spiral arms ───────────────────────────
    /// Number of logarithmic spiral arms.
    pub arm_count: u32,
    /// Pitch angle in radians (tightness of the winding).
    pub arm_pitch: f64,
    /// How concentrated each arm is azimuthally (larger = tighter).
    pub arm_concentration: f64,
    /// Relative density enhancement of the arms over the smooth disk.
    /// Physical value: spiral arm over-density is ~2× (arm_strength = 1).
    pub arm_strength: f64,

    // ── central bulge ─────────────────────────
    /// Scale radius of the bulge (Plummer / spherical).
    pub bulge_radius: f64,
    /// Central density of the bulge (stars / ly³).
    /// From Milky Way bulge mass (1.84×10¹⁰ M☉) within Plummer
    /// radius (2500 ly) converted to number density via full Kroupa IMF.
    pub bulge_central_density: f64,

    // ── stellar halo ──────────────────────────
    /// Halo core radius.
    pub halo_radius: f64,
    /// Halo central density (stars / ly³).
    /// From stellar halo mass (≈10⁹ M☉) within the 23 kpc break radius,
    /// giving ≈2 % of the disk column at the solar circle (full IMF).
    pub halo_central_density: f64,
    /// Halo outer power-law slope (typically ≈ -3 to -3.5).
    pub halo_slope: f64,
}

impl GalaxyParams {
    pub fn milky_way() -> Self {
        Self {
            disk_scale_length: 8_500.0,
            disk_scale_height: 950.0,
            // Physical: central stellar number density from mass surface
            // density Σ₀ ≈ 650 M☉ pc⁻², mean mass ⟨m⟩ ≈ 7.4 M☉ (full
            // Kroupa IMF 0.08–100 M☉), H = 950 ly.
            // n0 = Σ₀ / (⟨m⟩ · 2H) = 61.5 / (7.4 · 1900) ≈ 0.00437.
            disk_central_density: 4.37e-3,

            arm_count: 4,
            arm_pitch: 0.22,
            arm_concentration: 5.0,
            // Physical: spiral arms enhance surface density by ≈2×.
            arm_strength: 1.0,

            bulge_radius: 2_500.0,
            // Physical: central bulge number density from Plummer model
            // with M ≈ 1.84 × 10¹⁰ M☉, a = 2500 ly.
            // ρ₀ = 3M/(4πa³) / ⟨m⟩ ≈ 0.282 / 7.4 ≈ 0.0381.
            bulge_central_density: 0.0381,

            halo_radius: 75_000.0,
            // Physical: halo number density calibrated so the halo column
            // at the solar circle (8 kpc) is ≈2 % of the disk column,
            // consistent with stellar halo mass fraction (full IMF).
            halo_central_density: 6.9e-8,
            halo_slope: -3.5,
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
