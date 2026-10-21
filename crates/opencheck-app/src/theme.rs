//! Look and feel: "Grafit". Graphite chrome on a paper ground, 1 px lines
//! instead of shadows, and colour reserved for data (clash sides,
//! criticality, status). Text is IBM Plex Sans, values IBM Plex Mono.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle,
    Vec2,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The switchable colours: one palette per theme. Clash sides, the 3D
/// view's background and model colours are the same in both.
struct Palette {
    ink: Color32,
    ink_3: Color32,
    line_strong: Color32,
    line: Color32,
    sunken: Color32,
    paper: Color32,
    panel: Color32,
    danger: Color32,
    solved: Color32,
    c2: Color32,
    /// Text on an accent-filled button.
    on_accent: Color32,
}

/// "Grafit": graphite on paper.
const LIGHT: Palette = Palette {
    ink: Color32::from_rgb(23, 25, 28),
    ink_3: Color32::from_rgb(107, 111, 118),
    line_strong: Color32::from_rgb(196, 196, 188),
    line: Color32::from_rgb(220, 220, 214),
    sunken: Color32::from_rgb(236, 236, 232),
    paper: Color32::from_rgb(244, 244, 241),
    panel: Color32::WHITE,
    danger: Color32::from_rgb(180, 35, 24),
    solved: Color32::from_rgb(30, 122, 76),
    c2: Color32::from_rgb(168, 98, 0),
    on_accent: Color32::WHITE,
};

/// The same design dark: graphite panels, light ink, the colours a step
/// brighter so they still read.
const DARK: Palette = Palette {
    ink: Color32::from_rgb(228, 228, 222),
    ink_3: Color32::from_rgb(150, 154, 160),
    line_strong: Color32::from_rgb(82, 85, 90),
    line: Color32::from_rgb(58, 61, 66),
    sunken: Color32::from_rgb(40, 42, 46),
    paper: Color32::from_rgb(31, 33, 36),
    panel: Color32::from_rgb(24, 25, 28),
    danger: Color32::from_rgb(236, 96, 84),
    solved: Color32::from_rgb(82, 196, 132),
    c2: Color32::from_rgb(230, 160, 52),
    on_accent: Color32::from_rgb(20, 21, 24),
};

static DARK_THEME: AtomicBool = AtomicBool::new(false);

pub fn is_dark() -> bool {
    DARK_THEME.load(Ordering::Relaxed)
}

/// `%APPDATA%/OpenCheck/settings.json`: the user's choices for the app
/// itself, such as the theme.
fn settings_file() -> std::path::PathBuf {
    crate::library::folder()
        .parent()
        .map(|p| p.join("settings.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("settings.json"))
}

/// The theme chosen last time; light when nothing is saved.
pub fn saved_dark() -> bool {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("dark_theme").and_then(|d| d.as_bool()))
        .unwrap_or(false)
}

/// Remembers the theme for the next start.
pub fn save_dark(dark: bool) {
    let path = settings_file();
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    value["dark_theme"] = serde_json::Value::Bool(dark);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    );
}

fn p() -> &'static Palette {
    if is_dark() { &DARK } else { &LIGHT }
}

/// Switches between the light and the dark theme.
pub fn set_dark(ctx: &egui::Context, dark: bool) {
    DARK_THEME.store(dark, Ordering::Relaxed);
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

// Neutrals: the whole interface.
pub fn ink() -> Color32 {
    p().ink
}

/// Selection, primary buttons and active tools. Graphite, not a hue: colour
/// in this app always means something.
pub fn accent() -> Color32 {
    p().ink
}
/// Text on an accent-filled button.
pub fn on_accent() -> Color32 {
    p().on_accent
}
pub fn accent_soft() -> Color32 {
    p().sunken
}
pub fn text() -> Color32 {
    p().ink
}
pub fn text_muted() -> Color32 {
    p().ink_3
}
pub fn border() -> Color32 {
    p().line
}
pub fn panel() -> Color32 {
    p().panel
}
pub fn surface() -> Color32 {
    p().paper
}
pub fn danger() -> Color32 {
    p().danger
}
/// Issues and findings solved by a model update, and accepted findings.
pub fn solved() -> Color32 {
    p().solved
}

/// Criticality C1, C2 and C3. C2 is a dark amber so it also reads as text.
pub fn c1() -> Color32 {
    p().ink_3
}
pub fn c2() -> Color32 {
    p().c2
}
pub fn c3() -> Color32 {
    p().danger
}

/// The two sides of a clash.
pub const CLASH_A: Color32 = Color32::from_rgb(232, 101, 15);
pub const CLASH_B: Color32 = Color32::from_rgb(31, 95, 209);

/// Background of the 3D view, top to bottom. Flat: no gradient.
pub const SKY_TOP: Color32 = Color32::from_rgb(228, 229, 225);
pub const SKY_BOTTOM: Color32 = SKY_TOP;

/// Distinct, readable colours for model chips, assigned in load order.
pub const MODEL_COLORS: [Color32; 8] = [
    Color32::from_rgb(31, 95, 209),
    Color32::from_rgb(232, 101, 15),
    Color32::from_rgb(30, 122, 76),
    Color32::from_rgb(130, 70, 175),
    Color32::from_rgb(180, 35, 24),
    Color32::from_rgb(20, 130, 145),
    Color32::from_rgb(168, 98, 0),
    Color32::from_rgb(107, 111, 118),
];

/// Font family of IBM Plex Sans SemiBold, for headings and selected rows.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn install(ctx: &egui::Context, dark: bool) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "plex-sans",
            &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            "plex-sans-semibold",
            &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        ),
        (
            "plex-mono",
            &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // Plex first; egui's own fonts stay behind as fallback for missing glyphs.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex-sans".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".into());
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.insert(
        bold(),
        std::iter::once("plex-sans-semibold".to_owned())
            .chain(fallback.into_iter().skip(1))
            .collect(),
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Icons are used inside bold text too.
    fonts
        .families
        .get_mut(&bold())
        .unwrap()
        .push("phosphor".into());
    ctx.set_fonts(fonts);

    for (theme, palette) in [(egui::Theme::Light, &LIGHT), (egui::Theme::Dark, &DARK)] {
        ctx.style_mut_of(theme, |style| style_for(style, palette));
    }
    set_dark(ctx, dark);
}

fn style_for(style: &mut egui::Style, c: &Palette) {
    {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(15.0, bold())),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 6.0);
        spacing.button_padding = Vec2::new(12.0, 5.0);
        spacing.interact_size.y = 28.0;
        spacing.menu_margin = Margin::same(8);
        spacing.window_margin = Margin::same(12);
        spacing.indent = 12.0;

        let v = &mut style.visuals;
        v.panel_fill = c.panel;
        v.window_fill = c.panel;
        v.extreme_bg_color = c.panel;
        v.faint_bg_color = c.paper;
        v.code_bg_color = c.sunken;
        v.override_text_color = Some(c.ink);
        v.hyperlink_color = c.ink;
        v.warn_fg_color = c.c2;
        v.error_fg_color = c.danger;
        v.selection.bg_fill = c.line;
        v.selection.stroke = Stroke::new(1.0, c.ink);
        v.window_corner_radius = CornerRadius::same(6);
        v.menu_corner_radius = CornerRadius::same(6);
        v.window_shadow = soft_shadow();
        v.popup_shadow = soft_shadow();
        v.window_stroke = Stroke::new(1.0, c.line);

        let radius = CornerRadius::same(4);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.line);
        v.widgets.inactive.weak_bg_fill = c.panel;
        v.widgets.inactive.bg_fill = c.sunken;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, c.line_strong);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.ink);
        v.widgets.hovered.weak_bg_fill = c.sunken;
        v.widgets.hovered.bg_fill = c.line;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, c.ink_3);
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, c.ink);
        v.widgets.active.weak_bg_fill = c.line;
        v.widgets.active.bg_fill = c.line;
        v.widgets.active.bg_stroke = Stroke::new(1.0, c.ink);
        v.widgets.active.fg_stroke = Stroke::new(1.5, c.ink);
        v.widgets.open.weak_bg_fill = c.sunken;
        v.widgets.open.bg_stroke = Stroke::new(1.0, c.ink_3);
    }
}

/// Only for what floats over the 3D view and for popups; panels use lines.
pub fn soft_shadow() -> Shadow {
    Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(24),
    }
}

/// White card used for the floating toolbars on top of the 3D view.
pub fn floating_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(panel())
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(4, 4))
        .stroke(Stroke::new(1.0, border()))
        .shadow(soft_shadow())
}

/// One colour per discipline, used for swatches in the checking results.
pub fn discipline_color(d: opencheck_core::Discipline) -> Color32 {
    use opencheck_core::Discipline::*;
    match d {
        Architectural => Color32::from_rgb(232, 140, 40),
        Structural => Color32::from_rgb(140, 95, 60),
        Mechanical => Color32::from_rgb(60, 140, 220),
        Plumbing => Color32::from_rgb(40, 170, 160),
        Electrical => Color32::from_rgb(240, 196, 30),
        FireProtection => Color32::from_rgb(220, 60, 60),
        Landscape => Color32::from_rgb(80, 160, 70),
        Infrastructure => Color32::from_rgb(120, 120, 140),
        Unknown => Color32::from_rgb(160, 160, 160),
    }
}
