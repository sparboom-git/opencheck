//! Markers for findings in the 3D view: a circle where each clash is, for
//! a walk through the model or an ordinary look around. Only clashes in
//! sight are shown, within a distance of the eye (walking) or of the
//! point looked at (orbiting). Hovering shows a card, a click opens the
//! finding in a window with the same actions as in the Check tab.
//!
//! The layer works on [`MarkerSource`]s: anything with a point and the
//! elements it is about. For now the sources are the clashes the Check
//! tab's filters let through.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use egui_phosphor::regular as icon;
use glam::Vec3;
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::camera::Ray;
use crate::render::DisplayMode;
use crate::results_ui::{self, MarkerSource, MarkerState};

/// Radius of a marker's dot and of the ring around it, points.
const RADIUS: f32 = 6.0;
const RING: f32 = 12.0;
/// How close the pointer must be to a marker to hover or click it.
const REACH: f32 = 14.0;
/// Sight tests per frame; the rest follow in the next frames.
const RAYS_PER_FRAME: usize = 60;
/// Something this close in front of the clash point does not hide it.
const SLACK: f32 = 0.05;

pub struct MarkerLayer {
    pub on: bool,
    /// Only markers this close are shown, metres.
    pub distance: f32,
    /// Only the findings ticked in the Check tab, at any distance.
    pub ticked_only: bool,
    /// Whether each finding was in sight, and for which camera position.
    sight: FxHashMap<String, (bool, u64)>,
    generation: u64,
    camera: [f32; 5],
    /// The point looked at when orbiting.
    centre: Option<Vec3>,
    /// Markers drawn in the last frame, for hover and clicks.
    drawn: Vec<(String, Pos2)>,
}

impl Default for MarkerLayer {
    fn default() -> Self {
        Self {
            on: false,
            distance: 15.0,
            ticked_only: false,
            sight: FxHashMap::default(),
            generation: 0,
            camera: [f32::NAN; 5],
            centre: None,
            drawn: Vec::new(),
        }
    }
}

impl MarkerLayer {
    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.drawn.clear();
    }

    /// The marker under the pointer, if any.
    pub fn hit(&self, pos: Pos2) -> Option<String> {
        if !self.on {
            return None;
        }
        self.drawn
            .iter()
            .map(|(id, p)| (id, p.distance(pos)))
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id.clone())
    }
}

/// Opens the finding of a clicked marker. Coming from the whole model, the
/// model around it stays in view; while reviewing findings, the rest of the
/// model stays as it was chosen (shown, ghosted or hidden).
pub fn open(app: &mut OpenCheckApp, id: &str) {
    let reviewing = app.view.focus.is_some();
    app.show_finding(id, false);
    // Hidden, once chosen, stays until the reviewer changes it.
    if !reviewing && !app.view.show_context && !app.view.hide_context {
        app.view.show_context = true;
        app.view.modes_dirty = true;
    }
    app.finding_window_open = true;
}

/// Picks or lets go of the finding of a marker clicked with Ctrl or Shift,
/// to act on several at once. The ticks in the list are left alone.
pub fn pick(app: &mut OpenCheckApp, id: &str) {
    // The clash opened with a plain click is the first of the picks.
    if app.checking.picked.is_empty()
        && let Some(open_one) = app.checking.selected.clone()
        && app.checking.find(&open_one).is_some()
    {
        let same = open_one == id;
        app.checking.picked.push(open_one);
        if same {
            open(app, id);
            return;
        }
    }
    let picked = &mut app.checking.picked;
    if let Some(at) = picked.iter().position(|p| p == id) {
        picked.remove(at);
    } else {
        picked.push(id.to_owned());
    }
    open(app, id);
}

/// Works out which markers are in sight and draws them, with the card of
/// the one under the pointer.
pub fn draw(
    app: &mut OpenCheckApp,
    ui: &egui::Ui,
    rect: Rect,
    aspect: f32,
    response: &egui::Response,
) {
    if !app.markers.on {
        return;
    }
    let walking = app.nav.walk != crate::app::Walk::Off;
    let eye = app.camera.eye;
    let key = [eye.x, eye.y, eye.z, app.camera.yaw, app.camera.pitch];
    if key != app.markers.camera {
        app.markers.camera = key;
        app.markers.generation += 1;
        app.markers.centre = if walking {
            None
        } else {
            // The point straight ahead, or the middle of the scene.
            let ray = crate::viewport::pointer_ray(app, rect, aspect, rect.center());
            Some(
                app.view
                    .pick(&app.federation, &ray)
                    .map_or_else(|| centre_ahead(app, &ray), |h| h.point),
            )
        };
    }
    let reference = app.markers.centre.unwrap_or(eye);
    let distance = app.markers.distance;

    // Candidates: near enough (or ticked), on screen, both elements drawn.
    let ticked: Option<rustc_hash::FxHashSet<&str>> = app
        .markers
        .ticked_only
        .then(|| app.checking.marked.iter().map(String::as_str).collect());
    let mut near: Vec<(MarkerSource, Pos2, f32)> = results_ui::marker_sources(app)
        .into_iter()
        .filter(|m| {
            app.checking.picked.contains(&m.id)
                || match &ticked {
                    Some(ticked) => ticked.contains(m.id.as_str()),
                    None => m.point.distance(reference) <= distance,
                }
        })
        .filter(|m| app.view.is_visible_point(m.point))
        .filter(|m| {
            m.elements
                .iter()
                .all(|e| app.view.display_mode(*e) != DisplayMode::Hidden)
        })
        .filter_map(|m| {
            let p = crate::viewport::project(app, rect, aspect, m.point)?;
            let in_front = (m.point - eye).dot(app.camera.forward()) > 0.0;
            (in_front && rect.contains(p)).then(|| {
                let d = m.point.distance(eye);
                (m, p, d)
            })
        })
        .collect();
    near.sort_by(|a, b| a.2.total_cmp(&b.2));

    // Sight tests, nearest first, a few per frame.
    let generation = app.markers.generation;
    let mut budget = RAYS_PER_FRAME;
    let mut pending = false;
    for (m, _, d) in &near {
        let fresh = app
            .markers
            .sight
            .get(&m.id)
            .is_some_and(|(_, g)| *g == generation);
        if fresh {
            continue;
        }
        if budget == 0 {
            pending = true;
            break;
        }
        budget -= 1;
        let ray = Ray {
            origin: eye,
            dir: (m.point - eye).normalize_or_zero(),
        };
        let blocked = app
            .view
            .blocked(&app.federation, &ray, (d - SLACK).max(0.0), &m.elements);
        app.markers
            .sight
            .insert(m.id.clone(), (!blocked, generation));
    }
    if pending {
        ui.ctx().request_repaint();
    }

    let painter = ui.painter_at(rect);
    let selected = app.checking.selected.clone();
    let mut drawn = Vec::new();
    for (m, p, _) in &near {
        // Until tested, a marker keeps what it was last time.
        if !app.markers.sight.get(&m.id).is_some_and(|(seen, _)| *seen) {
            continue;
        }
        paint_marker(&painter, *p, m.color, m.state);
        if selected.as_deref() == Some(m.id.as_str()) || app.checking.picked.contains(&m.id) {
            paint_sight(&painter, *p, m.color);
        }
        drawn.push((m.id.clone(), *p));
    }
    app.markers.drawn = drawn;

    // The card of the marker under the pointer.
    let Some(pos) = response.hover_pos().filter(|_| !response.dragged()) else {
        return;
    };
    let Some(id) = app.markers.hit(pos) else {
        return;
    };
    if let Some((_, p)) = app.markers.drawn.iter().find(|(i, _)| *i == id) {
        painter.circle_stroke(*p, RING + 3.0, Stroke::new(2.0, Color32::WHITE));
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    egui::Area::new(egui::Id::new("marker_card"))
        .fixed_pos(pos + egui::vec2(16.0, 16.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(320.0);
                results_ui::marker_card(app, ui, &id);
            });
        });
}

/// A point straight ahead at the depth of the scene's middle.
fn centre_ahead(app: &OpenCheckApp, ray: &Ray) -> Vec3 {
    let depth = (app.scene_bounds.center() - ray.origin)
        .dot(app.camera.forward())
        .max(1.0);
    ray.origin + ray.dir * depth
}

/// A dot with a ring around it, in the category's colour, with a dark
/// edge so it stands out against the model. The state shows in the dot:
/// filled when new, white with a tick when decided, with a flag when in
/// an issue.
fn paint_marker(painter: &egui::Painter, p: Pos2, color: Color32, state: MarkerState) {
    let shade = Color32::from_black_alpha(110);
    painter.circle_stroke(p, RING, Stroke::new(5.0, shade));
    painter.circle_stroke(p, RING, Stroke::new(3.0, color));
    painter.circle_filled(p, RADIUS + 1.5, shade);
    match state {
        MarkerState::New => {
            painter.circle_filled(p, RADIUS, color);
        }
        MarkerState::Decided | MarkerState::Issue => {
            painter.circle_filled(p, RADIUS + 1.0, Color32::WHITE);
            let glyph = if state == MarkerState::Issue {
                icon::FLAG
            } else {
                icon::CHECK
            };
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(10.0),
                color,
            );
        }
    }
}

/// The chosen finding: a sight in the category's colour around its marker,
/// which stays readable inside it.
fn paint_sight(painter: &egui::Painter, p: Pos2, color: Color32) {
    let shade = Color32::from_black_alpha(120);
    for (stroke, c) in [(6.0, shade), (3.0, color)] {
        painter.circle_stroke(p, RING + 6.0, Stroke::new(stroke, c));
        for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
            painter.line_segment(
                [p + d * (RING + 2.0), p + d * (RING + 16.0)],
                Stroke::new(stroke, c),
            );
        }
    }
}
