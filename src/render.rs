use glam::DVec3;
use rayon::prelude::*;

use crate::galaxy::GalaxyParams;
use crate::sampler;

pub fn render_top_down(
    params: &GalaxyParams,
    image_size: u32,
    galaxy_extent_ly: f64,
    n_nearest: usize,
    output_path: &str,
) {
    assert!(image_size > 0);
    assert!(galaxy_extent_ly > 0.0);
    assert!(n_nearest > 0);

    let pixel_count = (image_size as usize) * (image_size as usize);

    println!("Rendering {image_size}×{image_size} top-down image ");
    println!(
        "  extent: ±{:.0} kly, N={n_nearest}, {pixel_count} pixels",
        galaxy_extent_ly / 1_000.0 / 2.0
    );

    let start = std::time::Instant::now();

    let nth_distances: Vec<f64> = (0..pixel_count)
        .into_par_iter()
        .map(|idx| {
            let u = idx as u32 % image_size;
            let v = idx as u32 / image_size;

            let half = galaxy_extent_ly * 0.5;
            let x = (u as f64 / image_size as f64 - 0.5) * galaxy_extent_ly;
            let z = (v as f64 / image_size as f64 - 0.5) * galaxy_extent_ly;

            let query = DVec3::new(x, 0.0, z);

            let stars = sampler::sample_nearest(query, n_nearest, params, Some(half * 2.0));

            if stars.len() >= n_nearest {
                (stars[n_nearest - 1].position - query).length()
            } else {
                half * 4.0
            }
        })
        .collect();

    let elapsed_pass1 = start.elapsed();

    let eps = 1e-12;
    let log_densities: Vec<f64> = nth_distances
        .iter()
        .map(|&r| (1.0 / (r * r * r) + eps).ln())
        .collect();

    let min_log = log_densities.iter().copied().fold(f64::INFINITY, f64::min);
    let max_log = log_densities
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    println!(
        "  distance range: {:.1} – {:.0} ly, log-density range: {:.1} – {:.1} ({:.2?} compute)",
        nth_distances.iter().copied().fold(f64::INFINITY, f64::min),
        nth_distances.iter().copied().fold(0.0f64, f64::max),
        min_log,
        max_log,
        elapsed_pass1
    );

    let log_range = max_log - min_log;
    let pixels: Vec<u8> = log_densities
        .iter()
        .map(|&ld| {
            let brightness = if log_range > 1e-6 {
                (ld - min_log) / log_range
            } else {
                0.5
            };
            (brightness * 255.0) as u8
        })
        .collect();

    image::save_buffer(
        output_path,
        &pixels,
        image_size,
        image_size,
        image::ColorType::L8,
    )
    .expect("Failed to write PNG");

    let elapsed_total = start.elapsed();
    println!("  wrote {output_path} ({elapsed_total:.2?} total)");
}
