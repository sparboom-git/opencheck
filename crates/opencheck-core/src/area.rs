//! A working area: a rectangle in plan, possibly turned, that limits what is
//! shown and checked to one part of the project. Elements whose extent
//! touches the rectangle are in; nothing is removed from the IFC files.

use glam::{DVec2, DVec3, Mat4};
use serde::{Deserialize, Serialize};

use crate::{Aabb, Element, IfcModel};

/// Rectangle in plan, in world (project) coordinates, metres. All heights.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkArea {
    pub center: [f64; 2],
    /// Half the size along the rectangle's own axes.
    pub half: [f64; 2],
    /// Turn of the rectangle's first axis from world X, radians.
    pub angle: f64,
}

impl WorkArea {
    pub fn axes(&self) -> (DVec2, DVec2) {
        let u = DVec2::new(self.angle.cos(), self.angle.sin());
        (u, DVec2::new(-u.y, u.x))
    }

    /// The four corners, world XY, counter-clockwise.
    pub fn corners(&self) -> [DVec2; 4] {
        let c = DVec2::from_array(self.center);
        let (u, v) = self.axes();
        let (a, b) = (u * self.half[0], v * self.half[1]);
        [c - a - b, c + a - b, c + a + b, c - a + b]
    }

    /// Whether a box in world coordinates (plan extent `min`..`max`) touches
    /// the rectangle. Separating axes: world X, world Y and the rectangle's
    /// two axes.
    pub fn touches(&self, min: DVec2, max: DVec2) -> bool {
        let rect = self.corners();
        let boxed = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
        let (u, v) = self.axes();
        [DVec2::X, DVec2::Y, u, v].iter().all(|axis| {
            let span = |points: &[DVec2; 4]| {
                points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                    let d = p.dot(*axis);
                    (lo.min(d), hi.max(d))
                })
            };
            let (a0, a1) = span(&rect);
            let (b0, b1) = span(&boxed);
            a0 <= b1 && b0 <= a1
        })
    }

    /// Whether an element is in the area. `transform` places the model in the
    /// federation frame and `origin` is that frame's world position.
    /// Elements without geometry follow their parts; with neither they are in.
    pub fn includes(
        &self,
        model: &IfcModel,
        element: &Element,
        transform: &Mat4,
        origin: DVec3,
    ) -> bool {
        let mut bounds = element.bounds;
        for part in &element.parts {
            if let Some(child) = model.element(*part) {
                bounds = bounds.union(&child.bounds);
            }
        }
        if bounds.is_empty() {
            return true;
        }
        let world: Aabb = bounds.transformed(&transform.as_dmat4());
        let min = DVec2::new(world.min.x as f64 + origin.x, world.min.y as f64 + origin.y);
        let max = DVec2::new(world.max.x as f64 + origin.x, world.max.y as f64 + origin.y);
        self.touches(min, max)
    }

    /// Per element of the model: whether it is in the area.
    pub fn mask(&self, model: &IfcModel, transform: &Mat4, origin: DVec3) -> Vec<bool> {
        model
            .elements
            .iter()
            .map(|e| self.includes(model, e, transform, origin))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(angle: f64) -> WorkArea {
        WorkArea {
            center: [0.0, 0.0],
            half: [10.0, 5.0],
            angle,
        }
    }

    #[test]
    fn boxes_inside_touching_and_outside() {
        let area = square(0.0);
        assert!(area.touches(DVec2::new(-1.0, -1.0), DVec2::new(1.0, 1.0)));
        // Touching the edge counts, as in the old cropping tool.
        assert!(area.touches(DVec2::new(9.0, 0.0), DVec2::new(20.0, 1.0)));
        assert!(!area.touches(DVec2::new(11.0, 0.0), DVec2::new(20.0, 1.0)));
        assert!(!area.touches(DVec2::new(-1.0, 6.0), DVec2::new(1.0, 8.0)));
    }

    #[test]
    fn a_turned_rectangle_misses_boxes_in_its_bounding_corners() {
        // Turned 45°: the corner region of its bounding box is outside.
        let area = square(std::f64::consts::FRAC_PI_4);
        assert!(area.touches(DVec2::new(-1.0, -1.0), DVec2::new(1.0, 1.0)));
        assert!(!area.touches(DVec2::new(9.0, -10.0), DVec2::new(10.0, -9.0)));
        // Along the turned long axis it reaches further than 5 in y.
        assert!(area.touches(DVec2::new(6.0, 6.0), DVec2::new(6.5, 6.5)));
    }

    #[test]
    fn corners_follow_the_angle() {
        let c = square(std::f64::consts::FRAC_PI_2).corners();
        // Turned 90°: the long side runs along Y.
        assert!((c[1] - c[0]).y.abs() > 19.0);
    }
}
