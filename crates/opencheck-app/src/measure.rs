//! Measuring in the 3D view.
//!
//! The laser works like a handheld laser distance meter: it sits on the
//! surface under the cursor and shoots straight out of it until it hits the
//! next surface. The reading follows the cursor live; a click keeps it.
//! Ctrl+click measures from point to point instead.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::camera::Ray;
use crate::snap::{EdgeCache, Snap};

/// Small offset so the laser does not hit the surface it starts on.
const SURFACE_OFFSET: f32 = 0.002;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MeasureKind {
    Laser,
    PointToPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub a: Vec3,
    pub b: Vec3,
    pub kind: MeasureKind,
}

impl Measurement {
    pub fn length(&self) -> f32 {
        self.a.distance(self.b)
    }

    pub fn label(&self) -> String {
        let main = format_length(self.length());
        let dz = (self.b.z - self.a.z).abs();
        match self.kind {
            // Show the height difference when the line is neither flat nor vertical.
            MeasureKind::PointToPoint if dz > 0.001 && (self.length() - dz).abs() > 0.001 => {
                format!("{main}   ΔZ {}", format_length(dz))
            }
            _ => main,
        }
    }
}

pub fn format_length(metres: f32) -> String {
    format!("{metres:.3} m")
}

/// The ray the laser shoots from a surface point along its normal.
pub fn laser_ray(point: Vec3, normal: Vec3) -> Ray {
    Ray {
        origin: point + normal * SURFACE_OFFSET,
        dir: normal,
    }
}

#[derive(Debug, Default)]
pub struct MeasureTool {
    pub active: bool,
    /// Reading under the cursor, updated every frame.
    pub live: Option<Measurement>,
    /// Laser start point when the beam hits nothing, to still show where it is.
    pub dangling: Option<(Vec3, Vec3)>,
    /// First point of a point-to-point measurement in progress.
    pub start: Option<Vec3>,
    pub pinned: Vec<Measurement>,
    /// Where a point-to-point click would land, shown under the cursor.
    pub snap: Option<Snap>,
    /// Feature edges of the meshes snapped to so far.
    pub edges: EdgeCache,
}

impl MeasureTool {
    pub fn toggle(&mut self) {
        self.active = !self.active;
        self.live = None;
        self.dangling = None;
        self.start = None;
        self.snap = None;
    }

    pub fn clear(&mut self) {
        self.pinned.clear();
        self.start = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        let flat = Measurement {
            a: Vec3::ZERO,
            b: Vec3::new(3.0, 4.0, 0.0),
            kind: MeasureKind::PointToPoint,
        };
        assert_eq!(flat.label(), "5.000 m");
        let sloped = Measurement {
            b: Vec3::new(3.0, 0.0, 4.0),
            ..flat
        };
        assert_eq!(sloped.label(), "5.000 m   ΔZ 4.000 m");
    }
}
