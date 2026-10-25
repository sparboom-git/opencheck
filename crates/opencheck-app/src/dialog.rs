//! The larger dialogs (Ruleset Manager, Where to look, BCF Live, Issue
//! report, Autorun) all open through [`show`], so they behave the same:
//!
//! - They open no larger than the screen and can be made smaller down to a
//!   modest minimum; content that does not fit scrolls instead of holding
//!   the window open (`Scroll`), or the dialog lays itself out to the width
//!   it gets (`Scroll::Own`).
//! - On the desktop a button in the title bar opens the dialog in a window
//!   of its own, which can be moved outside OpenCheck's window, e.g. onto a
//!   second screen; the same button puts it back. The choice is remembered.
//!
//! New dialogs must use this rather than `egui::Window` directly.

use eframe::egui::{self, RichText, Vec2};
use egui_phosphor::regular as icon;

/// How a dialog's content fits its window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// The window scrolls both ways: for forms and lists.
    Both,
    /// The dialog has its own scroll areas and fills the window it gets.
    Own,
}

pub struct Dialog<'a> {
    /// Stable id, also of the window position and the pop-out choice.
    pub id: &'a str,
    pub icon: &'a str,
    pub title: &'a str,
    pub size: Vec2,
    pub min: Vec2,
    pub scroll: Scroll,
}

/// Whether the dialog with this id is in a window of its own.
pub fn is_popped(ctx: &egui::Context, id: &str) -> bool {
    ctx.data_mut(|d| d.get_persisted::<bool>(popped_id(id)))
        .unwrap_or(false)
}

fn popped_id(id: &str) -> egui::Id {
    egui::Id::new(("dialog_popped", id))
}

fn set_popped(ctx: &egui::Context, id: &str, popped: bool) {
    ctx.data_mut(|d| d.insert_persisted(popped_id(id), popped));
}

/// Shows the dialog while `open`; the close button clears it. Returns the
/// layer of the in-app window (None when it is in a window of its own), so
/// a caller can bring it to the front.
pub fn show(
    ctx: &egui::Context,
    dialog: &Dialog,
    open: &mut bool,
    add: impl FnOnce(&mut egui::Ui),
) -> Option<egui::LayerId> {
    if !*open {
        return None;
    }
    if !crate::platform::IS_WEB && is_popped(ctx, dialog.id) {
        own_window(ctx, dialog, open, add);
        return None;
    }
    let screen = ctx.content_rect().size();
    // Never larger than the screen, never a minimum the screen cannot give.
    let size = dialog
        .size
        .min(screen - egui::vec2(40.0, 60.0))
        .max(egui::vec2(200.0, 150.0));
    let min = dialog.min.min(size);
    let title = RichText::new(format!("{}  {}", dialog.icon, dialog.title)).strong();
    let mut window = egui::Window::new(title)
        .id(egui::Id::new(dialog.id))
        .open(open)
        .default_size(size)
        .min_size(min)
        .max_size(screen - egui::vec2(16.0, 16.0))
        .constrain(true)
        .resizable(true)
        .collapsible(false);
    if dialog.scroll == Scroll::Both {
        window = window.scroll([true, true]);
    }
    let shown = window.show(ctx, add)?;
    if !crate::platform::IS_WEB {
        pop_out_button(ctx, dialog.id, shown.response.rect);
    }
    Some(shown.response.layer_id)
}

/// A small button in the window's title bar, left of the close button.
fn pop_out_button(ctx: &egui::Context, id: &str, rect: egui::Rect) {
    let height = ctx.global_style().spacing.interact_size.y + 6.0;
    let pos = egui::pos2(
        rect.right() - 58.0,
        rect.top() + (height - 22.0) / 2.0 + 2.0,
    );
    egui::Area::new(egui::Id::new(("dialog_pop", id)))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let button = egui::Button::new(RichText::new(icon::ARROW_SQUARE_OUT).size(15.0))
                .frame(false)
                .min_size(egui::vec2(22.0, 22.0));
            if ui
                .add(button)
                .on_hover_text("Open in its own window, which can be moved outside OpenCheck")
                .clicked()
            {
                set_popped(ctx, id, true);
            }
        });
}

/// The dialog in a window of its own (a separate viewport).
fn own_window(
    ctx: &egui::Context,
    dialog: &Dialog,
    open: &mut bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    let mut add = Some(add);
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{} · OpenCheck", dialog.title))
        .with_inner_size(dialog.size)
        .with_min_inner_size(dialog.min.min(dialog.size) * 0.7);
    let id = dialog.id;
    let scroll = dialog.scroll;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(("dialog", id)),
        builder,
        |ui, _class| {
            if ui.input(|i| i.viewport().close_requested()) {
                *open = false;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  {}", dialog.icon, dialog.title)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!("{}  Back into OpenCheck", icon::ARROW_SQUARE_IN))
                            .on_hover_text("Show the dialog inside OpenCheck's window again")
                            .clicked()
                        {
                            set_popped(ui.ctx(), id, false);
                        }
                    });
                });
                ui.separator();
                let Some(add) = add.take() else {
                    return;
                };
                match scroll {
                    Scroll::Both => {
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, add);
                    }
                    Scroll::Own => add(ui),
                }
            });
        },
    );
}
