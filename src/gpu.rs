use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;
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
}

impl GalaxyUniform {
    fn from_params(params: &GalaxyParams, image_size: u32, extent: f64) -> Self {
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
            image_width: image_size,
            image_height: image_size,
            extent: extent as f32,
        }
    }
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct NormUniform {
    min_log: f32,
    inv_range: f32,
    image_width: u32,
    image_height: u32,
}

pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    params: &GalaxyParams,
    image_size: u32,
    galaxy_extent_ly: f64,
    output_path: Option<&str>,
) -> Vec<u8> {
    assert!(
        image_size > 0 && image_size < 65536,
        "image_size out of range"
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

    let uniform_data = GalaxyUniform::from_params(params, image_size, galaxy_extent_ly);
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&uniform_data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let pixel_count = (image_size * image_size) as usize;
    let f32_byte_size = (pixel_count * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
    let u32_byte_size = (pixel_count * std::mem::size_of::<u32>()) as wgpu::BufferAddress;

    // ── buffers ──────────────────────────────────────────────
    // GPU-only writes — never mapped
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

    // CPU-readback buffers — COPY_DST only, paired with MAP_READ
    let density_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("density-readback"),
        size: f32_byte_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let rgba_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rgba-readback"),
        size: u32_byte_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // ── density pipeline ─────────────────────────────────────

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

    let density_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("density"),
        bind_group_layouts: &[&density_bgl],
        push_constant_ranges: &[],
    });

    let density_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("galaxy-density"),
        layout: Some(&density_pipeline_layout),
        module: &module,
        entry_point: Some("render_density"),
        compilation_options: Default::default(),
        cache: None,
    });

    // ── normalize pipeline ───────────────────────────────────

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

    let norm_uniform = device.create_buffer(&wgpu::BufferDescriptor {
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
                resource: norm_uniform.as_entire_binding(),
            },
        ],
    });

    let norm_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("normalize"),
        bind_group_layouts: &[&norm_bgl],
        push_constant_ranges: &[],
    });

    let norm_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("galaxy-normalize"),
        layout: Some(&norm_pipeline_layout),
        module: &module,
        entry_point: Some("normalize_rgba"),
        compilation_options: Default::default(),
        cache: None,
    });

    // ── dispatch density pass ────────────────────────────────

    let thread_group_x = image_size.div_ceil(8);
    let thread_group_y = image_size.div_ceil(8);

    println!(
        "GPU render {image_size}×{image_size} ±{:.0} kly | dispatching {thread_group_x}×{thread_group_y} groups",
        galaxy_extent_ly / 1_000.0 / 2.0,
    );

    let gpu_start = Instant::now();

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("density"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&density_pipeline);
        cpass.set_bind_group(0, &density_bg, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }
    encoder.copy_buffer_to_buffer(&density_buffer, 0, &density_readback, 0, f32_byte_size);
    queue.submit(Some(encoder.finish()));

    // ── CPU: read back density, find log min/max ─────────────

    let density_slice = density_readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    density_slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).ok();
    });
    device.poll(wgpu::Maintain::wait()).panic_on_timeout();
    rx.recv().unwrap().expect("density readback failed");

    let density_elapsed = gpu_start.elapsed();
    let cpu_start = Instant::now();

    let data = density_slice.get_mapped_range();
    let densities: &[f32] = bytemuck::cast_slice(&data);

    let (min_log, max_log) = densities
        .par_iter()
        .with_min_len(65536)
        .map(|&d| ((d.max(0.0) as f64) + 1e-12).ln())
        .fold(
            || (f64::INFINITY, f64::NEG_INFINITY),
            |(min, max), v| (min.min(v), max.max(v)),
        )
        .reduce(
            || (f64::INFINITY, f64::NEG_INFINITY),
            |(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)),
        );

    let density_min_raw = densities.iter().copied().fold(f32::INFINITY, f32::min);
    let density_max_raw = densities.iter().copied().fold(0.0f32, f32::max);

    drop(data);
    density_readback.unmap();

    let cpu_elapsed = cpu_start.elapsed();

    // ── dispatch normalize pass ──────────────────────────────

    let log_range = (max_log - min_log) as f32;
    let norm_data = NormUniform {
        min_log: min_log as f32,
        inv_range: if log_range > 1e-6 {
            1.0 / log_range
        } else {
            0.0
        },
        image_width: image_size,
        image_height: image_size,
    };
    queue.write_buffer(&norm_uniform, 0, bytemuck::bytes_of(&norm_data));

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("normalize"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&norm_pipeline);
        cpass.set_bind_group(0, &norm_bg, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }
    encoder.copy_buffer_to_buffer(&rgba_buffer, 0, &rgba_readback, 0, u32_byte_size);
    queue.submit(Some(encoder.finish()));

    // ── read back RGBA ───────────────────────────────────────

    let rgba_slice = rgba_readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    rgba_slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).ok();
    });
    device.poll(wgpu::Maintain::wait()).panic_on_timeout();
    rx.recv().unwrap().expect("RGBA readback failed");

    let gpu_total = gpu_start.elapsed();

    let data = rgba_slice.get_mapped_range();
    let rgba_u32: &[u32] = bytemuck::cast_slice(&data);
    let rgba: Vec<u8> = rgba_u32.iter().flat_map(|&p| p.to_ne_bytes()).collect();
    drop(data);
    rgba_readback.unmap();

    println!(
        "  density range: {density_min_raw:.2e} – {density_max_raw:.2e}   log range: {min_log:.1} – {max_log:.1}",
    );
    println!(
        "  density pass: {density:.2?}   CPU min/max: {cpu:.2?}   normalize+readback: {norm:.2?}   total: {total:.2?}",
        density = density_elapsed,
        cpu = cpu_elapsed,
        norm = gpu_total - density_elapsed - cpu_elapsed,
        total = total.elapsed(),
    );

    if let Some(path) = output_path {
        let luma: Vec<u8> = rgba.iter().step_by(4).copied().collect();
        image::save_buffer(path, &luma, image_size, image_size, image::ColorType::L8)
            .expect("Failed to write PNG");
        println!("  wrote {path}");
    }

    rgba
}
