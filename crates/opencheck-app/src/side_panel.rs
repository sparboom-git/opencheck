//! A tab's left panel that can hide itself: always shown, or a bar at the
//! left edge from which the panel comes out over the 3D view while the mouse
//! is on it. The pin in the panel's heading switches between the two; the
//! choice is the user's, per tab, and is remembered.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

use crate::app::OpenCheckApp;
use crate::checking_ui::panel_frame;
use crate::theme;

/// Width of the bar a hidden panel leaves at the edge.
const BAR_WIDTH: f32 = 22.0;
/// Seconds the mouse may be off the panel before it goes back.
const GRACE: f64 = 0.35;
/// The same after a menu of the panel, which reaches outside it.
const AFTER_MENU: f64 = 1.5;

pub struct SidePanel {
    /// Names the panel to egui.
    id: &'static str,
    /// Settings key of `autohide`.
    setting: &'static str,
    /// Width when first shown, and when out over the 3D view.
    width: f32,
    pub autohide: bool,
    /// With `autohide`: the panel is out now.
    out: bool,
    /// When the mouse left the panel that is out (egui's clock), and how
    /// long it may stay away before the panel goes back.
    away: Option<(f64, f64)>,
}

impl SidePanel {
    /// The panel as the user left it.
    pub fn saved(id: &'static str, setting: &'static str, width: f32) -> Self {
        Self {
            id,
            setting,
            width,
            autohide: crate::settings::get(setting).and_then(|v| v.as_bool()) == Some(true),
            out: false,
            away: None,
        }
    }

    /// The pin, for the panel's heading.
    pub fn pin(&mut self, ui: &mut egui::Ui) {
        let pinned = !self.autohide;
        if ui
            .add(egui::Button::new(icon::PUSH_PIN).selected(pinned))
            .on_hover_text(if pinned {
                "The panel is always shown. Click to hide it when the mouse is elsewhere; it \
                 comes out again with the mouse on the bar at the left edge."
            } else {
                "The panel hides when the mouse is elsewhere. Click to keep it shown."
            })
            .clicked()
        {
            self.autohide = pinned;
            self.out = true;
            crate::settings::set(self.setting, serde_json::Value::Bool(pinned));
        }
    }
}

/// Shows the panel `which` picks from the app, with `contents` in it.
pub fn show(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    which: fn(&mut OpenCheckApp) -> &mut SidePanel,
    contents: impl FnOnce(&mut OpenCheckApp, &mut egui::Ui),
) {
    let (id, width, autohide) = {
        let panel = which(app);
        (panel.id, panel.width, panel.autohide)
    };
    if !autohide {
        egui::Panel::left(id)
            .resizable(true)
            .default_size(width)
            .min_size(280.0)
            .frame(panel_frame())
            .show(ui, |ui| contents(app, ui));
        return;
    }
    // Hidden: a bar at the edge. The panel comes out over the 3D view, so
    // the view keeps its size.
    let bar = egui::Panel::left(egui::Id::new((id, "bar")))
        .resizable(false)
        .exact_size(BAR_WIDTH)
        .frame(
            egui::Frame::new()
                .fill(theme::panel())
                .stroke(egui::Stroke::new(1.0, theme::border())),
        )
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(icon::CARET_DOUBLE_RIGHT).color(theme::text_muted()));
            });
        })
        .response
        .rect;
    let ctx = ui.ctx().clone();
    let pointer = ctx.pointer_latest_pos();
    if pointer.is_some_and(|p| bar.contains(p)) {
        which(app).out = true;
    }
    if !which(app).out {
        return;
    }
    let frame = panel_frame();
    let margin = frame.total_margin().sum();
    let area = egui::Area::new(egui::Id::new((id, "out")))
        // Under the dialogs and the menus, over the 3D view.
        .order(egui::Order::Middle)
        .fixed_pos(bar.right_top())
        .show(&ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width - margin.x);
                ui.set_height(bar.height() - margin.y);
                contents(app, ui);
            });
        })
        .response
        .rect;
    // It goes back a moment after the mouse has left it; not under a menu of
    // its own, while something in it is dragged or while a field is typed
    // in. A menu reaches outside the panel, so after one the mouse gets
    // longer to come back.
    let keep = bar.union(area).expand(16.0);
    let now = ctx.input(|i| i.time);
    let busy = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
    let panel = which(app);
    if egui::Popup::is_any_open(&ctx) {
        panel.away = Some((now, AFTER_MENU));
    } else if busy || pointer.is_none_or(|p| keep.contains(p)) {
        panel.away = None;
    } else {
        let (since, grace) = *panel.away.get_or_insert((now, GRACE));
        if now - since >= grace {
            panel.out = false;
            panel.away = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(grace - (now - since)));
        }
    }
}
