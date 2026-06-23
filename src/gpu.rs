use std::time::Instant;

use bytemuck::{Pod, Zeroable};

use crate::galaxy::GalaxyParams;

pub struct GpuCompute {
    pub module: wgpu::ShaderModule,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
}

impl GpuCompute {
    pub fn new(device: &wgpu::Device) -> Self {
        let spirv_bytes = include_bytes!(env!("galaxy_shader.spv"));
        assert!(
            spirv_bytes.len().is_multiple_of(4),
            "SPIR-V binary not word-aligned"
        );
        let spirv_words: Vec<u32> = spirv_bytes
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("galaxy-shader"),
            source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Owned(spirv_words)),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("galaxy-scene"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("render_scene"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            module,
            bind_group_layout,
            pipeline,
        }
    }
}

/// GPU uniform buffer matching `galaxy-shader/src/lib.rs:GalaxyUniform`.
/// The two definitions MUST stay in sync. Field-offset tests in this
/// module's `#[cfg(test)]` block will catch mismatches at test time.
#[derive(Copy, Clone, Pod, Zeroable)]
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

impl GalaxyUniform {
    pub fn from_params(
        params: &GalaxyParams,
        image_w: u32,
        image_h: u32,
        extent: f64,
        center_x: f64,
        center_y: f64,
        exposure: f32,
        contrast: f32,
    ) -> Self {
        Self {
            disk_scale_length: params.disk_scale_length as f32,
            disk_scale_height: params.disk_scale_height as f32,
            disk_central_density: params.disk_central_density as f32,
            arm_count: params.arm_count,
            arm_pitch: params.arm_pitch as f32,
            arm_concentration: params.arm_concentration as f32,
            arm_strength: params.arm_strength as f32,
            bulge_radius: params.bulge_radius as f32,
            bulge_central_density: params.bulge_central_density as f32,
            halo_radius: params.halo_radius as f32,
            halo_central_density: params.halo_central_density as f32,
            halo_slope: params.halo_slope as f32,
            image_width: image_w,
            image_height: image_h,
            extent: extent as f32,
            center_x: center_x as f32,
            center_y: center_y as f32,
            exposure,
            log_contrast: contrast,
        }
    }
}

/// Single-pass unified galaxy render.
///
/// No CPU readback — the `render_scene` compute shader handles everything
/// (density field, individual stars, tone mapping) in one dispatch and
/// writes the result directly into `target_texture` via `copy_buffer_to_texture`.
pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compute: &GpuCompute,
    rgba_buffer: &wgpu::Buffer,
    uniform_buffer: &wgpu::Buffer,
    image_w: u32,
    image_h: u32,
    target_texture: &wgpu::Texture,
) {
    assert!(
        image_w > 0 && image_w < 65536 && image_h > 0 && image_h < 65536,
        "image dimensions out of range"
    );

    let total = Instant::now();

    // ── uniforms (pre-written by caller) ────────────────────

    let scene_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene"),
        layout: &compute.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: rgba_buffer.as_entire_binding(),
            },
        ],
    });

    // ── dispatch ─────────────────────────────────────────────

    let thread_group_x = image_w.div_ceil(8);
    let thread_group_y = image_h.div_ceil(8);

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("scene"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&compute.pipeline);
        cpass.set_bind_group(0, &scene_bg, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }

    // copy rgba_buffer → target_texture
    let padded_w = image_w.div_ceil(64) * 64;
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: rgba_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * padded_w),
                rows_per_image: Some(image_h),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture: target_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: image_w,
            height: image_h,
            depth_or_array_layers: 1,
        },
    );

    queue.submit(Some(encoder.finish()));

    println!("frame time: {:.2?}", total.elapsed());
}

#[cfg(test)]
mod tests {
    // ── Host-side replicas of shader star-colour functions ──

    const T_SOLAR_HOST: f64 = 5770.0;

    fn host_mass_to_temp(m: f64) -> f64 {
        if m <= 0.08 {
            return 2300.0;
        }
        let (ref_m, ref_t, exp) = if m < 0.50 {
            (0.16, 3060.0, 0.53)
        } else if m < 1.0 {
            (0.88, 5240.0, 0.67)
        } else if m < 2.40 {
            (1.00, T_SOLAR_HOST, 0.57)
        } else if m < 7.0 {
            (2.40, 9700.0, 0.36)
        } else {
            (15.0, 30000.0, 0.20)
        };
        (ref_t * (m / ref_m).powf(exp)).min(50000.0)
    }

    const COLOR_LUT_HOST: [(f64, f64, f64, f64); 16] = [
        (2300.0, 1.000, 0.745, 0.424),
        (2600.0, 1.000, 0.765, 0.427),
        (3060.0, 1.000, 0.800, 0.435),
        (3400.0, 1.000, 0.808, 0.506),
        (3750.0, 1.000, 0.765, 0.545),
        (4400.0, 1.000, 0.847, 0.710),
        (5240.0, 1.000, 0.933, 0.867),
        (5770.0, 1.000, 0.961, 0.949),
        (6540.0, 0.973, 0.969, 1.000),
        (7220.0, 0.878, 0.898, 1.000),
        (8180.0, 0.792, 0.843, 1.000),
        (9700.0, 0.725, 0.788, 1.000),
        (15200.0, 0.667, 0.749, 1.000),
        (26500.0, 0.612, 0.698, 1.000),
        (41400.0, 0.608, 0.690, 1.000),
        (50000.0, 0.608, 0.690, 1.000),
    ];

    fn host_temperature_to_rgb(t_kelvin: f64) -> (f64, f64, f64) {
        let t = t_kelvin.clamp(COLOR_LUT_HOST[0].0, COLOR_LUT_HOST[15].0);
        if t <= COLOR_LUT_HOST[0].0 {
            return (
                COLOR_LUT_HOST[0].1,
                COLOR_LUT_HOST[0].2,
                COLOR_LUT_HOST[0].3,
            );
        }
        for i in 0..(COLOR_LUT_HOST.len() - 1) {
            if t <= COLOR_LUT_HOST[i + 1].0 {
                let t_lo = COLOR_LUT_HOST[i].0;
                let t_hi = COLOR_LUT_HOST[i + 1].0;
                let frac = (t - t_lo) / (t_hi - t_lo);
                let r =
                    COLOR_LUT_HOST[i].1 + frac * (COLOR_LUT_HOST[i + 1].1 - COLOR_LUT_HOST[i].1);
                let g =
                    COLOR_LUT_HOST[i].2 + frac * (COLOR_LUT_HOST[i + 1].2 - COLOR_LUT_HOST[i].2);
                let b =
                    COLOR_LUT_HOST[i].3 + frac * (COLOR_LUT_HOST[i + 1].3 - COLOR_LUT_HOST[i].3);
                return (r, g, b);
            }
        }
        let last = COLOR_LUT_HOST[15];
        (last.1, last.2, last.3)
    }
    use super::*;
    use crate::galaxy::GalaxyParams;

    #[test]
    fn from_params_preserves_values() {
        let params = GalaxyParams::milky_way();
        let uniform =
            GalaxyUniform::from_params(&params, 1920, 1080, 512_000.0, 0.0, 0.0, 0.60, 0.04);

        assert_eq!(uniform.disk_scale_length, params.disk_scale_length as f32);
        assert_eq!(uniform.disk_scale_height, params.disk_scale_height as f32);
        assert_eq!(
            uniform.disk_central_density,
            params.disk_central_density as f32
        );
        assert_eq!(uniform.arm_count, params.arm_count);
        assert_eq!(uniform.arm_pitch, params.arm_pitch as f32);
        assert_eq!(uniform.arm_concentration, params.arm_concentration as f32);
        assert_eq!(uniform.arm_strength, params.arm_strength as f32);
        assert_eq!(uniform.bulge_radius, params.bulge_radius as f32);
        assert_eq!(
            uniform.bulge_central_density,
            params.bulge_central_density as f32
        );
        assert_eq!(uniform.halo_radius, params.halo_radius as f32);
        assert_eq!(
            uniform.halo_central_density,
            params.halo_central_density as f32
        );
        assert_eq!(uniform.halo_slope, params.halo_slope as f32);
        assert_eq!(uniform.image_width, 1920);
        assert_eq!(uniform.image_height, 1080);
        assert_eq!(uniform.extent, 512_000.0_f32);
        assert_eq!(uniform.center_x, 0.0_f32);
        assert_eq!(uniform.center_y, 0.0_f32);
        assert_eq!(uniform.exposure, 0.60);
        assert_eq!(uniform.log_contrast, 0.04);
    }

    #[test]
    fn from_params_preserves_values_ngc628() {
        let params = GalaxyParams::ngc628();
        let uniform =
            GalaxyUniform::from_params(&params, 1920, 1080, 512_000.0, 0.0, 0.0, 0.60, 0.04);

        assert_eq!(uniform.disk_scale_length, params.disk_scale_length as f32);
        assert_eq!(uniform.disk_scale_height, params.disk_scale_height as f32);
        assert_eq!(
            uniform.disk_central_density,
            params.disk_central_density as f32
        );
        assert_eq!(uniform.arm_count, params.arm_count);
        assert_eq!(uniform.arm_pitch, params.arm_pitch as f32);
        assert_eq!(uniform.arm_concentration, params.arm_concentration as f32);
        assert_eq!(uniform.arm_strength, params.arm_strength as f32);
        assert_eq!(uniform.bulge_radius, params.bulge_radius as f32);
        assert_eq!(
            uniform.bulge_central_density,
            params.bulge_central_density as f32
        );
        assert_eq!(uniform.halo_radius, params.halo_radius as f32);
        assert_eq!(
            uniform.halo_central_density,
            params.halo_central_density as f32
        );
        assert_eq!(uniform.halo_slope, params.halo_slope as f32);
    }

    #[test]
    fn from_params_with_nonzero_center() {
        let params = GalaxyParams::milky_way();
        let uniform =
            GalaxyUniform::from_params(&params, 800, 600, 100_000.0, 5000.0, -2000.0, 0.50, 0.02);
        assert_eq!(uniform.center_x, 5000.0_f32);
        assert_eq!(uniform.center_y, -2000.0_f32);
        assert_eq!(uniform.extent, 100_000.0_f32);
        assert_eq!(uniform.image_width, 800);
        assert_eq!(uniform.image_height, 600);
        assert_eq!(uniform.exposure, 0.50);
        assert_eq!(uniform.log_contrast, 0.02);
    }

    #[test]
    fn uniform_is_pod() {
        // Verify bytemuck traits work
        let params = GalaxyParams::milky_way();
        let uniform = GalaxyUniform::from_params(&params, 100, 100, 1000.0, 0.0, 0.0, 0.5, 0.05);
        let _bytes: &[u8] = bytemuck::bytes_of(&uniform);
        // If this compiles and doesn't panic, Pod+Zeroable is satisfied
    }

    #[test]
    fn uniform_struct_size_matches_fields() {
        // Runtime size check as a cross-check against shader-side layout mismatch
        let expected = std::mem::size_of::<f32>() * 16 + std::mem::size_of::<u32>() * 3;
        assert_eq!(std::mem::size_of::<GalaxyUniform>(), expected);
    }

    // ── Host-side replicas of shader math for characterization ──

    fn host_disk_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
            return 0.0;
        }
        let radial = (-r / p.disk_scale_length).exp();
        2.0 * p.disk_scale_height * p.disk_central_density * radial
    }

    fn host_bulge_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.bulge_radius <= 0.0 {
            return 0.0;
        }
        let x = r / p.bulge_radius;
        (4.0 / 3.0) * p.bulge_radius * p.bulge_central_density * (1.0 + x * x).powf(-2.0)
    }

    fn host_halo_column(r: f64, p: &GalaxyParams) -> f64 {
        if p.halo_radius <= 0.0 || r < 1e-6 {
            return p.halo_radius * p.halo_central_density * std::f64::consts::PI;
        }
        let x = r / p.halo_radius;
        std::f64::consts::PI
            * p.halo_radius
            * p.halo_central_density
            * (1.0 + x).powf(p.halo_slope + 1.0)
    }

    #[test]
    fn disk_column_decreases_with_radius() {
        let p = GalaxyParams::milky_way();
        let d0 = host_disk_column(0.0, &p);
        let d1 = host_disk_column(p.disk_scale_length, &p);
        let d2 = host_disk_column(3.0 * p.disk_scale_length, &p);
        assert!(d0 > 0.0, "disk column at r=0 should be positive");
        assert!(d1 < d0, "disk should decrease with radius");
        assert!(d2 < d1, "disk should continue decreasing");
        // Exponential drop: d1/d0 ≈ 1/e
        let ratio = d1 / d0;
        assert!(
            (ratio - 1.0 / std::f64::consts::E).abs() < 0.01,
            "d(scale_length)/d(0) should be ~1/e, got {ratio}"
        );
    }

    #[test]
    fn bulge_column_has_plummer_profile() {
        let p = GalaxyParams::milky_way();
        let d0 = host_bulge_column(0.0, &p);
        let d_r = host_bulge_column(p.bulge_radius, &p);
        assert!(d0 > 0.0, "bulge column at center should be positive");
        // Plummer: Σ(R) ∝ (1 + R²/a²)^(-2). At R=a, factor is (1+1)^(-2) = 1/4
        let expected_ratio = 0.25;
        let actual_ratio = d_r / d0;
        assert!(
            (actual_ratio - expected_ratio).abs() < 0.01,
            "bulge at R=a should be ~1/4 of central, got {actual_ratio}"
        );
    }

    #[test]
    fn halo_column_is_positive_and_finite() {
        let p = GalaxyParams::milky_way();
        let values: Vec<f64> = [0.0, 1000.0, 10000.0, 50000.0, 100000.0]
            .iter()
            .map(|&r| host_halo_column(r, &p))
            .collect();
        for (r, v) in [0.0_f64, 1000.0, 10000.0, 50000.0, 100000.0]
            .iter()
            .zip(&values)
        {
            assert!(v.is_finite(), "halo_column({r}) = {v} is not finite");
            assert!(*v >= 0.0, "halo_column({r}) = {v} is negative");
        }
        // Halo should decrease with radius
        assert!(values[0] >= values[1], "halo should decrease with radius");
        assert!(values[1] > values[3], "halo should continue decreasing");
    }

    // ── NGC 628 column profile checks ───────────────────

    #[test]
    fn ngc628_disk_column_profile() {
        let p = GalaxyParams::ngc628();
        let d0 = host_disk_column(0.0, &p);
        let d1 = host_disk_column(p.disk_scale_length, &p);
        let d2 = host_disk_column(3.0 * p.disk_scale_length, &p);
        assert!(d0 > 0.0, "ngc628 disk column at r=0 should be positive");
        assert!(d1 < d0, "ngc628 disk should decrease with radius");
        assert!(d2 < d1, "ngc628 disk should continue decreasing");
        // Exponential drop: d1/d0 ≈ 1/e
        let ratio = d1 / d0;
        assert!(
            (ratio - 1.0 / std::f64::consts::E).abs() < 0.01,
            "ngc628 d(scale_length)/d(0) should be ~1/e, got {ratio}"
        );
    }

    #[test]
    fn ngc628_bulge_is_compact_relative_to_disk() {
        let p = GalaxyParams::ngc628();
        // The bulge is compact (a = 1,871 ly) vs. the extended disk
        // (hr = 10,630 ly).  Even though the bulge is only 6.5 % of
        // total stellar mass, its central surface density exceeds the
        // disk's.  This is correct for a Plummer bulge.
        // Σ_bulge(0) / Σ_disk(0) ≈ (M_b/M_d) × 2(hr/a)² ≈ 4.5.
        let bulge_c0 = host_bulge_column(0.0, &p);
        let disk_c0 = host_disk_column(0.0, &p);
        let ratio = bulge_c0 / disk_c0;
        assert!(
            ratio > 2.0,
            "ngc628 bulge/disk column ratio {ratio:.3} should be > 2"
        );
        assert!(
            ratio < 10.0,
            "ngc628 bulge/disk column ratio {ratio:.3} should be < 10"
        );
        // But at the disk scale length, the disk should dominate.
        let bulge_c1 = host_bulge_column(p.disk_scale_length, &p);
        let disk_c1 = host_disk_column(p.disk_scale_length, &p);
        assert!(
            disk_c1 > bulge_c1,
            "ngc628 at r=hr: disk column {disk_c1:.3} should exceed bulge {bulge_c1:.3}"
        );
    }

    #[test]
    fn ngc628_bulge_column_plummer_drop() {
        let p = GalaxyParams::ngc628();
        let d0 = host_bulge_column(0.0, &p);
        let d_a = host_bulge_column(p.bulge_radius, &p);
        assert!(d0 > 0.0, "ngc628 bulge column at center should be positive");
        // Plummer: Σ(R) ∝ (1 + R²/a²)^(-2). At R=a, factor is 1/4.
        let ratio = d_a / d0;
        assert!(
            (ratio - 0.25).abs() < 0.01,
            "ngc628 bulge at R=a should be ~1/4 of central, got {ratio}"
        );
    }

    #[test]
    fn ngc628_disk_scale_length_matches_s4g() {
        // The S4G disk scale length is 69.3 arcsec at 9.7 Mpc.
        // 1 arcsec = 153.38 ly, so hr = 69.3 × 153.38 = 10,629 ly.
        let p = GalaxyParams::ngc628();
        let expected = 10_630.0;
        assert!(
            (p.disk_scale_length - expected).abs() < 10.0,
            "ngc628 disk_scale_length {} differs from expected {expected}",
            p.disk_scale_length
        );
    }

    #[test]
    fn zero_params_produce_zero_density() {
        let zero_params = GalaxyParams {
            disk_scale_length: 0.0,
            disk_scale_height: 0.0,
            disk_central_density: 0.0,
            arm_count: 0,
            arm_pitch: 0.0,
            arm_concentration: 0.0,
            arm_strength: 0.0,
            bulge_radius: 0.0,
            bulge_central_density: 0.0,
            halo_radius: 0.0,
            halo_central_density: 0.0,
            halo_slope: -3.0,
        };
        assert_eq!(host_disk_column(1000.0, &zero_params), 0.0);
        assert_eq!(host_bulge_column(1000.0, &zero_params), 0.0);
        // halo_radius=0 returns the center value regardless of r
    }

    fn host_arm_modulation_2d(x: f32, z: f32, p: &GalaxyParams) -> f32 {
        let hr = p.disk_scale_length as f32;
        let r = (x * x + z * z).sqrt();
        if p.arm_count == 0 || p.arm_strength <= 0.0 || hr <= 0.0 {
            return 1.0;
        }
        let theta = x.atan2(z);
        // Logarithmic spiral: θ = cot(φ) × ln(1 + r/hr)
        let cot_phi = 1.0 / (p.arm_pitch as f32).tan();
        let log_spiral = theta - cot_phi * (1.0 + r / hr).ln();
        let arm_width = 1.0 / p.arm_concentration as f32;
        let mut min_dtheta = std::f32::consts::PI;
        for k in 0..p.arm_count {
            let phase = log_spiral + std::f32::consts::TAU * (k as f32) / (p.arm_count as f32);
            let dtheta = phase % std::f32::consts::TAU;
            let dtheta = if dtheta > std::f32::consts::PI {
                dtheta - std::f32::consts::TAU
            } else {
                dtheta
            };
            min_dtheta = min_dtheta.min(dtheta.abs());
        }
        let arg = min_dtheta / arm_width;
        1.0 + p.arm_strength as f32 * (-0.5 * arg * arg).exp()
    }

    #[test]
    fn arm_modulation_no_arms_returns_one() {
        let mut p = GalaxyParams::milky_way();
        p.arm_count = 0;
        assert!((host_arm_modulation_2d(1000.0, 500.0, &p) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn arm_modulation_zero_strength_returns_one() {
        let mut p = GalaxyParams::milky_way();
        p.arm_strength = 0.0;
        assert!((host_arm_modulation_2d(5000.0, 3000.0, &p) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn arm_modulation_is_positive() {
        for (label, p) in [
            ("MW", GalaxyParams::milky_way()),
            ("NGC628", GalaxyParams::ngc628()),
        ] {
            for (r, theta) in &[
                (1000.0_f32, 0.0_f32),
                (5000.0, 0.5),
                (15000.0, 1.2),
                (25000.0, 2.0),
            ] {
                let x = r * theta.sin();
                let z = r * theta.cos();
                let m = host_arm_modulation_2d(x, z, &p);
                assert!(
                    m.is_finite() && m > 0.0,
                    "{label} modulation at r={r}, θ={theta} = {m} not positive+finite"
                );
            }
        }
    }

    #[test]
    fn arm_modulation_enhances_along_arms() {
        let mut p = GalaxyParams::milky_way();
        p.arm_strength = 1.0;
        let r = 0.5 * p.disk_scale_length as f32;
        let cot_phi = 1.0 / (p.arm_pitch as f32).tan();
        let theta_on_arm = cot_phi * (1.0 + r / p.disk_scale_length as f32).ln();
        let x = r * theta_on_arm.sin();
        let z = r * theta_on_arm.cos();
        let m = host_arm_modulation_2d(x, z, &p);
        assert!(
            (m - 2.0).abs() < 0.01,
            "on-arm modulation = {m}, expected ~2.0"
        );
    }

    #[test]
    fn arm_modulation_periodic_across_arms() {
        let p = GalaxyParams::milky_way();
        let r = 8000.0f32;
        let theta0 = 1.0f32;
        let spacing = std::f32::consts::TAU / p.arm_count as f32;
        let x0 = r * theta0.sin();
        let z0 = r * theta0.cos();
        let x1 = r * (theta0 + spacing).sin();
        let z1 = r * (theta0 + spacing).cos();
        let m0 = host_arm_modulation_2d(x0, z0, &p);
        let m1 = host_arm_modulation_2d(x1, z1, &p);
        assert!(
            (m0 - m1).abs() < 1e-5,
            "modulation at θ={theta0} = {m0}, at θ={} = {m1}; should match",
            theta0 + spacing
        );
    }

    #[test]
    fn arm_modulation_zero_pitch_does_not_nan() {
        // pitch=0 causes cot(0) = inf in both host and shader,
        // which could produce NaN.  Verify graceful handling.
        let mut p = GalaxyParams::milky_way();
        p.arm_pitch = 0.0;
        let m = host_arm_modulation_2d(5000.0, 3000.0, &p);
        // The function should still return a finite, positive value
        // (arm modulation still works with pitch=0 meaning radial arms).
        // At minimum it must not NaN or inf.
        assert!(m.is_finite(), "pitch=0 modulation = {m} is not finite");
        assert!(m > 0.0, "pitch=0 modulation = {m} should be positive");
    }

    #[test]
    fn arm_modulation_zero_concentration_returns_max() {
        // concentration=0 → arm_width = inf → arg = 0 → exp(-0) = 1 → 1 + strength
        let mut p = GalaxyParams::milky_way();
        p.arm_concentration = 0.0;
        let m = host_arm_modulation_2d(5000.0, 3000.0, &p);
        assert!(m.is_finite(), "conc=0 modulation = {m} is not finite");
        assert!(
            (m - (1.0 + p.arm_strength as f32)).abs() < 1e-6,
            "conc=0 modulation = {m}, expected {}",
            1.0 + p.arm_strength as f32
        );
    }

    #[test]
    fn arm_modulation_quadrant_coverage() {
        // Test all four quadrants for correct atan2 handling.
        // Coordinates are placed at the same radius and relative angle
        // in each quadrant.
        let p = GalaxyParams::milky_way();
        let r = 10000.0f32;
        let test_positions = [
            ("Q1 (+x,+z)", r, r),
            ("Q2 (-x,+z)", -r, r),
            ("Q3 (-x,-z)", -r, -r),
            ("Q4 (+x,-z)", r, -r),
        ];
        for (label, x, z) in &test_positions {
            let m = host_arm_modulation_2d(*x, *z, &p);
            assert!(
                m.is_finite() && m > 0.0,
                "{label} modulation = {m} not positive+finite"
            );
        }
    }

    #[test]
    fn galaxy_uniform_field_offsets_match_shader_layout() {
        // These offsets MUST match the shader-side GalaxyUniform layout.
        // The shader uses spirv-std glam types (f32 = 4 bytes, u32 = 4 bytes)
        // with #[repr(C)] packing.
        use std::mem::{offset_of, size_of};

        assert_eq!(
            size_of::<GalaxyUniform>(),
            76,
            "overall size mismatch with shader"
        );

        assert_eq!(offset_of!(GalaxyUniform, disk_scale_length), 0);
        assert_eq!(offset_of!(GalaxyUniform, disk_scale_height), 4);
        assert_eq!(offset_of!(GalaxyUniform, disk_central_density), 8);
        assert_eq!(offset_of!(GalaxyUniform, arm_count), 12);
        assert_eq!(offset_of!(GalaxyUniform, arm_pitch), 16);
        assert_eq!(offset_of!(GalaxyUniform, arm_concentration), 20);
        assert_eq!(offset_of!(GalaxyUniform, arm_strength), 24);
        assert_eq!(offset_of!(GalaxyUniform, bulge_radius), 28);
        assert_eq!(offset_of!(GalaxyUniform, bulge_central_density), 32);
        assert_eq!(offset_of!(GalaxyUniform, halo_radius), 36);
        assert_eq!(offset_of!(GalaxyUniform, halo_central_density), 40);
        assert_eq!(offset_of!(GalaxyUniform, halo_slope), 44);
        assert_eq!(offset_of!(GalaxyUniform, image_width), 48);
        assert_eq!(offset_of!(GalaxyUniform, image_height), 52);
        assert_eq!(offset_of!(GalaxyUniform, extent), 56);
        assert_eq!(offset_of!(GalaxyUniform, center_x), 60);
        assert_eq!(offset_of!(GalaxyUniform, center_y), 64);
        assert_eq!(offset_of!(GalaxyUniform, exposure), 68);
        assert_eq!(offset_of!(GalaxyUniform, log_contrast), 72);
    }

    // ── New star colour tests ─────────────────────────

    #[test]
    fn mass_to_temp_produces_correct_teff() {
        // Empirical reference values from Pecaut & Mamajek (2013), Eker et al. (2018).
        // The piecewise power-law fit is approximate; tolerance scales with Teff.
        let cases: &[(f64, f64, f64)] = &[
            (0.08, 2300.0, 1.0), // clamped
            (0.16, 3060.0, 1.0), // exact anchor
            (0.50, 3750.0, 500.0),
            (0.88, 5240.0, 1.0), // exact anchor
            (1.00, 5770.0, 1.0), // exact anchor (solar segment)
            (2.00, 8180.0, 500.0),
            (2.40, 9700.0, 1.0),     // exact anchor
            (7.00, 22000.0, 4000.0), // discontinuity at breakpoint
            (15.0, 30000.0, 1.0),    // exact anchor
        ];
        for &(mass, expected_teff, tol) in cases {
            let teff = host_mass_to_temp(mass);
            assert!(
                (teff - expected_teff).abs() < tol,
                "mass_to_temp({mass}) = {teff}, expected ~{expected_teff} (tol {tol})"
            );
        }
    }

    #[test]
    fn mass_to_temp_monotonic() {
        let mut prev = host_mass_to_temp(0.05);
        for mass in [
            0.08_f64, 0.1, 0.2, 0.5, 0.8, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0,
        ] {
            let t = host_mass_to_temp(mass);
            assert!(t >= prev, "mass_to_temp({mass}) = {t} < prev {prev}");
            prev = t;
        }
    }

    #[test]
    fn temperature_to_rgb_sun_is_white() {
        let (r, g, b) = host_temperature_to_rgb(5770.0);
        assert!((r - 1.0).abs() < 0.01);
        assert!(g > 0.95 && g < 1.0, "G={g} should be ~0.961");
        assert!(b > 0.93 && b < 0.97, "B={b} should be ~0.949");
    }

    #[test]
    fn temperature_to_rgb_no_green_stars() {
        for t_k in [
            2500.0_f64, 3500.0, 4500.0, 5770.0, 7000.0, 8200.0, 10000.0, 15000.0,
        ] {
            let (r, g, b) = host_temperature_to_rgb(t_k);
            if t_k < 6200.0 {
                // Cool stars are R-dominant (orange/white, never green)
                assert!(
                    r >= g && g >= b,
                    "at {t_k}K: R={r:.3} G={g:.3} B={b:.3} — expected R≥G≥B"
                );
            } else {
                // Hot stars are B-dominant (blue/white, never green)
                assert!(
                    b >= g && g >= r,
                    "at {t_k}K: R={r:.3} G={g:.3} B={b:.3} — expected B≥G≥R"
                );
            }
        }
    }

    #[test]
    fn no_channel_is_ever_the_maximum_alone() {
        // Green should never be the sole max channel (no green stars)
        for t_k in [
            2300_f64, 3060., 3750., 4400., 5240., 5770., 6540., 7220., 8180., 9700., 15200.,
            26500., 41400., 50000.,
        ] {
            let (r, g, b) = host_temperature_to_rgb(t_k);
            assert!(
                !(g > r && g > b),
                "green is max at {t_k}K: R={r:.3} G={g:.3} B={b:.3}"
            );
        }
    }

    #[test]
    fn temperature_to_rgb_m_dwarfs_are_orange() {
        let (r, g, b) = host_temperature_to_rgb(3750.0);
        assert!((r - 1.0).abs() < 0.01, "M dwarf R={r} should be 1.0");
        assert!(g > 0.70 && g < 0.85, "M dwarf G={g} should be ~0.765");
        assert!(b > 0.45 && b < 0.65, "M dwarf B={b} should be ~0.545");
        assert!(g > 0.5, "M dwarf G={g} > 0.5 (orange, not red)");
        assert!(r > g && g > b, "M dwarf: R > G > B (orange, not red)");
    }

    #[test]
    fn temperature_to_rgb_o_stars_are_blue() {
        let (r, g, b) = host_temperature_to_rgb(41400.0);
        assert!(b > 0.99, "O star B should be ~1.0");
        assert!(r > 0.55 && r < 0.70, "O star R={r} should be ~0.61");
        assert!(g > 0.60 && g < 0.75, "O star G={g} should be ~0.69");
        assert!(b > g && g > r, "O star: expected B > G > R");
    }

    #[test]
    fn temperature_to_rgb_monotonic_channels() {
        let mut prev_r = 2.0;
        let mut prev_b = -1.0;
        for t in [
            2300_f64, 3060., 3750., 4400., 5240., 5770., 6540., 7220., 8180., 9700., 15200., 26500.,
        ] {
            let (r, _g, b) = host_temperature_to_rgb(t);
            assert!(r <= prev_r + 0.001, "R({t}) = {r} > prev {prev_r}");
            assert!(b >= prev_b - 0.001, "B({t}) = {b} < prev {prev_b}");
            prev_r = r;
            prev_b = b;
        }
    }

    #[test]
    fn cell_star_light_with_new_teff_gives_plausible_colors() {
        let test_masses = [0.1_f64, 0.3, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0];
        for &mass in &test_masses {
            let teff = host_mass_to_temp(mass);
            let (r, g, b) = host_temperature_to_rgb(teff);
            assert!((0.0..=1.0).contains(&r), "mass={mass}: R={r} out of range");
            assert!((0.0..=1.0).contains(&g), "mass={mass}: G={g} out of range");
            assert!((0.0..=1.0).contains(&b), "mass={mass}: B={b} out of range");
            let max_ch = r.max(g).max(b);
            assert!(
                (max_ch - 1.0).abs() < 0.01,
                "mass={mass}: no channel near 1.0 (R={r}, G={g}, B={b})"
            );
        }
    }

    #[test]
    fn lut_entries_are_sorted_by_temperature() {
        for i in 0..(COLOR_LUT_HOST.len() - 1) {
            assert!(
                COLOR_LUT_HOST[i].0 < COLOR_LUT_HOST[i + 1].0,
                "LUT entry {i} Teff={} >= entry {} Teff={}",
                COLOR_LUT_HOST[i].0,
                i + 1,
                COLOR_LUT_HOST[i + 1].0
            );
        }
    }
}
