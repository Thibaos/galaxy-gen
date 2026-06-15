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

    /// NGC 628 (M74, Phantom Galaxy) — face-on grand-design Sc spiral.
    ///
    /// Photometric parameters from the S4G survey (Salo et al. 2015,
    /// Pipeline 4, 3.6 μm bulge+disk decomposition).  Stellar masses
    /// from PHANGS (Leroy et al. 2021).  Halo values are scaled from
    /// the Milky Way preset as external-galaxy halo data is scarce.
    ///
    /// Full reference data: `galaxies/ngc628.toml`.
    pub fn ngc628() -> Self {
        Self {
            // ── disk ──
            // Exponential scale length from S4G: hr = 69.3 arcsec.
            // At d = 9.7 Mpc, 1" = 153.4 ly → hr = 10,630 ly = 3.26 kpc.
            disk_scale_length: 10_630.0,
            // Scale height NOT directly measurable for face-on galaxies.
            // Outer-disk Hα knots suggest hz ~ 400-700 pc (Herrmann+2009).
            // Typical Sc thin-disk ratio hz/hr ~ 0.09 → ~290 pc.
            // We adopt 350 pc as a conservative mid-range estimate.
            disk_scale_height: 1_140.0,
            // Central number density from total disk stellar mass.
            //  M_disk = 2.057 × 10¹⁰ M☉ (93.5 % of 2.2e10 total).
            //  Σ₀ = M_disk / (2π hr²) ≈ 29.0 M☉ ly⁻².
            //  n₀ = Σ₀ / (2hz · ⟨m⟩) with ⟨m⟩ ≈ 7.4 M☉ (full Kroupa IMF).
            //  n₀ ≈ 29.0 / (2 · 1140 · 7.4) ≈ 0.00172.
            disk_central_density: 1.72e-3,

            // ── spiral arms ──
            // NGC 628 is multi-armed with a grand-design pattern.
            // We model with 4 arms, pitch angle ~15° (estimated from
            // published images; no single consensus measurement exists).
            arm_count: 4,
            arm_pitch: 0.262, // 15°
            arm_concentration: 5.0,
            arm_strength: 1.0, // 2× arm/interarm contrast

            // ── central bulge ──
            // S4G Sérsic bulge: Re = 12.2 arcsec = 1,871 ly.
            // For a Plummer model in projection, Re = a exactly, so
            // the Plummer scale radius equals the effective radius.
            bulge_radius: 1_871.0,
            // Bulge central density from bulge stellar mass.
            //  M_bulge = 6.5 % × 2.2e10 = 1.43 × 10⁹ M☉.
            //  Plummer: ρ₀ = 3M / (4π a³) ≈ 0.0521 M☉ ly⁻³.
            //  Number density: ρ₀ / ⟨m⟩ = 0.0521 / 7.4 ≈ 0.00704.
            bulge_central_density: 7.04e-3,

            // ── stellar halo ──
            // Not constrained for external galaxies.
            // Using Milky-Way-like values.
            halo_radius: 75_000.0,
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

    // ── NGC 628 (M74) preset tests ──────────────────

    #[test]
    fn ngc628_params_are_finite() {
        let p = GalaxyParams::ngc628();
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
    fn ngc628_disk_larger_than_milky_way() {
        let mw = GalaxyParams::milky_way();
        let ngc = GalaxyParams::ngc628();
        // NGC 628's disk scale length (~10.6 kly) > Milky Way's (~8.5 kly)
        assert!(
            ngc.disk_scale_length > mw.disk_scale_length,
            "NGC 628 disk scale length {} should be > MW {}",
            ngc.disk_scale_length,
            mw.disk_scale_length
        );
    }

    #[test]
    fn ngc628_bulge_is_fainter_than_milky_way() {
        let mw = GalaxyParams::milky_way();
        let ngc = GalaxyParams::ngc628();
        // NGC 628 is type Sc with a very faint pseudo-bulge (6.5 % flux).
        // Its bulge central density should be much lower than the Milky Way's.
        assert!(
            ngc.bulge_central_density < mw.bulge_central_density,
            "NGC 628 bulge density {} should be < MW {}",
            ngc.bulge_central_density,
            mw.bulge_central_density
        );
    }

    #[test]
    fn ngc628_bulge_to_disk_column_ratio_is_small() {
        let p = GalaxyParams::ngc628();
        // The bulge is compact (a = 1,871 ly) while the disk is extended
        // (hr = 10,630 ly).  Even though the bulge is only 6.5 % of total
        // stellar mass, its surface density at the centre is HIGHER than
        // the disk — this is expected for a Plummer bulge.
        // We verify the ratio is physically reasonable (not zero, not absurd).
        // Σ_bulge(0) / Σ_disk(0) ≈ (M_b/M_d) × 2(hr/a)² ≈ 4.5
        let bulge_col = (4.0 / 3.0) * p.bulge_radius * p.bulge_central_density;
        let disk_col = 2.0 * p.disk_scale_height * p.disk_central_density;
        let ratio = bulge_col / disk_col;
        assert!(
            ratio > 2.0,
            "ngc628 bulge/disk column ratio {ratio:.3} should be > 2 (compact bulge)"
        );
        assert!(
            ratio < 10.0,
            "ngc628 bulge/disk column ratio {ratio:.3} should be < 10"
        );
    }
}
