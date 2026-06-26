// ── Named constants for camera, orbit, and zoom ────────────

/// Radians of orbit rotation per pixel of mouse drag.
/// Tuned for ~30° rotation over a ~100px drag at default window size.
pub const ORBIT_SENSITIVITY: f32 = 0.005;

/// Near clipping plane for perspective projection (light-years).
pub const CAMERA_NEAR: f32 = 100.0;

/// Far clipping plane for perspective projection (light-years).
pub const CAMERA_FAR: f32 = 1_000_000.0;

/// Threshold for detecting when camera forward vector is near-parallel
/// to the world up vector (0, 1, 0). When |dot(forward, up)| > this value,
/// the look-at matrix is degenerate; we fall back to an alternative up vector.
pub const UP_PARALLEL_THRESHOLD: f32 = 0.9999;

/// Minimum elevation angle (radians from the XZ plane upward).
/// 0.01 rad ≈ 0.57° — prevents gimbal lock at exactly ±π/2.
pub const ELEVATION_MIN: f32 = 0.01;

/// Scroll-wheel zoom in factor (1 / ZOOM_SPEED) and zoom out factor.
/// 1.15 gives ~15% distance change per scroll notch.
pub const ZOOM_SPEED: f64 = 1.15;

/// Minimum camera distance from target (light-years).
pub const CAMERA_DIST_MIN: f32 = 5_000.0;

/// Maximum camera distance from target (light-years).
pub const CAMERA_DIST_MAX: f32 = 500_000.0;

/// Default camera distance from target (light-years).
pub const CAMERA_DIST_DEFAULT: f32 = 100_000.0;

/// Default vertical field of view (degrees).
pub const FOV_DEFAULT: f32 = 45.0;

// ── Camera state ───────────────────────────────────────────

/// Camera state for 3D galaxy view.
pub struct Camera {
    pub dist: f32,
    pub azimuth: f32,
    pub elevation: f32,
    pub target: glam::Vec3,
    pub fov_y_deg: f32,
}

impl Camera {
    /// Camera eye position in world space.
    pub fn position(&self) -> glam::Vec3 {
        let horiz = self.dist * self.elevation.cos();
        glam::Vec3::new(
            self.target.x + horiz * self.azimuth.sin(),
            self.target.y + self.dist * self.elevation.sin(),
            self.target.z + horiz * self.azimuth.cos(),
        )
    }

    /// Orthographic projection for 2D top-down mode.
    /// Maps world XZ coords to clip space. Y (height) is unused in 2D.
    pub fn ortho_proj_matrix(
        extent_x: f32,
        extent_y: f32,
        center_x: f32,
        center_y: f32,
        near: f32,
        far: f32,
    ) -> glam::Mat4 {
        let left = center_x - extent_x * 0.5;
        let right = center_x + extent_x * 0.5;
        let bottom = center_y - extent_y * 0.5;
        let top = center_y + extent_y * 0.5;
        // Camera looks down Y axis (from +Y toward XZ plane)
        let eye = glam::Vec3::new(0.0, far * 0.5, 0.0);
        let target = glam::Vec3::new(0.0, 0.0, 0.0);
        let up = glam::Vec3::NEG_Z; // Z = up on screen
        let view = glam::Mat4::look_at_rh(eye, target, up);
        let proj = glam::Mat4::orthographic_rh(left, right, bottom, top, near, far);
        proj * view
    }

    /// View-projection matrix for the given aspect ratio.
    pub fn view_proj_matrix(&self, aspect: f32) -> glam::Mat4 {
        let eye = self.position();
        let mut up = glam::Vec3::Y;

        let dir = (self.target - eye).normalize();
        if dir.dot(up).abs() > UP_PARALLEL_THRESHOLD {
            up = glam::Vec3::Z;
        }

        let view = glam::Mat4::look_at_rh(eye, self.target, up);
        let proj = glam::Mat4::perspective_rh(
            self.fov_y_deg.to_radians(),
            aspect,
            CAMERA_NEAR,
            CAMERA_FAR,
        );
        proj * view
    }

    /// Apply mouse-drag orbit delta (radians).
    pub fn orbit(&mut self, d_azimuth: f32, d_elevation: f32) {
        self.azimuth -= d_azimuth;
        self.elevation = (self.elevation + d_elevation).clamp(
            -std::f32::consts::FRAC_PI_2 + ELEVATION_MIN,
            std::f32::consts::FRAC_PI_2 - ELEVATION_MIN,
        );
    }

    /// Zoom by a multiplicative factor (< 1 = zoom in, > 1 = zoom out).
    pub fn zoom(&mut self, factor: f32) {
        self.dist = (self.dist * factor).clamp(CAMERA_DIST_MIN, CAMERA_DIST_MAX);
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            dist: CAMERA_DIST_DEFAULT,
            azimuth: 0.0,
            elevation: std::f32::consts::FRAC_PI_2 - 0.05,
            target: glam::Vec3::ZERO,
            fov_y_deg: FOV_DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ortho_proj_matrix_sanity() {
        // Center (0,0), extent 100, near=0, far=1000
        let m = Camera::ortho_proj_matrix(100.0, 100.0, 0.0, 0.0, 0.0, 1000.0);
        // Test that corners of the viewport map to ±1 in clip space
        let corners = [
            (50.0, 50.0),   // top-right
            (-50.0, 50.0),  // top-left
            (50.0, -50.0),  // bottom-right
            (-50.0, -50.0), // bottom-left
        ];
        for (x, z) in &corners {
            let clip = m * glam::Vec4::new(*x, 0.0, *z, 1.0);
            let ndc = clip.truncate() / clip.w;
            assert!(
                ndc.x.abs() <= 1.0 + 1e-5 && ndc.z.abs() <= 1.0 + 1e-5,
                "corner ({x}, {z}) maps to NDC ({}, {}) outside [-1,1]",
                ndc.x, ndc.z
            );
        }
    }
}
