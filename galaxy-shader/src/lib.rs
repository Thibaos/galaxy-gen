#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::glam::UVec3;
use spirv_std::spirv;

/// Compute shader entry point — staging for on-GPU star generation.
/// Currently does nothing; the 2D column-density glow has been removed.
#[spirv(compute(threads(8, 8, 1)))]
pub fn main_scene(
    #[spirv(global_invocation_id)] _id: UVec3,
) {
    // Placeholder — will be populated with instance-buffer-filling logic
}
