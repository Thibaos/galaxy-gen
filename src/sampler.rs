use glam::DVec3;
use std::collections::{BinaryHeap, HashSet};

use crate::galaxy::{self, GalaxyParams, Star};

pub fn sample_nearest(
    query: DVec3,
    n: usize,
    params: &GalaxyParams,
    max_radius: Option<f64>,
) -> Vec<Star> {
    if n == 0 {
        return Vec::new();
    }

    let local_density = params.density(query).max(1e-12);
    let target_per_cell = (n as f64 * 0.05).max(1.0);
    let cell_size = (target_per_cell / local_density).cbrt().clamp(1.0, 200.0);

    let max_dist = max_radius.unwrap_or(f64::MAX);

    let mut closest: BinaryHeap<Entry> = BinaryHeap::with_capacity(n);

    let mut cells: BinaryHeap<CellDist> = BinaryHeap::new();
    let mut visited: HashSet<(i64, i64, i64)> = HashSet::new();

    let origin_cell = world_to_cell(query, cell_size);
    cells.push(CellDist(
        dist_to_cell_min(query, origin_cell, cell_size),
        origin_cell,
    ));
    visited.insert(origin_cell);

    while let Some(CellDist(min_dist, (ix, iy, iz))) = cells.pop() {
        if let Some(farthest) = closest.peek()
            && closest.len() >= n
            && min_dist >= farthest.dist
        {
            break;
        }

        if min_dist > max_dist {
            break;
        }

        let cell_origin = cell_to_world((ix, iy, iz), cell_size);
        for star in galaxy::generate_stars_in_cell(cell_origin, cell_size, params) {
            let d = (star.position - query).length();
            if d > max_dist {
                continue;
            }
            if closest.len() < n {
                closest.push(Entry { dist: d, star });
            } else if let Some(farthest) = closest.peek()
                && d < farthest.dist
            {
                closest.pop();
                closest.push(Entry { dist: d, star });
            }
        }

        for (nx, ny, nz) in neighbours((ix, iy, iz)) {
            if visited.insert((nx, ny, nz)) {
                let d = dist_to_cell_min(query, (nx, ny, nz), cell_size);
                if d <= max_dist {
                    cells.push(CellDist(d, (nx, ny, nz)));
                }
            }
        }
    }

    closest
        .into_sorted_vec()
        .into_iter()
        .map(|e| e.star)
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    dist: f64,
    star: Star,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.dist.total_cmp(&other.dist)
    }
}

#[derive(Debug, Clone, Copy)]
struct CellDist(f64, (i64, i64, i64));
impl PartialEq for CellDist {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for CellDist {}

impl PartialOrd for CellDist {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CellDist {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0)
    }
}

fn world_to_cell(pos: DVec3, cell_size: f64) -> (i64, i64, i64) {
    (
        (pos.x / cell_size).floor() as i64,
        (pos.y / cell_size).floor() as i64,
        (pos.z / cell_size).floor() as i64,
    )
}

fn cell_to_world((ix, iy, iz): (i64, i64, i64), cell_size: f64) -> DVec3 {
    DVec3::new(
        ix as f64 * cell_size,
        iy as f64 * cell_size,
        iz as f64 * cell_size,
    )
}

fn dist_to_cell_min(query: DVec3, (ix, iy, iz): (i64, i64, i64), cell_size: f64) -> f64 {
    let min_corner = cell_to_world((ix, iy, iz), cell_size);
    let max_corner = min_corner + DVec3::splat(cell_size);

    let dx = if query.x < min_corner.x {
        min_corner.x - query.x
    } else if query.x > max_corner.x {
        query.x - max_corner.x
    } else {
        0.0f64
    };
    let dy = if query.y < min_corner.y {
        min_corner.y - query.y
    } else if query.y > max_corner.y {
        query.y - max_corner.y
    } else {
        0.0f64
    };
    let dz = if query.z < min_corner.z {
        min_corner.z - query.z
    } else if query.z > max_corner.z {
        query.z - max_corner.z
    } else {
        0.0f64
    };

    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn neighbours((ix, iy, iz): (i64, i64, i64)) -> impl Iterator<Item = (i64, i64, i64)> {
    [
        (ix - 1, iy, iz),
        (ix + 1, iy, iz),
        (ix, iy - 1, iz),
        (ix, iy + 1, iz),
        (ix, iy, iz - 1),
        (ix, iy, iz + 1),
    ]
    .into_iter()
}
