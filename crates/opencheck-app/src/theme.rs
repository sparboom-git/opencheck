//! Look and feel: a calm light theme with large hit targets.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2,
};

pub const ACCENT: Color32 = Color32::from_rgb(43, 127, 246);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(226, 238, 254);
pub const TEXT: Color32 = Color32::from_rgb(32, 37, 46);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(112, 120, 134);
pub const BORDER: Color32 = Color32::from_rgb(226, 229, 235);
pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255);
pub const SURFACE: Color32 = Color32::from_rgb(246, 247, 249);
pub const DANGER: Color32 = Color32::from_rgb(214, 69, 65);

/// Background gradient of the 3D view, top to bottom.
pub const SKY_TOP: Color32 = Color32::from_rgb(214, 224, 236);
pub const SKY_BOTTOM: Color32 = Color32::from_rgb(244, 246, 249);

/// Distinct, readable colours for model chips, assigned in load order.
pub const MODEL_COLORS: [Color32; 8] = [
    Color32::from_rgb(43, 127, 246),
    Color32::from_rgb(232, 120, 40),
    Color32::from_rgb(46, 160, 98),
    Color32::from_rgb(170, 84, 214),
    Color32::from_rgb(214, 69, 65),
    Color32::from_rgb(30, 160, 180),
    Color32::from_rgb(200, 160, 30),
    Color32::from_rgb(110, 120, 140),
];

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 6.0);
        spacing.button_padding = Vec2::new(10.0, 5.0);
        spacing.interact_size.y = 26.0;
        spacing.menu_margin = Margin::same(8);
        spacing.window_margin = Margin::same(12);
        spacing.indent = 12.0;

        let v = &mut style.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = SURFACE;
        v.faint_bg_color = Color32::from_rgb(249, 250, 251);
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = soft_shadow();
        v.popup_shadow = soft_shadow();
        v.window_stroke = Stroke::new(1.0, BORDER);

        let radius = CornerRadius::same(6);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.weak_bg_fill = SURFACE;
        v.widgets.inactive.bg_fill = SURFACE;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(236, 239, 244);
        v.widgets.hovered.bg_fill = Color32::from_rgb(236, 239, 244);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.active.weak_bg_fill = ACCENT_SOFT;
        v.widgets.active.bg_fill = ACCENT_SOFT;
    });
}

pub fn soft_shadow() -> Shadow {
    Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(28),
    }
}

/// White rounded card used for the floating toolbars on top of the 3D view.
pub fn floating_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(6, 5))
        .stroke(Stroke::new(1.0, BORDER))
        .shadow(soft_shadow())
}
