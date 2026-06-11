use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::galaxy::GalaxyParams;

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
}

impl GalaxyUniform {
    fn from_params(
        params: &GalaxyParams,
        image_w: u32,
        image_h: u32,
        extent: f64,
        center_x: f64,
        center_y: f64,
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
        }
    }
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct NormUniform {
    exposure: f32,
    contrast: f32,
    image_width: u32,
    image_height: u32,
}

/// Render the galaxy entirely on the GPU and write the result into `target_texture`.
///
/// No CPU readback — density stays on the GPU and the normalised RGBA output is
/// copied directly into the texture via `copy_buffer_to_texture`.
pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
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

    let uniform_data = GalaxyUniform::from_params(
        params,
        image_w,
        image_h,
        galaxy_extent_ly,
        center_x_ly,
        center_y_ly,
    );
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&uniform_data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let pixel_count = (image_w * image_h) as usize;
    let f32_byte_size = (pixel_count * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
    let u32_byte_size = (pixel_count * std::mem::size_of::<u32>()) as wgpu::BufferAddress;

    // ── buffers (GPU-only, never mapped) ────────────────────

    let density_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("density"),
        size: f32_byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let rgba_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rgba"),
        size: u32_byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // ── density pipeline ────────────────────────────────────

    let density_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("density"),
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

    let density_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("density"),
        layout: &density_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: density_buffer.as_entire_binding(),
            },
        ],
    });

    let density_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("density"),
        bind_group_layouts: &[&density_bgl],
        push_constant_ranges: &[],
    });

    let density_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("galaxy-density"),
        layout: Some(&density_pl),
        module: &module,
        entry_point: Some("render_density"),
        compilation_options: Default::default(),
        cache: None,
    });

    // ── normalize pipeline ──────────────────────────────────

    let norm_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("normalize"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let norm_ub = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("norm-uniforms"),
        size: std::mem::size_of::<NormUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let norm_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("normalize"),
        layout: &norm_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: density_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: rgba_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: norm_ub.as_entire_binding(),
            },
        ],
    });

    let norm_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("normalize"),
        bind_group_layouts: &[&norm_bgl],
        push_constant_ranges: &[],
    });

    let norm_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("galaxy-normalize"),
        layout: Some(&norm_pl),
        module: &module,
        entry_point: Some("normalize_rgba"),
        compilation_options: Default::default(),
        cache: None,
    });

    // ── upload normalize uniforms ───────────────────────────

    let norm_data = NormUniform {
        exposure,
        contrast,
        image_width: image_w,
        image_height: image_h,
    };
    queue.write_buffer(&norm_ub, 0, bytemuck::bytes_of(&norm_data));

    // ── single encoder: density → normalize → copy-to-texture

    let thread_group_x = image_w.div_ceil(8);
    let thread_group_y = image_h.div_ceil(8);

    println!(
        "GPU render {image_w}×{image_h} ±{:.0} kly  exp={exposure:.3} con={contrast:.4}",
        galaxy_extent_ly / 1_000.0 / 2.0,
    );

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    // density pass
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("density"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&density_pipeline);
        cpass.set_bind_group(0, &density_bg, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }

    // normalize pass (reads density_buffer, writes rgba_buffer)
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("normalize"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&norm_pipeline);
        cpass.set_bind_group(0, &norm_bg, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }

    // copy rgba_buffer → target_texture (all on GPU)
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &rgba_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image_w),
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
