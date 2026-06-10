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

pub fn compute_galaxy(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    params: &GalaxyParams,
    image_size: u32,
    galaxy_extent_ly: f64,
    output_path: Option<&str>,
) -> Vec<u8> {
    assert!(image_size > 0);

    let total = Instant::now();

    let spirv_bytes = include_bytes!(env!("galaxy_shader.spv"));
    let spirv_words: Vec<u32> = spirv_bytes
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("galaxy-density"),
        source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Owned(spirv_words)),
    });

    let uniform_data = GalaxyUniform::from_params(params, image_size, galaxy_extent_ly);
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&uniform_data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let pixel_count = (image_size * image_size) as usize;
    let output_byte_size = (pixel_count * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

    let storage_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("density-output"),
        size: output_byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: output_byte_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: storage_buffer.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("galaxy-density"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("render_density"),
        compilation_options: Default::default(),
        cache: None,
    });

    let thread_group_x = image_size.div_ceil(8);
    let thread_group_y = image_size.div_ceil(8);

    println!(
        "GPU render {image_size}×{image_size} ±{:.0} kly | dispatching {thread_group_x}×{thread_group_y} groups",
        galaxy_extent_ly / 1_000.0 / 2.0,
    );

    let compute_start = Instant::now();

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        cpass.set_pipeline(&compute_pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        cpass.dispatch_workgroups(thread_group_x, thread_group_y, 1);
    }
    encoder.copy_buffer_to_buffer(&storage_buffer, 0, &readback_buffer, 0, output_byte_size);
    queue.submit(Some(encoder.finish()));

    let buffer_slice = readback_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).ok();
    });
    device.poll(wgpu::Maintain::wait()).panic_on_timeout();
    rx.recv().unwrap().expect("GPU readback failed");

    let compute_elapsed = compute_start.elapsed();

    let data = buffer_slice.get_mapped_range();
    let densities: &[f32] = bytemuck::cast_slice(&data);
    let densities = densities.to_vec();
    drop(data);
    readback_buffer.unmap();

    let eps = 1e-12f64;
    let log_densities: Vec<f64> = densities.iter().map(|&d| (d as f64 + eps).ln()).collect();

    let min_log = log_densities.iter().copied().fold(f64::INFINITY, f64::min);
    let max_log = log_densities
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let log_range = max_log - min_log;

    println!(
        "  density range: {:.2e} – {:.2e}   log range: {:.1} – {:.1}  ({:.2?} GPU)",
        densities.iter().copied().fold(f32::INFINITY, f32::min),
        densities.iter().copied().fold(0.0f32, f32::max),
        min_log,
        max_log,
        compute_elapsed,
    );

    let brightness: Vec<u8> = log_densities
        .iter()
        .map(|&ld| {
            let b = if log_range > 1e-6 {
                (ld - min_log) / log_range
            } else {
                0.5
            };
            (b * 255.0) as u8
        })
        .collect();

    if let Some(path) = output_path {
        image::save_buffer(
            path,
            &brightness,
            image_size,
            image_size,
            image::ColorType::L8,
        )
        .expect("Failed to write PNG");
        println!("  wrote {path} ({:.2?} total)", total.elapsed());
    }

    brightness.iter().flat_map(|&b| [b, b, b, 255u8]).collect()
}
