//! "Edit view": draw a working area in a top view, and limit its heights in
//! a side view (e.g. to two storeys). Only elements touching it are shown in
//! the model and checked; "Reset view" brings everything back. Nothing is
//! removed from the IFC files.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use egui_phosphor::regular as icon;
use glam::{DVec2, Vec3};
use opencheck_core::area::WorkArea;
use rustc_hash::FxHashSet;

use crate::app::OpenCheckApp;
use crate::camera::Camera;
use crate::scene::ElementRef;
use crate::theme;

/// Looking straight down, slightly off vertical so the camera keeps a heading.
const TOP_PITCH: f32 = -89.5 * std::f32::consts::PI / 180.0;
/// A narrow lens from far away looks almost like a plan drawing.
const TOP_FOV: f32 = 18.0 * std::f32::consts::PI / 180.0;
/// Pointer distance, in points, that grabs an edge.
const EDGE_GRAB: f32 = 9.0;
/// Smallest area side, metres.
const MIN_SIDE: f64 = 0.5;
/// A dragged height snaps to a storey this close on screen, points.
const STOREY_SNAP: f32 = 10.0;
/// Heights chosen by storey start this far below the storey's level, so the
/// floor slab, whose top is the level, is in. Metres.
const STOREY_MARGIN: f64 = 0.05;

/// The 3D view keeps its light background in both themes, so the area is
/// always drawn dark, with a light edge so it also reads over dark models.
fn area_color() -> Color32 {
    Color32::from_rgb(23, 25, 28)
}

fn area_halo() -> Color32 {
    Color32::from_white_alpha(200)
}

#[derive(Clone, Copy)]
enum Drag {
    New {
        start: DVec2,
    },
    Move {
        grab: DVec2,
        from: WorkArea,
    },
    /// Side 0: -v, 1: +u, 2: +v, 3: -u.
    Edge {
        side: usize,
        from: WorkArea,
    },
}

/// Dragging the height limits in the side view.
#[derive(Clone, Copy)]
enum HeightDrag {
    Bottom,
    Top,
    Both { grab: f64, from: [f64; 2] },
}

pub struct AreaEdit {
    saved_camera: Camera,
    /// The top view's camera while the side view is shown.
    top_camera: Option<Camera>,
    height_drag: Option<HeightDrag>,
    /// Storey names and levels (world Z, metres), lowest first.
    pub storeys: Vec<(String, f64)>,
    /// The area being drawn, world coordinates.
    pub draft: Option<WorkArea>,
    drag: Option<Drag>,
    /// Waiting for a click on an element to turn the view along it.
    pub aligning: bool,
    /// Height (federation frame) of the plane the pointer is projected on.
    plane_z: f32,
    /// Elements in the draft, and all elements, for the count shown.
    pub counts: (usize, usize),
}

impl OpenCheckApp {
    pub fn is_editing_area(&self) -> bool {
        self.area_edit.is_some()
    }

    /// Switches to the top view to draw the working area.
    pub fn enter_area_edit(&mut self) {
        if self.federation.is_empty() || self.area_edit.is_some() {
            return;
        }
        self.measure.active = false;
        self.view.selection.clear();
        let saved_camera = self.camera;
        let draft = self.checking.area;
        // Everything is shown while editing; what falls outside is ghosted.
        self.view.outside.clear();
        let bounds = self.scene_bounds;
        let angle = draft.map_or(0.0, |a| a.angle as f32);
        self.camera.yaw = -angle;
        self.camera.pitch = TOP_PITCH;
        self.camera.fov_y = TOP_FOV;
        let target = match draft {
            Some(area) => {
                let origin = self.federation.origin();
                let corners = area.corners();
                let mut b = opencheck_core::Aabb::EMPTY;
                for c in corners {
                    let p = Vec3::new(
                        (c.x - origin.x) as f32,
                        (c.y - origin.y) as f32,
                        bounds.center().z,
                    );
                    b = b.union(&opencheck_core::Aabb { min: p, max: p });
                }
                b.min.z = bounds.min.z;
                b.max.z = bounds.max.z;
                b
            }
            None => bounds,
        };
        self.camera.fit(&target, self.nav.aspect);
        self.camera.update_clip_planes(&bounds);
        let storeys = self.storey_levels();
        self.area_edit = Some(AreaEdit {
            saved_camera,
            top_camera: None,
            height_drag: None,
            storeys,
            draft,
            drag: None,
            aligning: false,
            plane_z: bounds.center().z,
            counts: (0, 0),
        });
        self.update_area_preview();
    }

    /// Storey levels of all models in world Z, lowest first; storeys of
    /// different models at the same level are shown once.
    fn storey_levels(&self) -> Vec<(String, f64)> {
        let origin = self.federation.origin();
        let mut levels: Vec<(String, f64)> = Vec::new();
        for (id, model) in self.federation.iter() {
            let Some(transform) = self.federation.model_transform(id) else {
                continue;
            };
            for storey in model.storeys() {
                let Some(elevation) = storey.elevation else {
                    continue;
                };
                let z = transform
                    .transform_point3(Vec3::new(
                        0.0,
                        0.0,
                        (elevation * model.length_unit_scale) as f32,
                    ))
                    .z as f64
                    + origin.z;
                if !levels.iter().any(|(_, l)| (l - z).abs() < 0.05) {
                    levels.push((storey.label().to_owned(), z));
                }
            }
        }
        levels.sort_by(|a, b| a.1.total_cmp(&b.1));
        levels
    }

    /// Bottom and top of all geometry, world Z.
    fn scene_heights(&self) -> [f64; 2] {
        let z = self.federation.origin().z;
        [
            self.scene_bounds.min.z as f64 + z,
            self.scene_bounds.max.z as f64 + z,
        ]
    }

    /// Switches between the top view (the rectangle) and the side view (the
    /// heights), looking across the rectangle's long axis.
    pub fn set_side_view(&mut self, side: bool) {
        let Some(edit) = &self.area_edit else {
            return;
        };
        match (side, edit.top_camera) {
            (true, None) => {
                let Some(area) = edit.draft else {
                    return;
                };
                let top = self.camera;
                let origin = self.federation.origin();
                let bounds = self.scene_bounds;
                // Same heading as the top view: the rectangle's first axis
                // runs across the screen.
                self.camera.yaw = -(area.angle as f32);
                self.camera.pitch = 0.0;
                self.camera.fov_y = TOP_FOV;
                let mut target = opencheck_core::Aabb::EMPTY;
                for c in area.corners() {
                    for z in [bounds.min.z, bounds.max.z] {
                        let p = Vec3::new((c.x - origin.x) as f32, (c.y - origin.y) as f32, z);
                        target = target.union(&opencheck_core::Aabb { min: p, max: p });
                    }
                }
                self.camera.fit(&target, self.nav.aspect);
                self.camera.update_clip_planes(&bounds);
                if let Some(edit) = &mut self.area_edit {
                    edit.top_camera = Some(top);
                    edit.aligning = false;
                }
            }
            (false, Some(top)) => {
                self.camera = top;
                if let Some(edit) = &mut self.area_edit {
                    edit.top_camera = None;
                    edit.height_drag = None;
                }
            }
            _ => {}
        }
    }

    /// Limits the area to storeys `from` to `to` (indices into the storey
    /// levels), or to all heights with None.
    fn set_storey_range(&mut self, range: Option<(usize, usize)>) {
        let scene = self.scene_heights();
        let Some(edit) = &mut self.area_edit else {
            return;
        };
        let heights = range.map(|(from, to)| {
            let (from, to) = (from.min(to), from.max(to));
            let bottom = edit.storeys[from].1 - STOREY_MARGIN;
            let top = edit
                .storeys
                .get(to + 1)
                .map_or(scene[1], |(_, z)| z - STOREY_MARGIN);
            [bottom, top]
        });
        if let Some(area) = &mut edit.draft {
            area.heights = heights;
        }
        self.update_area_preview();
    }

    /// Leaves the top view without changing the working area.
    pub fn cancel_area_edit(&mut self) {
        let Some(edit) = self.area_edit.take() else {
            return;
        };
        self.camera = edit.saved_camera;
        self.view.ghosted.clear();
        self.apply_area();
    }

    /// Makes the drawn area the working area.
    pub fn save_area_edit(&mut self) {
        let Some(edit) = self.area_edit.take() else {
            return;
        };
        self.camera = edit.saved_camera;
        self.view.ghosted.clear();
        if self.checking.area != edit.draft {
            self.checking.area = edit.draft;
            self.checking.stale = true;
            self.session.dirty = true;
        }
        self.apply_area();
        if self.checking.area.is_some() {
            self.fit_all();
        }
    }

    /// Everything is shown and checked again.
    pub fn reset_area(&mut self) {
        if self.checking.area.take().is_some() {
            self.checking.stale = true;
            self.session.dirty = true;
        }
        self.apply_area();
    }

    /// Elements outside the area, per model.
    fn outside_of(&self, area: &WorkArea) -> FxHashSet<ElementRef> {
        let origin = self.federation.origin();
        let mut outside = FxHashSet::default();
        for (id, model) in self.federation.iter() {
            let Some(transform) = self.federation.model_transform(id) else {
                continue;
            };
            for element in &model.elements {
                if !area.includes(model, element, &transform, origin) {
                    outside.insert(ElementRef {
                        model: id,
                        express_id: element.express_id,
                    });
                }
            }
        }
        outside
    }

    /// Hides what lies outside the working area (or nothing without one).
    pub fn apply_area(&mut self) {
        self.view.outside = match self.checking.area {
            Some(area) => self.outside_of(&area),
            None => FxHashSet::default(),
        };
        self.view.modes_dirty = true;
    }

    /// Number of elements with geometry inside the working area, and in all.
    pub fn area_counts(&self) -> (usize, usize) {
        let total: usize = self
            .federation
            .iter()
            .map(|(_, m)| m.elements.iter().filter(|e| e.has_geometry()).count())
            .sum();
        let outside = self
            .view
            .outside
            .iter()
            .filter(|r| {
                self.federation
                    .get(r.model)
                    .and_then(|m| m.element(r.express_id))
                    .is_some_and(|e| e.has_geometry())
            })
            .count();
        (total - outside.min(total), total)
    }

    fn update_area_preview(&mut self) {
        let Some(edit) = &self.area_edit else {
            return;
        };
        let ghosted = match edit.draft {
            Some(area) => self.outside_of(&area),
            None => FxHashSet::default(),
        };
        let total: usize = self
            .federation
            .iter()
            .map(|(_, m)| m.elements.iter().filter(|e| e.has_geometry()).count())
            .sum();
        let out = ghosted
            .iter()
            .filter(|r| {
                self.federation
                    .get(r.model)
                    .and_then(|m| m.element(r.express_id))
                    .is_some_and(|e| e.has_geometry())
            })
            .count();
        self.view.ghosted = ghosted;
        self.view.modes_dirty = true;
        if let Some(edit) = &mut self.area_edit {
            edit.counts = (total - out.min(total), total);
        }
    }

    /// Turns the top view so the element's longest direction runs across
    /// the screen, and the area with it.
    fn align_to(&mut self, element: ElementRef) {
        let Some(angle) = self.element_direction(element) else {
            return;
        };
        // Keep the point in the middle of the screen where it is.
        let distance = (self.scene_bounds.center().z - self.camera.eye.z)
            .abs()
            .max(1.0);
        let target = self.camera.eye + self.camera.forward() * distance;
        self.camera.yaw = -(angle as f32);
        self.camera.eye = target - self.camera.forward() * distance;
        if let Some(edit) = &mut self.area_edit {
            if let Some(area) = &mut edit.draft {
                area.angle = angle;
            }
            edit.aligning = false;
        }
        self.update_area_preview();
    }

    /// Main direction in plan of an element's geometry (radians from world X).
    fn element_direction(&self, element: ElementRef) -> Option<f64> {
        let model = self.federation.get(element.model)?;
        let e = model.element(element.express_id)?;
        let transform = self.federation.model_transform(element.model)?;
        let mut points: Vec<DVec2> = Vec::new();
        for &m in &e.meshes {
            for p in &model.meshes[m].positions {
                let w = transform.transform_point3(Vec3::from_array(*p));
                points.push(DVec2::new(w.x as f64, w.y as f64));
            }
        }
        if points.len() < 2 {
            return None;
        }
        let mean = points.iter().copied().sum::<DVec2>() / points.len() as f64;
        let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
        for p in &points {
            let d = *p - mean;
            xx += d.x * d.x;
            yy += d.y * d.y;
            xy += d.x * d.y;
        }
        let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
        // Keep the screen the right way up: between -90° and 90°.
        Some(angle)
    }
}

/// World XY under the pointer, on the plane at the edit height.
fn plane_point(
    app: &OpenCheckApp,
    rect: Rect,
    aspect: f32,
    pos: Pos2,
    plane_z: f32,
) -> Option<DVec2> {
    let ray = crate::viewport::pointer_ray(app, rect, aspect, pos);
    if ray.dir.z.abs() < 1e-6 {
        return None;
    }
    let t = (plane_z - ray.origin.z) / ray.dir.z;
    let p = ray.at(t);
    let origin = app.federation.origin();
    Some(DVec2::new(p.x as f64 + origin.x, p.y as f64 + origin.y))
}

fn to_screen(app: &OpenCheckApp, rect: Rect, aspect: f32, p: DVec2, plane_z: f32) -> Option<Pos2> {
    let origin = app.federation.origin();
    let local = Vec3::new((p.x - origin.x) as f32, (p.y - origin.y) as f32, plane_z);
    crate::viewport::project(app, rect, aspect, local)
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// A rectangle from two corners along the screen's axes.
fn rect_from_corners(a: DVec2, b: DVec2, angle: f64) -> WorkArea {
    let u = DVec2::new(angle.cos(), angle.sin());
    let v = DVec2::new(-u.y, u.x);
    let d = b - a;
    let center = a + d * 0.5;
    WorkArea {
        center: center.to_array(),
        half: [(d.dot(u).abs() * 0.5), (d.dot(v).abs() * 0.5)],
        angle,
        heights: None,
    }
}

/// Moves one side of the rectangle to `q`, keeping the opposite side.
fn move_side(from: &WorkArea, side: usize, q: DVec2) -> WorkArea {
    let (u, v) = from.axes();
    let c = DVec2::from_array(from.center);
    let (axis, sign, index) = match side {
        0 => (v, -1.0, 1),
        1 => (u, 1.0, 0),
        2 => (v, 1.0, 1),
        _ => (u, -1.0, 0),
    };
    let h = from.half[index];
    let fixed = -sign * h;
    let moved = (q - c).dot(axis);
    let lo = fixed.min(moved);
    let hi = fixed.max(moved);
    let mut out = *from;
    out.half[index] = ((hi - lo) * 0.5).max(MIN_SIDE * 0.5);
    let center = c + axis * ((lo + hi) * 0.5);
    out.center = center.to_array();
    out
}

/// Pointer handling in the top view; called instead of selection.
pub fn interact(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    aspect: f32,
) {
    let Some(edit) = &app.area_edit else {
        return;
    };
    let plane_z = edit.plane_z;
    let pointer = response.interact_pointer_pos().or(response.hover_pos());

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.cancel_area_edit();
        return;
    }
    if edit.top_camera.is_some() {
        side_interact(app, ui, rect, response, aspect);
        return;
    }

    if edit.aligning {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if response.clicked()
            && let Some(pos) = pointer
            && let Some(hit) = app.view.pick(
                &app.federation,
                &crate::viewport::pointer_ray(app, rect, aspect, pos),
            )
        {
            app.align_to(hit.element);
        }
        return;
    }

    // Which part of the area is under the pointer: an edge, the inside, or nothing.
    let hover_part = |app: &OpenCheckApp, pos: Pos2| -> Option<Option<usize>> {
        let area = app.area_edit.as_ref()?.draft?;
        let corners: Vec<Pos2> = area
            .corners()
            .iter()
            .filter_map(|c| to_screen(app, rect, aspect, *c, plane_z))
            .collect();
        if corners.len() != 4 {
            return None;
        }
        for side in 0..4 {
            if distance_to_segment(pos, corners[side], corners[(side + 1) % 4]) < EDGE_GRAB {
                return Some(Some(side));
            }
        }
        let q = plane_point(app, rect, aspect, pos, plane_z)?;
        let (u, v) = area.axes();
        let d = q - DVec2::from_array(area.center);
        (d.dot(u).abs() <= area.half[0] && d.dot(v).abs() <= area.half[1]).then_some(None)
    };

    if let Some(pos) = response.hover_pos()
        && app.area_edit.as_ref().is_some_and(|e| e.drag.is_none())
    {
        match hover_part(app, pos) {
            Some(Some(side)) => ui.ctx().set_cursor_icon(if side % 2 == 0 {
                egui::CursorIcon::ResizeVertical
            } else {
                egui::CursorIcon::ResizeHorizontal
            }),
            Some(None) => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
            None => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        }
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = pointer
        && let Some(q) = plane_point(app, rect, aspect, pos, plane_z)
    {
        let part = hover_part(app, pos);
        let draft = app.area_edit.as_ref().and_then(|e| e.draft);
        let drag = match (part, draft) {
            (Some(Some(side)), Some(from)) => Drag::Edge { side, from },
            (Some(None), Some(from)) => Drag::Move { grab: q, from },
            _ => Drag::New { start: q },
        };
        if let Some(edit) = &mut app.area_edit {
            edit.drag = Some(drag);
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = pointer
        && let Some(q) = plane_point(app, rect, aspect, pos, plane_z)
    {
        // The screen's horizontal axis in plan, for new rectangles.
        let right = app.camera.right();
        let angle = (right.y as f64).atan2(right.x as f64);
        if let Some(edit) = &mut app.area_edit
            && let Some(drag) = edit.drag
        {
            edit.draft = Some(match drag {
                Drag::New { start } => {
                    let mut area = rect_from_corners(start, q, angle);
                    area.heights = edit.draft.and_then(|a| a.heights);
                    area
                }
                Drag::Move { grab, from } => {
                    let mut moved = from;
                    moved.center = (DVec2::from_array(from.center) + (q - grab)).to_array();
                    moved
                }
                Drag::Edge { side, from } => move_side(&from, side, q),
            });
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(edit) = &mut app.area_edit {
            edit.drag = None;
            // A click-sized rectangle is no area.
            if edit
                .draft
                .is_some_and(|a| a.half[0] * 2.0 < MIN_SIDE || a.half[1] * 2.0 < MIN_SIDE)
            {
                edit.draft = None;
            }
        }
        app.update_area_preview();
    }
}

/// Screen height of a level (world Z) in the side view, at the area's centre.
fn level_y(app: &OpenCheckApp, rect: Rect, aspect: f32, area: &WorkArea, z: f64) -> Option<f32> {
    let origin = app.federation.origin();
    let p = Vec3::new(
        (area.center[0] - origin.x) as f32,
        (area.center[1] - origin.y) as f32,
        (z - origin.z) as f32,
    );
    crate::viewport::project(app, rect, aspect, p).map(|s| s.y)
}

/// World Z under the pointer in the side view: on the upright plane through
/// the area's centre that faces the camera.
fn level_at(
    app: &OpenCheckApp,
    rect: Rect,
    aspect: f32,
    area: &WorkArea,
    pos: Pos2,
) -> Option<f64> {
    let ray = crate::viewport::pointer_ray(app, rect, aspect, pos);
    let origin = app.federation.origin();
    let (_, v) = area.axes();
    let normal = Vec3::new(v.x as f32, v.y as f32, 0.0);
    let centre = Vec3::new(
        (area.center[0] - origin.x) as f32,
        (area.center[1] - origin.y) as f32,
        0.0,
    );
    let denom = ray.dir.dot(normal);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (centre - ray.origin).dot(normal) / denom;
    Some(ray.at(t).z as f64 + origin.z)
}

/// Pointer handling in the side view: drag the bottom or top line, or the
/// band between them. Lines snap to storey levels.
fn side_interact(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    aspect: f32,
) {
    let Some(area) = app.area_edit.as_ref().and_then(|e| e.draft) else {
        return;
    };
    let heights = area.heights.unwrap_or_else(|| app.scene_heights());
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let ys = (
        level_y(app, rect, aspect, &area, heights[0]),
        level_y(app, rect, aspect, &area, heights[1]),
    );
    let part = |pos: Pos2| -> Option<HeightDrag> {
        let (Some(bottom), Some(top)) = ys else {
            return None;
        };
        if (pos.y - bottom).abs() < EDGE_GRAB {
            Some(HeightDrag::Bottom)
        } else if (pos.y - top).abs() < EDGE_GRAB {
            Some(HeightDrag::Top)
        } else if pos.y < bottom && pos.y > top {
            Some(HeightDrag::Both {
                grab: 0.0,
                from: heights,
            })
        } else {
            None
        }
    };
    let dragging = app
        .area_edit
        .as_ref()
        .is_some_and(|e| e.height_drag.is_some());
    if let Some(pos) = response.hover_pos()
        && !dragging
    {
        match part(pos) {
            Some(HeightDrag::Both { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
            Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical),
            None => {}
        }
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = pointer
    {
        let drag = match part(pos) {
            Some(HeightDrag::Both { from, .. }) => {
                level_at(app, rect, aspect, &area, pos).map(|grab| HeightDrag::Both { grab, from })
            }
            other => other,
        };
        if let Some(edit) = &mut app.area_edit {
            edit.height_drag = drag;
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = pointer
        && let Some(z) = level_at(app, rect, aspect, &area, pos)
        && let Some(drag) = app.area_edit.as_ref().and_then(|e| e.height_drag)
    {
        // Snap to a storey level close on screen.
        let snap = |z: f64| -> f64 {
            let storeys = app
                .area_edit
                .as_ref()
                .map(|e| e.storeys.clone())
                .unwrap_or_default();
            let y = level_y(app, rect, aspect, &area, z);
            storeys
                .iter()
                .map(|(_, level)| level - STOREY_MARGIN)
                .filter_map(|level| {
                    let ly = level_y(app, rect, aspect, &area, level)?;
                    let dy = (ly - y?).abs();
                    (dy < STOREY_SNAP).then_some((level, dy))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(z, |(level, _)| level)
        };
        let next = match drag {
            HeightDrag::Bottom => [snap(z).min(heights[1] - MIN_SIDE), heights[1]],
            HeightDrag::Top => [heights[0], snap(z).max(heights[0] + MIN_SIDE)],
            HeightDrag::Both { grab, from } => {
                let shift = snap(from[0] + (z - grab)) - from[0];
                [from[0] + shift, from[1] + shift]
            }
        };
        if let Some(edit) = &mut app.area_edit
            && let Some(area) = &mut edit.draft
        {
            area.heights = Some(next);
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(edit) = &mut app.area_edit {
            edit.height_drag = None;
        }
        app.update_area_preview();
    }
}

/// The side view: storey levels, and the area's heights as a band.
fn draw_side(app: &OpenCheckApp, ui: &egui::Ui, rect: Rect, aspect: f32) {
    let Some(edit) = &app.area_edit else {
        return;
    };
    let Some(area) = edit.draft else {
        return;
    };
    let painter = ui.painter_at(rect);
    let storey_color = Color32::from_rgb(90, 95, 104);
    for (name, level) in &edit.storeys {
        let Some(y) = level_y(app, rect, aspect, &area, *level) else {
            continue;
        };
        let stroke = Stroke::new(1.0, storey_color.gamma_multiply(0.7));
        let mut x = rect.left();
        while x < rect.right() {
            painter.line_segment(
                [Pos2::new(x, y), Pos2::new((x + 6.0).min(rect.right()), y)],
                stroke,
            );
            x += 10.0;
        }
        painter.text(
            Pos2::new(rect.left() + 8.0, y - 3.0),
            egui::Align2::LEFT_BOTTOM,
            name,
            egui::FontId::proportional(12.0),
            storey_color,
        );
    }
    let heights = area.heights.unwrap_or_else(|| app.scene_heights());
    let origin = app.federation.origin();
    let (u, _) = area.axes();
    let c = DVec2::from_array(area.center);
    let ends = [c - u * area.half[0], c + u * area.half[0]];
    let corner = |p: DVec2, z: f64| {
        crate::viewport::project(
            app,
            rect,
            aspect,
            Vec3::new(
                (p.x - origin.x) as f32,
                (p.y - origin.y) as f32,
                (z - origin.z) as f32,
            ),
        )
    };
    let corners: Vec<Pos2> = [
        corner(ends[0], heights[0]),
        corner(ends[1], heights[0]),
        corner(ends[1], heights[1]),
        corner(ends[0], heights[1]),
    ]
    .into_iter()
    .flatten()
    .collect();
    if corners.len() != 4 {
        return;
    }
    painter.add(egui::Shape::convex_polygon(
        corners.clone(),
        area_color().gamma_multiply(0.10),
        Stroke::NONE,
    ));
    for (a, b) in [(0, 1), (3, 2)] {
        painter.line_segment([corners[a], corners[b]], Stroke::new(5.0, area_halo()));
        painter.line_segment([corners[a], corners[b]], Stroke::new(2.5, area_color()));
        let mid = corners[a] + (corners[b] - corners[a]) * 0.5;
        painter.circle_filled(mid, 6.5, Color32::WHITE);
        painter.circle_stroke(mid, 6.5, Stroke::new(2.5, area_color()));
    }
    for (a, b) in [(0, 3), (1, 2)] {
        painter.line_segment([corners[a], corners[b]], Stroke::new(1.5, area_color()));
    }
    let label = if area.heights.is_some() {
        format!(
            "{} to {}",
            level_name(&edit.storeys, heights[0]),
            level_name(&edit.storeys, heights[1])
        )
    } else {
        "All heights".to_owned()
    };
    painter.text(
        corners[2] + Vec2::new(0.0, -6.0),
        egui::Align2::RIGHT_BOTTOM,
        label,
        egui::FontId::proportional(13.0),
        area_color(),
    );
}

/// A height as text: the storey it lies at, or metres.
fn level_name(storeys: &[(String, f64)], z: f64) -> String {
    match storeys
        .iter()
        .find(|(_, level)| (level - STOREY_MARGIN - z).abs() < 0.01)
    {
        Some((name, _)) => name.clone(),
        None => format!("{z:.2} m"),
    }
}

/// The area outline and the toolbar of the top view.
pub fn draw(app: &mut OpenCheckApp, ui: &mut egui::Ui, rect: Rect, aspect: f32) {
    let Some(edit) = &app.area_edit else {
        return;
    };
    if edit.top_camera.is_some() {
        draw_side(app, ui, rect, aspect);
        toolbar(app, ui, rect);
        return;
    }
    let plane_z = edit.plane_z;
    if let Some(area) = edit.draft {
        let corners: Vec<Pos2> = area
            .corners()
            .iter()
            .filter_map(|c| to_screen(app, rect, aspect, *c, plane_z))
            .collect();
        if corners.len() == 4 {
            let painter = ui.painter_at(rect);
            painter.add(egui::Shape::convex_polygon(
                corners.clone(),
                area_color().gamma_multiply(0.14),
                Stroke::new(5.0, area_halo()),
            ));
            painter.add(egui::Shape::closed_line(
                corners.clone(),
                Stroke::new(2.5, area_color()),
            ));
            for side in 0..4 {
                let mid = corners[side] + (corners[(side + 1) % 4] - corners[side]) * 0.5;
                painter.circle_filled(mid, 6.5, Color32::WHITE);
                painter.circle_stroke(mid, 6.5, Stroke::new(2.5, area_color()));
            }
            let size = format!("{:.1} × {:.1} m", area.half[0] * 2.0, area.half[1] * 2.0);
            painter.text(
                corners[2] + Vec2::new(0.0, -6.0),
                egui::Align2::RIGHT_BOTTOM,
                size,
                egui::FontId::proportional(13.0),
                area_color(),
            );
        }
    }
    toolbar(app, ui, rect);
}

fn toolbar(app: &mut OpenCheckApp, ui: &mut egui::Ui, view: Rect) {
    let Some(edit) = &app.area_edit else {
        return;
    };
    let (aligning, has_draft, counts) = (edit.aligning, edit.draft.is_some(), edit.counts);
    let side = edit.top_camera.is_some();
    let storeys: Vec<String> = edit.storeys.iter().map(|(n, _)| n.clone()).collect();
    let heights = edit.draft.and_then(|a| a.heights);
    // The storeys the heights run between, when set by storey.
    let current_range = heights.and_then(|[bottom, top]| {
        let from = edit
            .storeys
            .iter()
            .position(|(_, l)| (l - STOREY_MARGIN - bottom).abs() < 0.01)?;
        let to = edit
            .storeys
            .iter()
            .position(|(_, l)| (l - STOREY_MARGIN - top).abs() < 0.01)
            .map_or(edit.storeys.len() - 1, |i| i.saturating_sub(1));
        Some((from, to))
    });
    let mut range: Option<Option<(usize, usize)>> = None;
    let bar = Rect::from_center_size(
        Pos2::new(view.center().x, view.top() + 40.0),
        Vec2::new(view.width().min(820.0) - 20.0, 62.0),
    );
    let mut action = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        theme::floating_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  Edit view", icon::CROP)).strong());
                if ui
                    .selectable_label(!side, "Top view")
                    .on_hover_text("Draw the area in plan")
                    .clicked()
                    && side
                {
                    action = Some("top");
                }
                if ui
                    .add_enabled(has_draft, egui::Button::selectable(side, "Side view"))
                    .on_hover_text("Limit the area's heights, e.g. to some storeys")
                    .on_disabled_hover_text("Draw the area in the top view first")
                    .clicked()
                    && !side
                {
                    action = Some("side");
                }
                let hint = if side {
                    "Drag the bottom and top lines; they snap to the storeys"
                } else if aligning {
                    "Click an element: the view and the area turn along it"
                } else if has_draft {
                    "Drag inside to move, drag the edges to resize, or draw a new rectangle"
                } else {
                    "Drag a rectangle around the area to keep"
                };
                ui.label(RichText::new(hint).small().color(theme::text_muted()));
            });
            ui.horizontal(|ui| {
                if side {
                    if !storeys.is_empty() {
                        let (mut from, mut to) = current_range.unwrap_or((0, storeys.len() - 1));
                        ui.label("From");
                        let before = (from, to);
                        storey_combo(ui, "area_from", &storeys, &mut from);
                        ui.label("to");
                        storey_combo(ui, "area_to", &storeys, &mut to);
                        if (from, to) != before {
                            range = Some(Some((from, to)));
                        }
                    }
                    if ui
                        .add_enabled(heights.is_some(), egui::Button::new("All heights"))
                        .clicked()
                    {
                        range = Some(None);
                    }
                } else if ui
                    .selectable_label(aligning, format!("{}  Align to element", icon::ANGLE))
                    .on_hover_text("Turn the view and the area along an element, e.g. a wall")
                    .clicked()
                {
                    action = Some("align");
                }
                if ui
                    .add_enabled(
                        has_draft,
                        egui::Button::new(format!("{}  Clear", icon::ERASER)),
                    )
                    .clicked()
                {
                    action = Some("clear");
                }
                if has_draft {
                    ui.label(
                        RichText::new(format!("Keeps {} of {} elements", counts.0, counts.1))
                            .small()
                            .color(theme::text_muted()),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        action = Some("cancel");
                    }
                    let save = egui::Button::new(
                        RichText::new(format!("{}  Save", icon::CHECK))
                            .strong()
                            .color(theme::on_accent()),
                    )
                    .fill(theme::accent());
                    if ui
                        .add(save)
                        .on_hover_text(if has_draft {
                            "Show and check only this area"
                        } else {
                            "Without a rectangle: show and check everything"
                        })
                        .clicked()
                    {
                        action = Some("save");
                    }
                });
            });
        });
    });
    match action {
        Some("align") => {
            if let Some(edit) = &mut app.area_edit {
                edit.aligning = !edit.aligning;
            }
        }
        Some("clear") => {
            if let Some(edit) = &mut app.area_edit {
                edit.draft = None;
            }
            app.update_area_preview();
        }
        Some("cancel") => app.cancel_area_edit(),
        Some("save") => app.save_area_edit(),
        Some("top") => app.set_side_view(false),
        Some("side") => app.set_side_view(true),
        _ => {}
    }
    if let Some(range) = range {
        app.set_storey_range(range);
    }
}

fn storey_combo(ui: &mut egui::Ui, id: &str, storeys: &[String], index: &mut usize) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(storeys.get(*index).map_or("", String::as_str))
        .show_ui(ui, |ui| {
            for (i, name) in storeys.iter().enumerate() {
                ui.selectable_value(index, i, name);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_follow_the_screen_axes() {
        let r = rect_from_corners(DVec2::new(0.0, 0.0), DVec2::new(10.0, 4.0), 0.0);
        assert_eq!(r.center, [5.0, 2.0]);
        assert_eq!(r.half, [5.0, 2.0]);
        // Turned 90°: the screen's x runs along world y.
        let r = rect_from_corners(
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 4.0),
            std::f64::consts::FRAC_PI_2,
        );
        assert!((r.half[0] - 2.0).abs() < 1e-9 && (r.half[1] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn dragging_a_side_keeps_the_opposite_one() {
        let area = WorkArea {
            center: [0.0, 0.0],
            half: [5.0, 2.0],
            angle: 0.0,
            heights: None,
        };
        // Pull the +u side (x = 5) out to x = 9: the -u side stays at -5.
        let moved = move_side(&area, 1, DVec2::new(9.0, 0.0));
        assert!((moved.half[0] - 7.0).abs() < 1e-9);
        assert!((moved.center[0] - 2.0).abs() < 1e-9);
        assert_eq!(moved.half[1], 2.0);
    }
}
