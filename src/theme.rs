//! Appearance: several background palettes and any accent color.
use eframe::egui::{self, Color32, FontFamily, FontId, Rounding, Stroke, TextStyle, Vec2, Visuals};
use std::sync::atomic::{AtomicU8, Ordering};
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

pub const PALETTES: [Palette; 10] = [
    Palette {
        name: "Graphite",
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
        name: "Midnight",
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
        name: "Pine",
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
        name: "Burgundy",
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
        name: "Light",
        bg: c(238, 236, 232),
        sidebar: c(228, 225, 219),
        card: c(250, 249, 246),
        surface: c(236, 233, 227),
        hover: c(224, 220, 212),
        border: c(206, 201, 192),
        text: c(34, 36, 40),
        muted: c(110, 114, 118),
    },
    Palette {
        name: "AMOLED",
        bg: c(0, 0, 0),
        sidebar: c(0, 0, 0),
        card: c(14, 14, 16),
        surface: c(26, 26, 30),
        hover: c(38, 38, 44),
        border: c(44, 44, 50),
        text: c(236, 236, 236),
        muted: c(136, 136, 144),
    },
    Palette {
        name: "Ocean",
        bg: c(10, 22, 30),
        sidebar: c(6, 15, 21),
        card: c(16, 34, 46),
        surface: c(23, 48, 64),
        hover: c(32, 64, 84),
        border: c(36, 72, 92),
        text: c(224, 238, 244),
        muted: c(120, 150, 166),
    },
    Palette {
        name: "Sakura",
        bg: c(250, 238, 242),
        sidebar: c(243, 226, 232),
        card: c(255, 250, 252),
        surface: c(248, 232, 238),
        hover: c(240, 218, 226),
        border: c(228, 200, 210),
        text: c(58, 36, 46),
        muted: c(140, 110, 122),
    },
    Palette {
        name: "Sand",
        bg: c(244, 238, 226),
        sidebar: c(234, 226, 210),
        card: c(252, 248, 240),
        surface: c(240, 232, 216),
        hover: c(228, 218, 198),
        border: c(212, 200, 178),
        text: c(52, 44, 32),
        muted: c(128, 116, 96),
    },
    Palette {
        name: "Neon",
        bg: c(16, 12, 28),
        sidebar: c(11, 8, 20),
        card: c(26, 20, 46),
        surface: c(38, 30, 66),
        hover: c(52, 42, 88),
        border: c(62, 50, 104),
        text: c(236, 230, 255),
        muted: c(150, 138, 190),
    },
];

static RADIUS: AtomicU8 = AtomicU8::new(1);

/// 0 — sharp corners, 1 — soft (default), 2 — round.
pub fn set_radius(level: u32) {
    RADIUS.store(level.min(2) as u8, Ordering::Relaxed);
}

/// Scales the corner radius to the chosen style.
pub fn round(x: f32) -> f32 {
    match RADIUS.load(Ordering::Relaxed) {
        0 => x * 0.25,
        2 => x * 1.5,
        _ => x,
    }
}

static CURRENT: RwLock<Palette> = RwLock::new(PALETTES[0]);

pub fn set_palette(idx: usize) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = PALETTES[idx.min(PALETTES.len() - 1)];
}
fn cur() -> Palette {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
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

pub const ACCENTS: [(&str, Color32); 9] = [
    ("Rust", c(224, 102, 44)),
    ("Patina", c(63, 181, 160)),
    ("Brass", c(214, 164, 65)),
    ("Cobalt", c(77, 141, 255)),
    ("Heather", c(167, 123, 216)),
    ("Rose", c(255, 94, 150)),
    ("Lime", c(132, 214, 60)),
    ("Cyan", c(40, 200, 230)),
    ("Crimson", c(220, 56, 72)),
];

pub fn accent(idx: usize) -> Color32 {
    ACCENTS[idx.min(ACCENTS.len() - 1)].1
}

/// Text color on an accent button: dark on light accents, white on dark ones.
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
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_rounding = Rounding::same(round(16.0));
    v.extreme_bg_color = if is_light() { Color32::WHITE } else { p.sidebar };
    v.faint_bg_color = p.card;
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.linear_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0_f32, accent);

    let r = Rounding::same(round(10.0));
    v.widgets.noninteractive.bg_fill = p.card;
    v.widgets.noninteractive.weak_bg_fill = p.card;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = p.surface;
    v.widgets.inactive.weak_bg_fill = p.surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = accent.linear_multiply(0.6);
    v.widgets.active.weak_bg_fill = accent.linear_multiply(0.6);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text);
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
    fn light_palettes_are_detected() {
        // indices of the light palettes: Light (4), Sakura (7), Sand (8)
        for i in 0..PALETTES.len() {
            set_palette(i);
            assert_eq!(is_light(), matches!(i, 4 | 7 | 8), "{}", PALETTES[i].name);
        }
        set_palette(0);
    }

    #[test]
    fn corner_styles() {
        set_radius(0);
        assert_eq!(round(16.0), 4.0);
        set_radius(2);
        assert_eq!(round(16.0), 24.0);
        set_radius(1);
        assert_eq!(round(16.0), 16.0);
        set_radius(99);
        assert_eq!(round(16.0), 24.0);
        set_radius(1);
    }
}
