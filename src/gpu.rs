use bytemuck::{Pod, Zeroable};

use crate::galaxy::GalaxyParams;

// ═══════════════════════════════════════════════════════════
//  GPU-instanced star rendering
// ═══════════════════════════════════════════════════════════

/// Maximum number of stars in the catalogue buffer.
pub const MAX_STARS: u32 = 65536 * 8;

/// Coarse cell size for catalogue scan (light-years).
pub const CATALOGUE_CELL_SIZE: f32 = 50.0;

/// Minimum stellar mass to include in the catalogue (M☉).
pub const CATALOGUE_MASS_THRESHOLD: f32 = 0.8;

/// A single star in the instance buffer.
///
/// Layout (28 bytes, must match `stars.wgsl`):
///   pos_x, pos_y, pos_z, mass, temp, lum, _pad
#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct StarInstance {
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub mass: f32,
    pub temp: f32,
    pub lum: f32,
    pub _pad: u32,
}

/// f64 host-side disk column density.
fn disk_column_host(r: f64, p: &GalaxyParams) -> f64 {
    if p.disk_scale_length <= 0.0 || p.disk_scale_height <= 0.0 {
        return 0.0;
    }
    let radial = (-r / p.disk_scale_length).exp();
    2.0 * p.disk_scale_height * p.disk_central_density * radial
}

/// f64 host-side bulge column density.
fn bulge_column_host(r: f64, p: &GalaxyParams) -> f64 {
    if p.bulge_radius <= 0.0 {
        return 0.0;
    }
    let x = r / p.bulge_radius;
    (4.0 / 3.0) * p.bulge_radius * p.bulge_central_density * (1.0 + x * x).powf(-2.0)
}

/// f64 host-side halo column density.
fn halo_column_host(r: f64, p: &GalaxyParams) -> f64 {
    if p.halo_radius <= 0.0 || r < 1e-6 {
        return p.halo_radius * p.halo_central_density * std::f64::consts::PI;
    }
    let x = r / p.halo_radius;
    std::f64::consts::PI
        * p.halo_radius
        * p.halo_central_density
        * (1.0 + x).powf(p.halo_slope + 1.0)
}

/// f64 host-side arm modulation.
fn arm_modulation_host(wx: f64, wz: f64, r: f64, p: &GalaxyParams) -> f64 {
    let hr = p.disk_scale_length;
    if p.arm_count == 0 || p.arm_strength <= 0.0 || hr <= 0.0 {
        return 1.0;
    }
    let theta = (wx as f32).atan2(wz as f32) as f64;
    let cot_phi = 1.0 / p.arm_pitch.tan();
    let log_spiral = theta - cot_phi * (1.0 + r / hr).ln();
    let arm_width = 1.0 / p.arm_concentration;
    let mut min_dtheta = std::f64::consts::PI;
    for k in 0..p.arm_count {
        let phase = log_spiral + std::f64::consts::TAU * (k as f64) / (p.arm_count as f64);
        let dtheta = phase % std::f64::consts::TAU;
        let dtheta = if dtheta > std::f64::consts::PI {
            dtheta - std::f64::consts::TAU
        } else {
            dtheta
        };
        min_dtheta = min_dtheta.min(dtheta.abs());
    }
    let arg = min_dtheta / arm_width;
    1.0 + p.arm_strength * (-0.5 * arg * arg).exp()
}

/// Host-side star cell hash (mirrors shader's hash3).
fn hash3_host(x: u32, y: u32, seed: u32) -> u32 {
    let mut x = x.wrapping_mul(0xcc9e2d51).wrapping_add(y);
    x = x.rotate_left(15);
    x = x.wrapping_mul(0x1b873593);
    x ^= seed;
    x ^= x >> 16;
    x = x.wrapping_mul(0x85ebca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2ae35);
    x ^= x >> 16;
    x
}

/// Host-side star cell coordinate.
fn star_cell_host(w: f32) -> u32 {
    ((w + 1_000_000.0) * 10.0) as u32
}

/// Host-side IMF mass sample (mirrors shader).
fn sample_imf_host(cx: u32, cz: u32) -> f32 {
    let h = hash3_host(cx, cz, 123u32);
    let seg = h;
    let u = (h >> 8) as f32 / 16777215.0;
    const IMF1_M_MIN_POW: f32 = 2.133_391_9;
    const IMF1_RANGE: f32 = -0.902_161_4;
    const IMF1_INV_E: f32 = -3.333_333_3;
    const IMF2_M_MIN_POW: f32 = 2.462_288_8;
    const IMF2_RANGE: f32 = -2.459_776_8;
    const IMF2_INV_E: f32 = -0.769_230_8;
    const IMF_SEG_THRESH: u32 = 2_636_411_560;
    if seg < IMF_SEG_THRESH {
        (IMF1_M_MIN_POW + IMF1_RANGE * u).powf(IMF1_INV_E)
    } else {
        (IMF2_M_MIN_POW + IMF2_RANGE * u).powf(IMF2_INV_E)
    }
}

/// Host-side mass→temperature (mirrors shader).
fn mass_to_temp_host(m: f64) -> f64 {
    if m <= 0.08 {
        return 2300.0;
    }
    let (ref_m, ref_t, exp) = if m < 0.50 {
        (0.16, 3060.0, 0.53)
    } else if m < 1.0 {
        (0.88, 5240.0, 0.67)
    } else if m < 2.40 {
        (1.00, 5770.0, 0.57)
    } else if m < 7.0 {
        (2.40, 9700.0, 0.36)
    } else {
        (15.0, 30000.0, 0.20)
    };
    (ref_t * (m / ref_m).powf(exp)).min(50000.0)
}

fn mass_to_lum_f64(m: f64) -> f64 {
    const L_BREAK: f64 = 11.313_708;
    if m < 2.0 {
        m.powf(3.5)
    } else {
        L_BREAK * (m / 2.0)
    }
}

/// Generate a deterministic star catalogue.
///
/// Scans a coarse grid over the galaxy's XZ plane, uses hash-based
/// IMF sampling and column density to decide whether each cell contains
/// a bright star, and assigns a vertical offset from sech² profile.
///
/// Candidates are selected via weighted random sampling (A-Res):
/// each star gets key = –ln(hash) / column_density; sorting by
/// key and taking the top `max_stars` preserves the exponential
/// disk profile, bulge concentration, and spiral structure in the
/// resulting point cloud.
pub fn generate_star_catalogue(params: &GalaxyParams, max_stars: usize) -> Vec<StarInstance> {
    // Extend to 8 disk scale lengths so the exponential tail fades
    // naturally before the cutoff (Σ/Σ₀ ≈ e⁻⁸ ≈ 0.03 %).
    // Capped at 80 kLY to bound the scan grid for large galaxies.
    let disc_radius = (8.0 * params.disk_scale_length).clamp(50_000.0, 80_000.0);
    let half_side = (disc_radius / CATALOGUE_CELL_SIZE as f64).ceil() as i32;

    struct Candidate {
        /// Weighted-reservoir key: smaller = higher priority.
        key: f64,
        star: StarInstance,
    }
    let mut candidates: Vec<Candidate> = Vec::new();

    for ix in -half_side..=half_side {
        for iz in -half_side..=half_side {
            let wx = ix as f64 * CATALOGUE_CELL_SIZE as f64;
            let wz = iz as f64 * CATALOGUE_CELL_SIZE as f64;
            let r = (wx * wx + wz * wz).sqrt();
            if r > disc_radius {
                continue;
            }

            let col = disk_column_host(r, params) * arm_modulation_host(wx, wz, r, params)
                + bulge_column_host(r, params)
                + halo_column_host(r, params);
            if col <= 0.0 {
                continue;
            }

            let cell_area = CATALOGUE_CELL_SIZE as f64 * CATALOGUE_CELL_SIZE as f64;
            let lambda = col * cell_area;
            let prob = 1.0 - (-lambda).exp();

            let cx = star_cell_host(wx as f32);
            let cz = star_cell_host(wz as f32);
            let h = hash3_host(cx, cz, 77u32);
            let rnd = (h >> 8) as f64 / 16777215.0;
            if rnd >= prob {
                continue;
            }

            let mass = sample_imf_host(cx, cz);
            if (mass as f64) < CATALOGUE_MASS_THRESHOLD as f64 {
                continue;
            }

            let temp = mass_to_temp_host(mass as f64);
            let lum = mass_to_lum_f64(mass as f64);

            let jx = ((h & 0xFF) as f64 / 255.0 - 0.5) * CATALOGUE_CELL_SIZE as f64;
            let jz = (((h >> 16) & 0xFF) as f64 / 255.0 - 0.5) * CATALOGUE_CELL_SIZE as f64;

            let hy = hash3_host(cx, cz, 31337u32);
            let u_y = ((hy >> 8) as f64 / 16777215.0).clamp(0.001, 0.999);
            let y_offset = 2.0 * params.disk_scale_height * (2.0 * u_y - 1.0).atanh();

            // Weighted-reservoir key (A-Res): –ln(uniform) / density
            // Cells with higher column density produce smaller keys,
            // so they dominate the top-k selection.
            let key_hash = hash3_host(cx, cz, 99991u32);
            let key_u = (key_hash as f64 / u32::MAX as f64).max(1e-12);
            let key = -key_u.ln() / col;

            candidates.push(Candidate {
                key,
                star: StarInstance {
                    pos_x: (wx + jx) as f32,
                    pos_y: y_offset as f32,
                    pos_z: (wz + jz) as f32,
                    mass,
                    temp: temp as f32,
                    lum: lum as f32,
                    _pad: 0,
                },
            });
        }
    }

    candidates.sort_unstable_by(|a, b| a.key.partial_cmp(&b.key).unwrap());
    candidates.truncate(max_stars);
    candidates.into_iter().map(|c| c.star).collect()
}

/// Holds all GPU resources for the instanced star rendering pass.
pub struct GpuStars {
    pub instance_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub camera_buffer: wgpu::Buffer,
    pub brightness_buffer: wgpu::Buffer,
}

impl GpuStars {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let stars_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stars.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stars.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stars"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stars"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stars"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &stars_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &stars_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Camera uniform buffer (64 bytes = mat4)
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star_camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Brightness uniform buffer (16 bytes = vec4<f32> for alignment)
        let brightness_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star_brightness"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Instance buffer (header + max_stars * stride)
        let star_stride = 7u32; // 7 u32 per StarInstance
        let header_bytes = 8u64; // 2 u32 = count + capacity
        let instance_bytes = MAX_STARS as u64 * star_stride as u64 * 4;
        let buf_size = header_bytes + instance_bytes;

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star_instances"),
            size: buf_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stars-bind"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instance_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: brightness_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            instance_buffer,
            bind_group,
            bind_group_layout,
            pipeline,
            camera_buffer,
            brightness_buffer,
        }
    }
}

/// Filter star instances to those within the 2D ortho viewport rectangle.
/// Returns a subset of `catalogue` whose (pos_x, pos_z) fall within the
/// given world-space XZ bounds.
pub fn cull_stars_to_viewport(
    catalogue: &[StarInstance],
    min_x: f64,
    max_x: f64,
    min_z: f64,
    max_z: f64,
) -> Vec<StarInstance> {
    catalogue
        .iter()
        .filter(|s| {
            let x = s.pos_x as f64;
            let z = s.pos_z as f64;
            x >= min_x && x <= max_x && z >= min_z && z <= max_z
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
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

    use super::*;
    use crate::galaxy::GalaxyParams;

    #[test]
    fn wgsl_color_lut_matches_rust_lut() {
        let wgsl_source = include_str!("stars.wgsl");
        let start = wgsl_source.find("const LUT_DATA:").unwrap();
        let end = wgsl_source[start..].find(");").unwrap() + start + 2;
        let lut_section = &wgsl_source[start..end];

        for (i, entry) in COLOR_LUT_HOST.iter().enumerate() {
            let expected = format!(
                "vec4<f32>({:.1}, {:.3}, {:.3}, {:.3})",
                entry.0, entry.1, entry.2, entry.3
            );
            assert!(
                lut_section.contains(&expected),
                "LUT entry {i} ({expected}) not found in stars.wgsl"
            );
        }
    }

    // ── Star catalogue characterisation tests ────────────────

    /// Star catalogue is deterministic: same params + seed → identical output.
    #[test]
    fn star_catalogue_deterministic() {
        let params = GalaxyParams::milky_way();
        let a = generate_star_catalogue(&params, 500);
        let b = generate_star_catalogue(&params, 500);
        assert_eq!(a, b);
    }

    /// No NaN or infinite values in any star field.
    #[test]
    fn star_catalogue_no_nan_or_infinite() {
        let catalogue = generate_star_catalogue(&GalaxyParams::milky_way(), 1000);
        for star in &catalogue {
            assert!(star.pos_x.is_finite());
            assert!(star.pos_y.is_finite());
            assert!(star.pos_z.is_finite());
            assert!(star.mass.is_finite());
            assert!(star.temp.is_finite());
            assert!(star.lum.is_finite());
        }
    }

    /// Each star has physically plausible mass, temperature, and luminosity.
    #[test]
    fn star_catalogue_stars_within_physical_bounds() {
        let catalogue = generate_star_catalogue(&GalaxyParams::milky_way(), 1000);
        assert_eq!(catalogue.len(), 1000);
        for star in &catalogue {
            assert!(
                star.mass > 0.08 && star.mass < 100.0,
                "mass {} out of range",
                star.mass
            );
            assert!(
                star.temp >= 2300.0 && star.temp <= 50000.0,
                "temp {} out of range",
                star.temp
            );
            assert!(star.lum > 0.0, "lum {} <= 0", star.lum);
        }
    }

    /// Star positions span the full disc (at least 40k LY in extent).
    #[test]
    fn star_catalogue_spatial_extent() {
        let catalogue = generate_star_catalogue(&GalaxyParams::milky_way(), 10_000);
        let (mut x_min, mut x_max) = (f32::MAX, f32::MIN);
        let (mut z_min, mut z_max) = (f32::MAX, f32::MIN);
        for star in &catalogue {
            x_min = x_min.min(star.pos_x);
            x_max = x_max.max(star.pos_x);
            z_min = z_min.min(star.pos_z);
            z_max = z_max.max(star.pos_z);
        }
        assert!(
            x_max - x_min > 40_000.0,
            "x span {} too small",
            x_max - x_min
        );
        assert!(
            z_max - z_min > 40_000.0,
            "z span {} too small",
            z_max - z_min
        );
        assert!(
            x_min < -20_000.0 && x_max > 20_000.0,
            "x range {}..{} not spanning origin",
            x_min,
            x_max
        );
        assert!(
            z_min < -20_000.0 && z_max > 20_000.0,
            "z range {}..{} not spanning origin",
            z_min,
            z_max
        );
    }

    /// Bulge region is significantly enriched over the outer disc.
    #[test]
    fn star_catalogue_bulge_enriched_over_outer_disc() {
        let params = GalaxyParams::milky_way();
        let catalogue = generate_star_catalogue(&params, 10_000);
        let centre_count = catalogue
            .iter()
            .filter(|s| s.pos_x.powi(2) + s.pos_z.powi(2) < 5000.0_f32.powi(2))
            .count() as f64;
        let outer_count = catalogue
            .iter()
            .filter(|s| {
                let r = (s.pos_x.powi(2) + s.pos_z.powi(2)).sqrt();
                r > 40_000.0 && r < 45_000.0
            })
            .count() as f64;
        let centre_density = centre_count / (std::f64::consts::PI * 5000.0_f64.powi(2));
        let outer_area = std::f64::consts::PI * (45_000.0_f64.powi(2) - 40_000.0_f64.powi(2));
        let outer_density = outer_count / outer_area;
        let ratio = centre_density / outer_density;
        assert!(
            ratio > 10.0,
            "bulge enrichment ratio {} should exceed 10",
            ratio
        );
    }

    /// Different presets produce different catalogues.
    #[test]
    fn star_catalogue_different_presets_differ() {
        let mw = generate_star_catalogue(&GalaxyParams::milky_way(), 500);
        let ngc = generate_star_catalogue(&GalaxyParams::ngc628(), 500);
        assert_ne!(mw, ngc, "catalogues for different presets should differ");
    }

    /// NGC 628 has larger disk scale length → catalogue extends further than Milky Way.
    #[test]
    fn star_catalogue_ngc628_more_extended_than_milky_way() {
        let mw = generate_star_catalogue(&GalaxyParams::milky_way(), 5000);
        let ngc = generate_star_catalogue(&GalaxyParams::ngc628(), 5000);
        let mw_extent = mw
            .iter()
            .map(|s| (s.pos_x.powi(2) + s.pos_z.powi(2)).sqrt())
            .fold(0.0_f32, f32::max);
        let ngc_extent = ngc
            .iter()
            .map(|s| (s.pos_x.powi(2) + s.pos_z.powi(2)).sqrt())
            .fold(0.0_f32, f32::max);
        assert!(
            ngc_extent > mw_extent,
            "NGC 628 extent {} should exceed MW extent {}",
            ngc_extent,
            mw_extent
        );
    }

    // ── AABB cull filter tests ─────────────────────────────

    #[test]
    fn cull_stars_to_viewport_empty_catalogue() {
        let empty: Vec<StarInstance> = Vec::new();
        let result = cull_stars_to_viewport(&empty, 0.0, 100.0, 0.0, 100.0);
        assert!(result.is_empty(), "empty catalogue should return empty");
    }

    #[test]
    fn cull_stars_to_viewport_all_inside() {
        let stars = vec![
            StarInstance {
                pos_x: 10.0,
                pos_y: 0.0,
                pos_z: 20.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
            StarInstance {
                pos_x: -5.0,
                pos_y: 0.0,
                pos_z: 15.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
        ];
        let result = cull_stars_to_viewport(&stars, -10.0, 50.0, -10.0, 50.0);
        assert_eq!(
            result.len(),
            2,
            "all stars inside bounds should be returned"
        );
    }

    #[test]
    fn cull_stars_to_viewport_all_outside() {
        let stars = vec![
            StarInstance {
                pos_x: 200.0,
                pos_y: 0.0,
                pos_z: 200.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
            StarInstance {
                pos_x: -200.0,
                pos_y: 0.0,
                pos_z: -200.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
        ];
        let result = cull_stars_to_viewport(&stars, -50.0, 50.0, -50.0, 50.0);
        assert_eq!(
            result.len(),
            0,
            "all stars outside bounds should be filtered"
        );
    }

    #[test]
    fn cull_stars_to_viewport_mixed() {
        let stars = vec![
            StarInstance {
                pos_x: 10.0,
                pos_y: 0.0,
                pos_z: 20.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
            StarInstance {
                pos_x: 100.0,
                pos_y: 0.0,
                pos_z: 200.0,
                mass: 1.0,
                temp: 5770.0,
                lum: 1.0,
                _pad: 0,
            },
        ];
        let result = cull_stars_to_viewport(&stars, -50.0, 50.0, -50.0, 50.0);
        assert_eq!(
            result.len(),
            1,
            "only the star inside bounds should be returned"
        );
        assert_eq!(result[0].pos_x, 10.0);
    }
}
