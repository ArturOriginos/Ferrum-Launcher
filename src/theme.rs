//! Оформление: несколько палитр фона и любой акцентный цвет.
use eframe::egui::{self, Color32, FontFamily, FontId, Rounding, Stroke, TextStyle, Vec2, Visuals};
use std::sync::RwLock;

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
}

pub const PALETTES: [Palette; 5] = [
    Palette {
        name: "Графит",
        bg: c(20, 22, 24),
        sidebar: c(15, 17, 19),
        card: c(29, 33, 36),
        surface: c(39, 44, 48),
        hover: c(50, 56, 61),
        border: c(55, 62, 67),
        text: c(232, 228, 220),
        muted: c(142, 149, 148),
    },
    Palette {
        name: "Полночь",
        bg: c(14, 17, 27),
        sidebar: c(10, 12, 20),
        card: c(22, 27, 41),
        surface: c(31, 38, 58),
        hover: c(42, 51, 76),
        border: c(46, 56, 84),
        text: c(228, 232, 242),
        muted: c(130, 140, 165),
    },
    Palette {
        name: "Хвоя",
        bg: c(15, 22, 20),
        sidebar: c(10, 16, 14),
        card: c(22, 32, 29),
        surface: c(30, 44, 40),
        hover: c(40, 58, 52),
        border: c(46, 66, 60),
        text: c(226, 236, 230),
        muted: c(128, 150, 142),
    },
    Palette {
        name: "Бордо",
        bg: c(24, 15, 18),
        sidebar: c(17, 10, 13),
        card: c(37, 23, 28),
        surface: c(52, 33, 40),
        hover: c(68, 44, 52),
        border: c(76, 50, 58),
        text: c(240, 228, 230),
        muted: c(160, 132, 138),
    },
    Palette {
        name: "Светлая",
        bg: c(238, 236, 232),
        sidebar: c(228, 225, 219),
        card: c(250, 249, 246),
        surface: c(236, 233, 227),
        hover: c(224, 220, 212),
        border: c(206, 201, 192),
        text: c(34, 36, 40),
        muted: c(110, 114, 118),
    },
];

static CURRENT: RwLock<Palette> = RwLock::new(PALETTES[0]);

pub fn set_palette(idx: usize) {
    *CURRENT.write().unwrap() = PALETTES[idx.min(PALETTES.len() - 1)];
}
fn cur() -> Palette {
    *CURRENT.read().unwrap()
}
pub fn bg() -> Color32 {
    cur().bg
}
pub fn sidebar() -> Color32 {
    cur().sidebar
}
pub fn card() -> Color32 {
    cur().card
}
pub fn surface() -> Color32 {
    cur().surface
}
pub fn hover() -> Color32 {
    cur().hover
}
pub fn border() -> Color32 {
    cur().border
}
pub fn text() -> Color32 {
    cur().text
}
pub fn muted() -> Color32 {
    cur().muted
}
pub fn is_light() -> bool {
    let b = cur().bg;
    (b.r() as u32 + b.g() as u32 + b.b() as u32) / 3 > 128
}

pub const DANGER: Color32 = c(232, 93, 93);
pub const WARN: Color32 = c(235, 180, 70);

pub const ACCENTS: [(&str, Color32); 5] = [
    ("Окалина", c(224, 102, 44)),
    ("Патина", c(63, 181, 160)),
    ("Латунь", c(214, 164, 65)),
    ("Кобальт", c(77, 141, 255)),
    ("Вереск", c(167, 123, 216)),
];

pub fn accent(idx: usize) -> Color32 {
    ACCENTS[idx.min(ACCENTS.len() - 1)].1
}

/// Цвет текста на акцентной кнопке: тёмный на светлых акцентах, белый на тёмных.
pub fn on_accent(col: Color32) -> Color32 {
    let l = 0.299 * col.r() as f32 + 0.587 * col.g() as f32 + 0.114 * col.b() as f32;
    if l > 150.0 {
        c(20, 22, 24)
    } else {
        Color32::WHITE
    }
}

pub fn apply(ctx: &egui::Context, accent: Color32) {
    let p = cur();
    let mut v = if is_light() { Visuals::light() } else { Visuals::dark() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_rounding = Rounding::same(16.0);
    v.extreme_bg_color = if is_light() { Color32::WHITE } else { p.sidebar };
    v.faint_bg_color = p.card;
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.linear_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, accent);

    let r = Rounding::same(10.0);
    v.widgets.noninteractive.bg_fill = p.card;
    v.widgets.noninteractive.weak_bg_fill = p.card;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = p.surface;
    v.widgets.inactive.weak_bg_fill = p.surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = accent.linear_multiply(0.6);
    v.widgets.active.weak_bg_fill = accent.linear_multiply(0.6);
    v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = p.surface;
    v.widgets.open.weak_bg_fill = p.surface;
    v.widgets.open.rounding = r;

    let mut style = (*ctx.style()).clone();
    style.visuals = v;
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(14.0, 8.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.slider_width = 260.0;
    style.text_styles.insert(TextStyle::Heading, FontId::new(30.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Small, FontId::new(12.5, FontFamily::Proportional));
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_color_on_accent() {
        assert_eq!(on_accent(c(224, 102, 44)), Color32::WHITE);
        assert_eq!(on_accent(c(214, 164, 65)), c(20, 22, 24));
    }

    #[test]
    fn light_palette_is_detected() {
        set_palette(4);
        assert!(is_light());
        set_palette(0);
        assert!(!is_light());
    }
}
