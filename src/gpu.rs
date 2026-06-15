use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

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
    fn from_params(
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
    params: &GalaxyParams,
    image_w: u32,
    image_h: u32,
    galaxy_extent_ly: f64,
    center_x_ly: f64,
    center_y_ly: f64,
    exposure: f32,
    contrast: f32,
    target_texture: &wgpu::Texture,
) {
    assert!(
        image_w > 0 && image_w < 65536 && image_h > 0 && image_h < 65536,
        "image dimensions out of range"
    );

    let total = Instant::now();

    // ── uniforms ─────────────────────────────────────────────

    let uniform_data = GalaxyUniform::from_params(
        params,
        image_w,
        image_h,
        galaxy_extent_ly,
        center_x_ly,
        center_y_ly,
        exposure,
        contrast,
    );
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&uniform_data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

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

    println!(
        "GPU render {image_w}×{image_h} ±{:.0} kly  exp={exposure:.3} con={contrast:.4}",
        galaxy_extent_ly / 1_000.0 / 2.0,
    );

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

    println!("  total: {:.2?}", total.elapsed());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::GalaxyParams;

    #[test]
    fn from_params_preserves_values() {
        let params = GalaxyParams::milky_way();
        let uniform =
            GalaxyUniform::from_params(&params, 1920, 1080, 512_000.0, 0.0, 0.0, 0.60, 0.04);
        // Field-by-field assertions to catch field-order bugs
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
}
