//! The 3D view: input handling, the GPU paint callback and the tools drawn
//! on top of it (section handle, measurements, floating toolbars).

use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};
use eframe::egui_wgpu;
use egui_phosphor::regular as icon;
use glam::{Vec2 as GVec2, Vec3};

use crate::app::OpenCheckApp;
use crate::camera::Ray;
use crate::measure::{self, MeasureKind, Measurement};
use crate::render::{CameraUniform, ViewportCallback};
use crate::scene::PickHit;
use crate::snap::{Candidates, Snap, SnapKind, ray_segment_closest};
use crate::theme;

const ORBIT_SPEED: f32 = 0.006;
const ZOOM_SPEED: f32 = 0.0018;
const LASER: Color32 = Color32::from_rgb(230, 40, 60);

pub fn show(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let aspect = rect.width() / rect.height().max(1.0);
    app.nav.aspect = aspect;

    paint_background(ui, rect);

    if app.federation.is_empty() {
        empty_state(app, ui, rect, &response);
        return;
    }

    handle_navigation(app, ui, rect, &response, aspect);
    if app.measure.active {
        handle_measuring(app, ui, rect, &response, aspect);
    } else {
        handle_selection(app, ui, rect, &response, aspect);
    }
    handle_keyboard(app, ui, &response);

    app.camera.update_clip_planes(&app.scene_bounds);
    let uniform = CameraUniform {
        view_proj: app.camera.view_proj(aspect).to_cols_array_2d(),
        eye: app.camera.eye.extend(1.0).to_array(),
        // A horizontal cut is a box that is unbounded except at the top.
        section_min: [-1e9, -1e9, -1e9, 0.0],
        section_max: [1e9, 1e9, app.view.section.map_or(1e9, |s| s.height), 0.0],
        params: [
            if app.view.section.is_some() { 1.0 } else { 0.0 },
            0.12,
            0.0,
            0.0,
        ],
    };
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        ViewportCallback { camera: uniform },
    ));

    if let (Some(pivot), true) = (app.nav.pivot, response.dragged_by(PointerButton::Primary))
        && let Some(p) = project(app, rect, aspect, pivot)
    {
        ui.painter().circle_filled(p, 4.5, theme::ACCENT);
        ui.painter()
            .circle_stroke(p, 8.0, Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.5)));
    }

    draw_measurements(app, ui, rect, aspect);
    draw_focus_marker(app, ui, rect, aspect);
    section_handle(app, ui, rect, aspect);
    storey_bar(app, ui, rect);
    tool_bar(app, ui, rect);
    if app.measure.active {
        measure_hint(app, ui, rect);
    }
    error_toasts(app, ui, rect);
    drop_overlay(ui, rect);
}

fn paint_background(ui: &egui::Ui, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), theme::SKY_TOP);
    mesh.colored_vertex(rect.right_top(), theme::SKY_TOP);
    mesh.colored_vertex(rect.right_bottom(), theme::SKY_BOTTOM);
    mesh.colored_vertex(rect.left_bottom(), theme::SKY_BOTTOM);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
}

fn empty_state(app: &mut OpenCheckApp, ui: &mut egui::Ui, rect: Rect, response: &egui::Response) {
    let center = rect.center();
    let painter = ui.painter();
    painter.text(
        center - Vec2::new(0.0, 44.0),
        Align2::CENTER_CENTER,
        icon::FILE_ARROW_UP,
        FontId::proportional(54.0),
        theme::ACCENT,
    );
    let message = if app.is_busy() {
        "Loading…"
    } else {
        "Drop IFC files or a project here"
    };
    painter.text(
        center + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        message,
        FontId::proportional(22.0),
        theme::TEXT,
    );
    painter.text(
        center + Vec2::new(0.0, 44.0),
        Align2::CENTER_CENTER,
        "or click anywhere to browse. Several files are combined into one federated model.",
        FontId::proportional(14.0),
        theme::TEXT_MUTED,
    );
    if response.clicked() && !app.is_busy() {
        app.open_file_dialog(ui.ctx());
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    error_toasts(app, ui, rect);
    drop_overlay(ui, rect);
}

/// Pointer position as normalised device coordinates of the viewport.
fn ndc(rect: Rect, pos: Pos2) -> GVec2 {
    GVec2::new(
        (pos.x - rect.left()) / rect.width() * 2.0 - 1.0,
        1.0 - (pos.y - rect.top()) / rect.height() * 2.0,
    )
}

fn pointer_ray(app: &OpenCheckApp, rect: Rect, aspect: f32, pos: Pos2) -> Ray {
    app.camera.ray(ndc(rect, pos), aspect)
}

fn pick_at(app: &OpenCheckApp, rect: Rect, aspect: f32, pos: Pos2) -> Option<PickHit> {
    app.view
        .pick(&app.federation, &pointer_ray(app, rect, aspect, pos))
}

fn project(app: &OpenCheckApp, rect: Rect, aspect: f32, p: Vec3) -> Option<Pos2> {
    let clip = app.camera.view_proj(aspect) * p.extend(1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Pos2::new(
        rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
        rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
    ))
}

/// Projects a convex polygon, first clipping away the part behind the camera
/// so a plane that passes under the viewer is still drawn.
fn project_polygon(app: &OpenCheckApp, rect: Rect, aspect: f32, polygon: &[Vec3]) -> Vec<Pos2> {
    const MIN_W: f32 = 1e-3;
    let view_proj = app.camera.view_proj(aspect);
    let clip: Vec<glam::Vec4> = polygon.iter().map(|p| view_proj * p.extend(1.0)).collect();
    let mut kept = Vec::with_capacity(clip.len() + 2);
    for (i, &current) in clip.iter().enumerate() {
        let next = clip[(i + 1) % clip.len()];
        if current.w >= MIN_W {
            kept.push(current);
        }
        if (current.w >= MIN_W) != (next.w >= MIN_W) {
            let t = (MIN_W - current.w) / (next.w - current.w);
            kept.push(current.lerp(next, t));
        }
    }
    kept.into_iter()
        .map(|c| {
            let ndc = c.truncate() / c.w;
            Pos2::new(
                rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
            )
        })
        .collect()
}

/// A point to anchor navigation on when the cursor is over empty space:
/// the same depth as the middle of the scene, along the cursor ray.
fn fallback_point(app: &OpenCheckApp, ray: &Ray) -> Vec3 {
    let center = app.scene_bounds.center();
    let depth = (center - ray.origin).dot(app.camera.forward()).max(1.0);
    ray.at(depth / ray.dir.dot(app.camera.forward()).max(0.05))
}

fn handle_navigation(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    aspect: f32,
) {
    let pointer = response.interact_pointer_pos().or(response.hover_pos());

    // Orbit with the left button, around the point that was under the cursor.
    if response.drag_started_by(PointerButton::Primary)
        && let Some(pos) = pointer
    {
        let ray = pointer_ray(app, rect, aspect, pos);
        let hit = app.view.pick(&app.federation, &ray);
        app.nav.pivot = Some(hit.map_or_else(|| fallback_point(app, &ray), |h| h.point));
    }
    if response.dragged_by(PointerButton::Primary)
        && let Some(pivot) = app.nav.pivot
    {
        let d = response.drag_delta();
        app.camera
            .orbit(pivot, d.x * ORBIT_SPEED, -d.y * ORBIT_SPEED);
    }

    // Pan with the right or middle button; the grabbed point follows the cursor.
    let pan_buttons = [PointerButton::Secondary, PointerButton::Middle];
    if pan_buttons.iter().any(|b| response.drag_started_by(*b))
        && let Some(pos) = pointer
    {
        let ray = pointer_ray(app, rect, aspect, pos);
        let point = app
            .view
            .pick(&app.federation, &ray)
            .map_or_else(|| fallback_point(app, &ray), |h| h.point);
        app.nav.pan_depth = Some((point - app.camera.eye).dot(app.camera.forward()).max(0.1));
    }
    if pan_buttons.iter().any(|b| response.dragged_by(*b)) {
        let d = response.drag_delta();
        let depth = app.nav.pan_depth.unwrap_or(10.0);
        app.camera.pan(GVec2::new(d.x, d.y), depth, rect.height());
    }

    // Zoom towards whatever is under the cursor.
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 300.0);
        if scroll.abs() > 0.0
            && let Some(pos) = response.hover_pos()
        {
            let ray = pointer_ray(app, rect, aspect, pos);
            let target = pick_at(app, rect, aspect, pos)
                .map_or_else(|| fallback_point(app, &ray), |h| h.point);
            app.camera
                .zoom_towards(target, (-scroll * ZOOM_SPEED).exp());
        }
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

fn handle_selection(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    aspect: f32,
) {
    let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
    if response.double_clicked() {
        if let Some(pos) = response.interact_pointer_pos()
            && let Some(hit) = pick_at(app, rect, aspect, pos)
        {
            app.select(Some(hit.element), false);
            app.fit_element(hit.element);
        }
    } else if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let hit = pick_at(app, rect, aspect, pos);
        app.select(hit.map(|h| h.element), additive);
    }

    // Hover highlight, only while the pointer is resting over the view.
    let hover = if response.hovered() && !response.dragged() {
        response.hover_pos().map(|pos| {
            if app.nav.last_pointer == Some(pos) {
                return app.view.hover;
            }
            app.nav.last_pointer = Some(pos);
            pick_at(app, rect, aspect, pos).map(|h| h.element)
        })
    } else {
        Some(None)
    };
    if let Some(hover) = hover
        && hover != app.view.hover
    {
        app.view.hover = hover;
        app.view.modes_dirty = true;
    }
    if app.view.hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.context_menu(|ui| {
        context_menu(app, ui);
    });
}

/// Laser reading under the cursor, point-to-point on Ctrl+click.
fn handle_measuring(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    aspect: f32,
) {
    if app.view.hover.take().is_some() {
        app.view.modes_dirty = true;
    }
    let hit = (response.hovered() && !response.dragged())
        .then(|| response.hover_pos())
        .flatten()
        .and_then(|pos| pick_at(app, rect, aspect, pos));
    let point_mode = ui.input(|i| i.modifiers.command) || app.measure.start.is_some();

    app.measure.live = None;
    app.measure.dangling = None;
    app.measure.snap = match (point_mode, hit, response.hover_pos()) {
        (true, Some(hit), Some(pos)) => Some(snap_point(app, rect, aspect, pos, &hit)),
        _ => None,
    };
    if let Some(hit) = hit {
        if point_mode {
            if let (Some(start), Some(snap)) = (app.measure.start, app.measure.snap) {
                app.measure.live = Some(Measurement {
                    a: start,
                    b: snap.point,
                    kind: MeasureKind::PointToPoint,
                });
            }
        } else {
            let beam = measure::laser_ray(hit.point, hit.normal);
            match app.view.pick(&app.federation, &beam) {
                Some(end) => {
                    app.measure.live = Some(Measurement {
                        a: hit.point,
                        b: end.point,
                        kind: MeasureKind::Laser,
                    });
                }
                None => app.measure.dangling = Some((hit.point, hit.normal)),
            }
        }
    }

    if response.clicked() {
        // Snap again exactly where the button went down; the hover state of
        // this frame may already count as a drag.
        let clicked_snap = response.interact_pointer_pos().and_then(|pos| {
            let hit = pick_at(app, rect, aspect, pos)?;
            Some(snap_point(app, rect, aspect, pos, &hit))
        });
        match (point_mode, clicked_snap, app.measure.start) {
            (true, Some(snap), None) => app.measure.start = Some(snap.point),
            (true, Some(snap), Some(start)) => {
                app.measure.pinned.push(Measurement {
                    a: start,
                    b: snap.point,
                    kind: MeasureKind::PointToPoint,
                });
                app.measure.start = None;
            }
            (false, _, _) => {
                if let Some(live) = app.measure.live {
                    app.measure.pinned.push(live);
                }
            }
            _ => {}
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    response.context_menu(|ui| {
        if ui
            .button(format!("{}  Clear measurements", icon::TRASH))
            .clicked()
        {
            app.measure.clear();
            ui.close();
        }
        if ui.button(format!("{}  Stop measuring", icon::X)).clicked() {
            app.measure.toggle();
            ui.close();
        }
    });
}

/// Screen distance within which corners and edges catch the cursor.
const SNAP_CORNER_PX: f32 = 14.0;
const SNAP_EDGE_PX: f32 = 9.0;

/// The point a point-to-point click lands on: a nearby corner, else a nearby
/// edge, else the surface under the cursor. Corners and edges of all elements
/// around the hit count, so junctions between elements snap too.
fn snap_point(
    app: &mut OpenCheckApp,
    rect: Rect,
    aspect: f32,
    cursor: Pos2,
    hit: &PickHit,
) -> Snap {
    let surface = Snap {
        point: hit.point,
        kind: SnapKind::Surface,
    };
    let ray = pointer_ray(app, rect, aspect, cursor);
    let depth = hit.point.distance(app.camera.eye);
    let world_per_pixel = 2.0 * depth * (app.camera.fov_y * 0.5).tan() / rect.height().max(1.0);
    let radius = SNAP_CORNER_PX * world_per_pixel * 1.5;

    let mut candidates = Candidates::new();
    for (model_id, transform, index) in app.view.elements_near(hit.point, radius) {
        let Some(model) = app.federation.get(model_id) else {
            continue;
        };
        for &mesh_index in &model.elements[index].meshes {
            let features = app
                .measure
                .edges
                .get(model_id, mesh_index, &model.meshes[mesh_index]);
            candidates.add(&features, &transform);
        }
    }

    let screen_distance =
        |p: Vec3| project(app, rect, aspect, p).map_or(f32::INFINITY, |s| s.distance(cursor));
    // Nearest candidates first; the first one the camera can actually see wins.
    let ranked = |points: Vec<Vec3>, max_px: f32| {
        let mut near: Vec<(f32, Vec3)> = points
            .into_iter()
            .filter(|p| app.view.is_visible_point(*p))
            .map(|p| (screen_distance(p), p))
            .filter(|(d, _)| *d <= max_px)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.into_iter()
            .take(8)
            .map(|(_, p)| p)
            .find(|p| in_sight(app, *p, world_per_pixel))
    };

    if let Some(point) = ranked(candidates.corners.clone(), SNAP_CORNER_PX) {
        return Snap {
            point,
            kind: SnapKind::Corner,
        };
    }
    let edge_points = candidates
        .edges
        .iter()
        .map(|[a, b]| ray_segment_closest(&ray, *a, *b).1)
        .collect();
    match ranked(edge_points, SNAP_EDGE_PX) {
        Some(point) => Snap {
            point,
            kind: SnapKind::Edge,
        },
        None => surface,
    }
}

/// Whether nothing blocks the line of sight from the camera to `p`.
/// Points on an edge sit exactly where surfaces meet, so a little slack
/// (a couple of pixels at that distance) is allowed.
fn in_sight(app: &OpenCheckApp, p: Vec3, world_per_pixel_at_hit: f32) -> bool {
    let eye = app.camera.eye;
    let distance = eye.distance(p);
    if distance < 1e-4 {
        return true;
    }
    let ray = Ray {
        origin: eye,
        dir: (p - eye) / distance,
    };
    let slack = (world_per_pixel_at_hit * 2.0).max(0.002);
    app.view
        .pick(&app.federation, &ray)
        .is_none_or(|h| h.distance >= distance - slack)
}

fn context_menu(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let has_selection = !app.view.selection.is_empty();
    let item = |ui: &mut egui::Ui, enabled: bool, label: String| {
        ui.add_enabled(enabled, egui::Button::new(label)).clicked()
    };
    if item(
        ui,
        has_selection,
        format!("{}  Zoom to selection", icon::CORNERS_OUT),
    ) {
        app.fit_selection();
        ui.close();
    }
    if item(
        ui,
        has_selection,
        format!("{}  Section above selection", icon::SCISSORS),
    ) {
        app.section_at_selection();
        ui.close();
    }
    if item(
        ui,
        has_selection,
        format!("{}  Isolate", icon::SELECTION_FOREGROUND),
    ) {
        app.isolate_selection();
        ui.close();
    }
    if item(ui, has_selection, format!("{}  Hide", icon::EYE_SLASH)) {
        app.hide_selection();
        ui.close();
    }
    ui.separator();
    if item(ui, true, format!("{}  Show all", icon::EYE)) {
        app.show_all();
        ui.close();
    }
}

fn handle_keyboard(app: &mut OpenCheckApp, ui: &egui::Ui, response: &egui::Response) {
    if ui.ctx().egui_wants_keyboard_input() && !response.has_focus() {
        return;
    }
    let pressed = |key: egui::Key| ui.input(|i| i.key_pressed(key) && !i.modifiers.command);
    let shift = ui.input(|i| i.modifiers.shift);
    if pressed(egui::Key::F) {
        app.fit_selection();
    }
    if pressed(egui::Key::Home) {
        app.fit_all();
    }
    if pressed(egui::Key::Escape) {
        if app.measure.start.is_some() {
            app.measure.start = None;
        } else if app.measure.active {
            app.measure.toggle();
        } else if app.view.selection.is_empty() {
            app.show_all();
        } else {
            app.select(None, false);
        }
    }
    if pressed(egui::Key::Delete) && app.measure.active {
        app.measure.clear();
    }
    if pressed(egui::Key::H) {
        if shift {
            app.show_all();
        } else {
            app.hide_selection();
        }
    }
    if pressed(egui::Key::I) {
        app.isolate_selection();
    }
    if pressed(egui::Key::X) {
        app.toggle_xray();
    }
    if pressed(egui::Key::C) {
        app.toggle_section();
    }
    if pressed(egui::Key::M) {
        app.measure.toggle();
    }

    // Walk with WASD / arrow keys; Q and E move down and up.
    let speed = ui.input(|i| {
        let step = i.stable_dt.min(0.1) * if i.modifiers.shift { 12.0 } else { 4.0 };
        let axis = |pos: &[egui::Key], neg: &[egui::Key]| {
            let p = pos.iter().any(|k| i.key_down(*k)) as i32;
            let n = neg.iter().any(|k| i.key_down(*k)) as i32;
            (p - n) as f32 * step
        };
        (
            axis(
                &[egui::Key::W, egui::Key::ArrowUp],
                &[egui::Key::S, egui::Key::ArrowDown],
            ),
            axis(
                &[egui::Key::D, egui::Key::ArrowRight],
                &[egui::Key::A, egui::Key::ArrowLeft],
            ),
            axis(&[egui::Key::E], &[egui::Key::Q]),
        )
    });
    if speed != (0.0, 0.0, 0.0) && !ui.input(|i| i.modifiers.command) {
        app.camera.walk(speed.0, speed.1, speed.2);
        ui.ctx().request_repaint();
    }
}

// ---- Section ---------------------------------------------------------------

/// The cut plane outline and the scissors handle that drags it up and down.
fn section_handle(app: &mut OpenCheckApp, ui: &mut egui::Ui, rect: Rect, aspect: f32) {
    let Some(section) = app.view.section else {
        return;
    };
    let painter = ui.painter_at(rect);

    // Outline of the plane over the extent of the model.
    let b = app.scene_bounds;
    let margin = b.size().max_element() * 0.05;
    let corners = [
        Vec3::new(b.min.x - margin, b.min.y - margin, section.height),
        Vec3::new(b.max.x + margin, b.min.y - margin, section.height),
        Vec3::new(b.max.x + margin, b.max.y + margin, section.height),
        Vec3::new(b.min.x - margin, b.max.y + margin, section.height),
    ];
    let points = project_polygon(app, rect, aspect, &corners);
    if points.len() >= 3 {
        painter.add(Shape::convex_polygon(
            points.clone(),
            theme::ACCENT.gamma_multiply(0.08),
            Stroke::NONE,
        ));
        painter.add(Shape::closed_line(
            points,
            Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.8)),
        ));
    }

    let anchor = section.anchor.with_z(section.height);
    let Some(center) = project(app, rect, aspect, anchor) else {
        return;
    };
    let handle = Rect::from_center_size(center, Vec2::splat(38.0));
    let response = ui.interact(
        handle,
        ui.id().with("section_handle"),
        Sense::click_and_drag(),
    );

    if response.dragged() {
        // How many pixels one metre up moves on screen at the handle.
        if let Some(above) = project(app, rect, aspect, anchor + Vec3::Z) {
            let per_metre = above - center;
            let len2 = per_metre.length_sq();
            if len2 > 1e-3 {
                let dz = response.drag_delta().dot(per_metre) / len2;
                if let Some(s) = &mut app.view.section {
                    s.height += dz;
                }
            }
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.double_clicked() {
        app.view.section = None;
    }

    let active = response.hovered() || response.dragged();
    painter.circle_filled(
        center,
        17.0,
        if active {
            theme::ACCENT
        } else {
            Color32::WHITE
        },
    );
    painter.circle_stroke(center, 17.0, Stroke::new(2.0, theme::ACCENT));
    painter.text(
        center,
        Align2::CENTER_CENTER,
        icon::SCISSORS,
        FontId::proportional(19.0),
        if active {
            Color32::WHITE
        } else {
            theme::ACCENT
        },
    );
    // Arrows hint that the handle moves up and down.
    for (dy, glyph) in [(-27.0, icon::CARET_UP), (27.0, icon::CARET_DOWN)] {
        painter.text(
            center + Vec2::new(0.0, dy),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(14.0),
            theme::ACCENT,
        );
    }
    let elevation = app.federation.origin().z + section.height as f64;
    label_box(
        &painter,
        center + Vec2::new(26.0, 0.0),
        Align2::LEFT_CENTER,
        &format!("Section  {elevation:+.3} m"),
        theme::ACCENT,
    );
    response.on_hover_text("Drag up or down to move the section. Double click to remove it.");
}

/// A target ring on the point of the finding under review.
fn draw_focus_marker(app: &OpenCheckApp, ui: &egui::Ui, rect: Rect, aspect: f32) {
    let Some(p) = app
        .focus_point()
        .and_then(|p| project(app, rect, aspect, p))
    else {
        return;
    };
    let painter = ui.painter_at(rect);
    let color = Color32::from_rgb(214, 40, 60);
    for (radius, width) in [(16.0, 4.0), (16.0, 2.0)] {
        let c = if width > 3.0 { Color32::WHITE } else { color };
        painter.circle_stroke(p, radius, Stroke::new(width, c));
    }
    for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
        painter.line_segment([p + d * 10.0, p + d * 24.0], Stroke::new(2.0, color));
    }
}

// ---- Measurements ------------------------------------------------------------

fn draw_measurements(app: &OpenCheckApp, ui: &egui::Ui, rect: Rect, aspect: f32) {
    let painter = ui.painter_at(rect);
    for m in &app.measure.pinned {
        draw_measurement(app, &painter, rect, aspect, m, theme::ACCENT);
    }
    if let Some(m) = &app.measure.live {
        let color = match m.kind {
            MeasureKind::Laser => LASER,
            MeasureKind::PointToPoint => theme::ACCENT,
        };
        draw_measurement(app, &painter, rect, aspect, m, color);
    }
    if let Some((point, normal)) = app.measure.dangling
        && let (Some(a), Some(b)) = (
            project(app, rect, aspect, point),
            project(app, rect, aspect, point + normal * 2.0),
        )
    {
        painter.add(Shape::dashed_line(
            &[a, b],
            Stroke::new(2.0, LASER),
            6.0,
            4.0,
        ));
        painter.circle_filled(a, 4.0, LASER);
        label_box(&painter, b, Align2::LEFT_CENTER, "no surface", LASER);
    }
    if let Some(start) = app.measure.start
        && let Some(p) = project(app, rect, aspect, start)
    {
        painter.circle_filled(p, 5.0, theme::ACCENT);
        painter.circle_stroke(p, 9.0, Stroke::new(1.5, theme::ACCENT));
    }
    if let Some(snap) = app.measure.snap
        && let Some(p) = project(app, rect, aspect, snap.point)
    {
        draw_snap_marker(&painter, p, snap.kind);
    }
}

/// CAD-style snap markers: square for a corner, diamond for an edge,
/// circle for a plain surface point.
fn draw_snap_marker(painter: &egui::Painter, p: Pos2, kind: SnapKind) {
    let color = Color32::from_rgb(20, 170, 90);
    let stroke = Stroke::new(2.0, color);
    let halo = Stroke::new(4.0, Color32::WHITE);
    let (label, shape) = match kind {
        SnapKind::Corner => {
            let r = Rect::from_center_size(p, Vec2::splat(13.0));
            (
                "Corner",
                Shape::closed_line(
                    vec![
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                    ],
                    stroke,
                ),
            )
        }
        SnapKind::Edge => {
            let d = 8.0;
            let points = vec![
                p + Vec2::new(0.0, -d),
                p + Vec2::new(d, 0.0),
                p + Vec2::new(0.0, d),
                p + Vec2::new(-d, 0.0),
            ];
            ("Edge", Shape::closed_line(points, stroke))
        }
        SnapKind::Surface => ("", Shape::circle_stroke(p, 6.0, stroke)),
    };
    let mut halo_shape = shape.clone();
    match &mut halo_shape {
        Shape::Path(path) => path.stroke = halo.into(),
        Shape::Circle(circle) => circle.stroke = halo,
        _ => {}
    }
    painter.add(halo_shape);
    painter.add(shape);
    painter.circle_filled(p, 2.0, color);
    if !label.is_empty() {
        label_box(
            painter,
            p + Vec2::new(12.0, -14.0),
            Align2::LEFT_BOTTOM,
            label,
            color,
        );
    }
}

fn draw_measurement(
    app: &OpenCheckApp,
    painter: &egui::Painter,
    rect: Rect,
    aspect: f32,
    m: &Measurement,
    color: Color32,
) {
    let (Some(a), Some(b)) = (
        project(app, rect, aspect, m.a),
        project(app, rect, aspect, m.b),
    ) else {
        return;
    };
    painter.line_segment([a, b], Stroke::new(3.0, Color32::WHITE));
    painter.line_segment([a, b], Stroke::new(1.8, color));
    for p in [a, b] {
        painter.circle_filled(p, 4.5, Color32::WHITE);
        painter.circle_filled(p, 3.2, color);
    }
    label_box(
        painter,
        a.lerp(b, 0.5),
        Align2::CENTER_CENTER,
        &m.label(),
        color,
    );
}

/// Text on a white rounded plate, readable on any background.
fn label_box(painter: &egui::Painter, pos: Pos2, align: Align2, text: &str, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), color);
    let rect = align
        .anchor_size(pos, galley.size())
        .expand2(Vec2::new(7.0, 4.0));
    painter.rect_filled(rect, 6.0, Color32::WHITE);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    painter.galley(rect.min + Vec2::new(7.0, 4.0), galley, color);
}

fn measure_hint(app: &OpenCheckApp, ui: &mut egui::Ui, view: Rect) {
    let text = if app.measure.start.is_some() {
        "Click the second point  ·  Esc cancels"
    } else {
        "Laser: point at a surface, click to keep  ·  Ctrl+click: point to point  ·  Del clears"
    };
    let rect = Rect::from_center_size(
        Pos2::new(view.center().x, view.bottom() - 86.0),
        Vec2::new(560.0, 34.0),
    );
    overlay(ui, rect, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::RULER).color(LASER));
            ui.label(RichText::new(text).small());
        });
    });
}

// ---- Floating toolbars -------------------------------------------------------

/// A row of icon buttons inside a floating card.
fn icon_button(ui: &mut egui::Ui, glyph: &str, active: bool, tooltip: &str) -> egui::Response {
    let text =
        RichText::new(glyph)
            .size(19.0)
            .color(if active { theme::ACCENT } else { theme::TEXT });
    ui.add(
        egui::Button::selectable(active, text)
            .min_size(Vec2::splat(34.0))
            .corner_radius(7),
    )
    .on_hover_text(tooltip)
}

fn overlay(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        theme::floating_frame().show(ui, add);
    });
}

fn tool_bar(app: &mut OpenCheckApp, ui: &mut egui::Ui, view: Rect) {
    let rect = Rect::from_center_size(
        Pos2::new(view.center().x, view.bottom() - 34.0),
        Vec2::new(470.0, 46.0),
    );
    overlay(ui, rect, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icon_button(ui, icon::HOUSE, false, "Fit everything in view (Home)").clicked() {
                app.fit_all();
            }
            let has_selection = !app.view.selection.is_empty();
            if icon_button(ui, icon::CORNERS_OUT, false, "Zoom to selection (F)").clicked() {
                app.fit_selection();
            }
            ui.separator();
            let isolated = app.view.isolated.is_some();
            if icon_button(
                ui,
                icon::SELECTION_FOREGROUND,
                isolated,
                "Isolate selection (I)",
            )
            .clicked()
            {
                if isolated {
                    app.view.isolated = None;
                    app.view.modes_dirty = true;
                } else if has_selection {
                    app.isolate_selection();
                }
            }
            if icon_button(ui, icon::EYE_SLASH, false, "Hide selection (H)").clicked() {
                app.hide_selection();
            }
            if icon_button(ui, icon::EYE, false, "Show everything (Shift+H)").clicked() {
                app.show_all();
            }
            ui.separator();
            if icon_button(ui, icon::CUBE_TRANSPARENT, app.view.xray, "X-ray (X)").clicked() {
                app.toggle_xray();
            }
            let sectioned = app.view.section.is_some();
            if icon_button(
                ui,
                icon::SCISSORS,
                sectioned,
                "Section: cut just above the selected object (C)",
            )
            .clicked()
            {
                app.toggle_section();
            }
            if icon_button(ui, icon::RULER, app.measure.active, "Measure (M)").clicked() {
                app.measure.toggle();
            }
            if icon_button(ui, icon::SQUARE_HALF, app.view.show_spaces, "Show spaces").clicked() {
                app.view.show_spaces = !app.view.show_spaces;
                app.view.modes_dirty = true;
            }
            let filtered =
                !app.view.hidden.is_empty() || isolated || app.view.storey_filter.is_some();
            if filtered {
                ui.label(RichText::new("filtered").small().color(theme::TEXT_MUTED));
            }
        });
    });
}

fn storey_bar(app: &mut OpenCheckApp, ui: &mut egui::Ui, view: Rect) {
    let storeys = app.view.storey_names(&app.federation);
    if storeys.is_empty() {
        return;
    }
    let rect = Rect::from_min_size(
        view.left_top() + Vec2::new(12.0, 12.0),
        Vec2::new(260.0, 44.0),
    );
    overlay(ui, rect, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::STACK).size(18.0));
            let current = app
                .view
                .storey_filter
                .clone()
                .unwrap_or_else(|| "All storeys".to_owned());
            let mut choice = app.view.storey_filter.clone();
            egui::ComboBox::from_id_salt("storey_filter")
                .selected_text(current)
                .width(190.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "All storeys");
                    for name in storeys.iter().rev() {
                        ui.selectable_value(&mut choice, Some(name.clone()), name);
                    }
                });
            if choice != app.view.storey_filter {
                app.view.storey_filter = choice;
                app.view.modes_dirty = true;
            }
        });
    });
}

fn error_toasts(app: &mut OpenCheckApp, ui: &mut egui::Ui, view: Rect) {
    let mut dismissed = None;
    for (i, message) in app.errors.iter().enumerate() {
        let rect = Rect::from_center_size(
            Pos2::new(view.center().x, view.top() + 34.0 + i as f32 * 52.0),
            Vec2::new(520.0, 44.0),
        );
        overlay(ui, rect, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::WARNING).size(18.0).color(theme::DANGER));
                ui.label(message);
                if ui.small_button(icon::X).clicked() {
                    dismissed = Some(i);
                }
            });
        });
    }
    if let Some(i) = dismissed {
        app.errors.remove(i);
    }
}

fn drop_overlay(ui: &egui::Ui, rect: Rect) {
    let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
    if hovering {
        let painter = ui.painter();
        painter.rect_filled(rect.shrink(8.0), 12.0, theme::ACCENT.gamma_multiply(0.12));
        painter.rect_stroke(
            rect.shrink(8.0),
            12.0,
            Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Release to add models",
            FontId::proportional(22.0),
            Color32::from_rgb(20, 60, 140),
        );
    }
}
