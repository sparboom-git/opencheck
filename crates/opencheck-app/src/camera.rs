//! Orbit camera with Z up.
//!
//! All navigation is anchored on a point in the model: orbit turns around the
//! point under the cursor, zoom moves towards it, and pan keeps it under the
//! cursor. That is what makes navigation feel direct.

use glam::{Mat4, Quat, Vec2, Vec3};
use opencheck_core::Aabb;

const MAX_PITCH: f32 = 89.0_f32.to_radians();

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    /// Rotation about +Z, radians. 0 looks along +Y.
    pub yaw: f32,
    /// Elevation of the view direction, radians. Negative looks down.
    pub pitch: f32,
    pub fov_y: f32,
    near: f32,
    far: f32,
}

/// A ray in world space.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }
}

/// Direction of the default 3D view: from the south-west, looking down.
const HOME_YAW: f32 = std::f32::consts::FRAC_PI_4;
const HOME_PITCH: f32 = -0.55;

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(-20.0, -20.0, 15.0),
            yaw: HOME_YAW,
            pitch: HOME_PITCH,
            fov_y: 45.0_f32.to_radians(),
            near: 0.05,
            far: 2000.0,
        }
    }
}

impl Camera {
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
        )
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), -self.yaw.sin(), 0.0)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward())
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.eye, self.forward(), Vec3::Z)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect.max(1e-3), self.near, self.far)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Fits the clip planes to the scene so depth precision is spent where
    /// the geometry is.
    pub fn update_clip_planes(&mut self, scene: &Aabb) {
        if scene.is_empty() {
            return;
        }
        let radius = scene.size().length() * 0.5;
        let distance_to_center = self.eye.distance(scene.center());
        self.far = (distance_to_center + radius) * 1.05 + 1.0;
        self.near = (self.far * 2e-5)
            .max((distance_to_center - radius) * 0.5)
            .max(0.01);
        self.near = self.near.min(self.far * 0.01);
    }

    /// Ray through a point in the viewport, given in normalised device
    /// coordinates (-1..1, y up).
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> Ray {
        let inv = self.view_proj(aspect).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        Ray {
            origin: self.eye,
            dir: (far - near).normalize(),
        }
    }

    /// Turns the camera around `pivot` while keeping the pivot fixed on screen.
    pub fn orbit(&mut self, pivot: Vec3, delta_yaw: f32, delta_pitch: f32) {
        let new_pitch = (self.pitch + delta_pitch).clamp(-MAX_PITCH, MAX_PITCH);
        let delta_pitch = new_pitch - self.pitch;

        let yaw_rotation = Quat::from_rotation_z(-delta_yaw);
        let mut offset = yaw_rotation * (self.eye - pivot);
        self.yaw += delta_yaw;
        let pitch_rotation = Quat::from_axis_angle(self.right(), delta_pitch);
        offset = pitch_rotation * offset;
        self.pitch = new_pitch;
        self.eye = pivot + offset;
    }

    /// Moves the camera parallel to the screen. `pixels` is the mouse delta,
    /// `depth` the distance to the point being dragged, so that point follows
    /// the cursor exactly.
    pub fn pan(&mut self, pixels: Vec2, depth: f32, viewport_height: f32) {
        let world_per_pixel = 2.0 * depth * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        self.eye += (-self.right() * pixels.x + self.up() * pixels.y) * world_per_pixel;
    }

    /// Moves towards `target` by `factor` of the distance (0.8 = 20 % closer).
    pub fn zoom_towards(&mut self, target: Vec3, factor: f32) {
        let new_eye = target + (self.eye - target) * factor;
        // Never step onto the target itself: keep a sliver of distance.
        if new_eye.distance(target) > 0.05 || factor > 1.0 {
            self.eye = new_eye;
        }
    }

    /// Moves the camera back along its current direction until `bounds` fills the view.
    pub fn fit(&mut self, bounds: &Aabb, aspect: f32) {
        if bounds.is_empty() {
            return;
        }
        let radius = (bounds.size().length() * 0.5).max(0.5);
        let half_fov =
            (self.fov_y * 0.5).min((self.fov_y * 0.5).tan().atan2(1.0 / aspect.max(0.2)));
        let distance = radius / half_fov.sin() * 1.05;
        self.eye = bounds.center() - self.forward() * distance;
        self.update_clip_planes(bounds);
    }

    /// The default 3D view of `bounds`.
    pub fn home(&mut self, bounds: &Aabb, aspect: f32) {
        self.yaw = HOME_YAW;
        self.pitch = HOME_PITCH;
        self.fit(bounds, aspect);
    }

    /// Moves in the horizontal plane for walk mode.
    pub fn walk(&mut self, forward: f32, strafe: f32, rise: f32) {
        let flat_forward = Vec3::new(self.yaw.sin(), self.yaw.cos(), 0.0);
        self.eye += flat_forward * forward + self.right() * strafe + Vec3::Z * rise;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_keeps_pivot_on_screen() {
        let mut camera = Camera::default();
        let pivot = camera.eye + camera.forward() * 10.0 + camera.right() * 2.0;
        let before = camera.view_proj(1.5).project_point3(pivot);
        camera.orbit(pivot, 0.4, -0.2);
        let after = camera.view_proj(1.5).project_point3(pivot);
        assert!((before.truncate() - after.truncate()).length() < 1e-3);
    }

    #[test]
    fn center_ray_matches_forward() {
        let camera = Camera::default();
        let ray = camera.ray(Vec2::ZERO, 1.5);
        assert!(ray.dir.distance(camera.forward()) < 1e-4);
    }

    #[test]
    fn fit_contains_bounds() {
        let mut camera = Camera::default();
        let bounds = Aabb {
            min: Vec3::new(-10.0, -5.0, 0.0),
            max: Vec3::new(10.0, 5.0, 6.0),
        };
        camera.fit(&bounds, 1.6);
        for corner in [bounds.min, bounds.max] {
            let ndc = camera.view_proj(1.6).project_point3(corner);
            assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "{ndc:?}");
        }
    }
}
