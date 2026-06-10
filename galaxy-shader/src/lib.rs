#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::glam::{UVec3, Vec3};
use spirv_std::num_traits::real::Real;
use spirv_std::spirv;

#[derive(Copy, Clone)]
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

const PI: f32 = 3.141_592_7;
const TAU: f32 = 6.283_185_5;

fn rem_euclid(x: f32, y: f32) -> f32 {
    let r = x % y;
    if r < 0.0 { r + y } else { r }
}

fn density(pos: Vec3, p: &GalaxyUniform) -> f32 {
    let r = (pos.x * pos.x + pos.z * pos.z).sqrt();
    let z = pos.y.abs();

    disk_density(r, z, p) * arm_modulation(pos, r, p)
        + bulge_density(pos.length(), p)
        + halo_density(pos.length(), p)
}

fn disk_density(r: f32, z: f32, p: &GalaxyUniform) -> f32 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    let zeta = z / p.disk_scale_height;
    let sech = 1.0 / zeta.cosh();
    p.disk_central_density * radial * sech * sech
}

fn arm_modulation(pos: Vec3, r: f32, p: &GalaxyUniform) -> f32 {
    if p.arm_count == 0 || p.arm_strength <= 0.0 {
        return 1.0;
    }
    let theta = pos.x.atan2(pos.z);
    let log_spiral = theta - (r / p.disk_scale_length) * p.arm_pitch;

    let arm_width = 1.0 / p.arm_concentration;
    let mut min_dtheta = PI;

    for k in 0..p.arm_count {
        let phase = log_spiral + TAU * (k as f32) / (p.arm_count as f32);
        let dtheta = rem_euclid(phase, TAU);
        let dtheta = if dtheta > PI { dtheta - TAU } else { dtheta };
        min_dtheta = min_dtheta.min(dtheta.abs());
    }

    let arg = min_dtheta / arm_width;
    1.0 + p.arm_strength * (-0.5 * arg * arg).exp()
}

fn bulge_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = dist / p.bulge_radius;
    p.bulge_central_density * (1.0 + x * x).powf(-2.5)
}

fn halo_density(dist: f32, p: &GalaxyUniform) -> f32 {
    if p.halo_radius <= 0.0 || dist < 1e-6 {
        return p.halo_central_density;
    }
    let x = dist / p.halo_radius;
    p.halo_central_density * (1.0 + x).powf(p.halo_slope)
}

// ── Compute entry point ────────────────────────────────────────

/// One thread per pixel.  Thread group size 8×8 = 64 pixels per
/// workgroup keeps occupancy high without wasting too many threads
/// on out-of-bounds pixels near the right / bottom edge.
#[spirv(compute(threads(8, 8, 1)))]
pub fn render_density(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &GalaxyUniform,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] output: &mut [f32],
) {
    let x = id.x;
    let y = id.y;
    if x >= params.image_width || y >= params.image_height {
        return;
    }

    let wx = (x as f32 / params.image_width as f32 - 0.5) * params.extent;
    let wz = -(y as f32 / params.image_height as f32 - 0.5) * params.extent;
    let pos = Vec3::new(wx, 0.0, wz);

    let d = density(pos, params);
    let idx = (y * params.image_width + x) as usize;
    output[idx] = d;
}
