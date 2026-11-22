//! Preset avatars for accounts: small 8×8 pixel-art heads drawn by the launcher itself.
//!
//! They are simplified original pixel art, not files taken from the game.
//! Every picture is described by 8 rows of 8 characters; each character is a key in the palette.
//! The preview sheet `assets/avatars-preview.png` is rendered from exactly these tables.

use eframe::egui;

pub struct Preset {
    /// Stored in config.json (`avatar` field of an account). Never change an existing id.
    pub id: &'static str,
    /// Name shown under the picture.
    pub name: &'static str,
    rows: [&'static str; 8],
    palette: &'static [(char, u32)],
}

impl Preset {
    /// 8×8 pixels as RGBA bytes.
    pub fn rgba(&self) -> [u8; 8 * 8 * 4] {
        let mut out = [255u8; 8 * 8 * 4];
        for (y, row) in self.rows.iter().enumerate() {
            for (x, ch) in row.chars().take(8).enumerate() {
                let rgb = self
                    .palette
                    .iter()
                    .find(|(k, _)| *k == ch)
                    .map(|(_, v)| *v)
                    .unwrap_or(0xFF00FF);
                let i = (y * 8 + x) * 4;
                out[i] = (rgb >> 16) as u8;
                out[i + 1] = (rgb >> 8) as u8;
                out[i + 2] = rgb as u8;
            }
        }
        out
    }

    pub fn image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([8, 8], &self.rgba())
    }
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "axolotl",
        name: "Axolotl",
        rows: [
            "PPPPPPPP",
            "RPPPPPPR",
            "rPKPPKPr",
            "RPPPPPPR",
            "rPBPPBPr",
            "RPPMMPPR",
            "rPPPPPPr",
            "PPPPPPPP",
        ],
        palette: &[
            ('P', 0xF4B6D0),
            ('R', 0xD9608F),
            ('r', 0xE98AB0),
            ('K', 0x1A1420),
            ('B', 0xEF9DBF),
            ('M', 0xC2507C),
        ],
    },
    Preset {
        id: "cat",
        name: "Cat",
        rows: [
            "dOoOOoOd",
            "dOOOOOOd",
            "OOoOOoOO",
            "OYKOOKYO",
            "OOOPPOOO",
            "OWOMMOWO",
            "OOWWWWOO",
            "OOWWWWOO",
        ],
        palette: &[
            ('O', 0xE08A33),
            ('o', 0xB86A22),
            ('d', 0x7A4519),
            ('Y', 0x9BD34A),
            ('K', 0x111111),
            ('P', 0xF2A0A8),
            ('W', 0xF5EBDD),
            ('M', 0x8A4A2A),
        ],
    },
    Preset {
        id: "wolf",
        name: "Wolf",
        rows: [
            "dGGGGGGd",
            "dGgGGgGd",
            "GGGGGGGG",
            "GeKGGKeG",
            "GGWWWWGG",
            "GWWnnWWG",
            "GWWMMWWG",
            "GGWWWWGG",
        ],
        palette: &[
            ('G', 0x9A9EA5),
            ('g', 0x6E727A),
            ('d', 0x4A4D54),
            ('e', 0xD9A441),
            ('K', 0x1A1A1E),
            ('W', 0xD8D8DC),
            ('n', 0x1A1A1E),
            ('M', 0x4A4D54),
        ],
    },
    Preset {
        id: "creeper",
        name: "Creeper",
        rows: [
            "GgGGGgGG",
            "GGGgGGgG",
            "GKKGGKKG",
            "GKKGGKKG",
            "GGGKKGGG",
            "GGKKKKGG",
            "GGKKKKGG",
            "GgKGGKgG",
        ],
        palette: &[('G', 0x5BA84B), ('g', 0x3F7F36), ('K', 0x0E1A0C)],
    },
    Preset {
        id: "zombie",
        name: "Zombie",
        rows: [
            "HHHHHHHH",
            "HHHHHHHH",
            "ZZZZZZZZ",
            "ZKKZZKKZ",
            "ZZZnnZZZ",
            "ZZZZZZZZ",
            "ZZMMMMZZ",
            "ZzZZZZzZ",
        ],
        palette: &[
            ('H', 0x2F4A2B),
            ('Z', 0x5E8F48),
            ('z', 0x4C7A3A),
            ('K', 0x2A2A5C),
            ('n', 0x486F37),
            ('M', 0x2F4A2B),
        ],
    },
    Preset {
        id: "skeleton",
        name: "Skeleton",
        rows: [
            "LLLLLLLL",
            "LgLLLLgL",
            "LLLLLLLL",
            "LKKLLKKL",
            "LKKLLKKL",
            "LLLddLLL",
            "LLKKKKLL",
            "LLLLLLLL",
        ],
        palette: &[
            ('L', 0xCFCFCF),
            ('g', 0xB0B0B0),
            ('K', 0x2B2B2B),
            ('d', 0x8E8E8E),
        ],
    },
    Preset {
        id: "enderman",
        name: "Enderman",
        rows: [
            "KKkKKKkK",
            "KKKKKKKK",
            "KkKKKKkK",
            "KKKKKKKK",
            "KPPKKPPK",
            "KWPKKWPK",
            "KKKKKKKK",
            "KKkKKkKK",
        ],
        palette: &[
            ('K', 0x12101A),
            ('k', 0x1E1A2A),
            ('P', 0xD04BFF),
            ('W', 0xF2C9FF),
        ],
    },
    Preset {
        id: "pig",
        name: "Pig",
        rows: [
            "PPPPPPPP",
            "PpPPPPpP",
            "PPPPPPPP",
            "PWKPPKWP",
            "PPSSSSPP",
            "PPSnnSPP",
            "PPSSSSPP",
            "PPPPPPPP",
        ],
        palette: &[
            ('P', 0xEC9FA6),
            ('p', 0xD98890),
            ('W', 0xFFFFFF),
            ('K', 0x3A2F6B),
            ('S', 0xD9767F),
            ('n', 0x8A3E48),
        ],
    },
    Preset {
        id: "villager",
        name: "Villager",
        rows: [
            "BBBBBBBB",
            "BBBBBBBB",
            "BbbbbbbB",
            "SWGSSGWS",
            "SSSNNSSS",
            "SSSNNSSS",
            "SSSSSSSS",
            "SSSSSSSS",
        ],
        palette: &[
            ('B', 0x6B4A32),
            ('b', 0x4A3322),
            ('S', 0xC08E6C),
            ('W', 0xFFFFFF),
            ('G', 0x3F8F4A),
            ('N', 0x9E6B4E),
        ],
    },
    Preset {
        id: "slime",
        name: "Slime",
        rows: [
            "gggggggg",
            "gGGGGGGg",
            "gGGGGGGg",
            "gGKGGKGg",
            "gGGGGGGg",
            "gGGKKGGg",
            "gGGGGGGg",
            "gggggggg",
        ],
        palette: &[('g', 0x8FD67A), ('G', 0x5DB04C), ('K', 0x1F3B1A)],
    },
    Preset {
        id: "blaze",
        name: "Blaze",
        rows: [
            "YYOYYOYY",
            "YOYYYYOY",
            "YYYYYYYY",
            "YKKYYKKY",
            "YKKYYKKY",
            "YYYYYYYY",
            "YOKKKKOY",
            "YYOYYOYY",
        ],
        palette: &[('Y', 0xF7C531), ('O', 0xE8892B), ('K', 0x3A2410)],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for p in PRESETS {
            assert!(ids.insert(p.id), "duplicate id {}", p.id);
            assert!(!p.id.is_empty() && p.id.chars().all(|c| c.is_ascii_lowercase()), "bad id {}", p.id);
            assert!(!p.name.is_empty());
            for row in p.rows {
                assert_eq!(row.chars().count(), 8, "{}: row must have 8 pixels", p.id);
                for ch in row.chars() {
                    assert!(
                        p.palette.iter().any(|(k, _)| *k == ch),
                        "{}: character '{}' is not in the palette",
                        p.id,
                        ch
                    );
                }
            }
        }
    }

    #[test]
    fn rgba_is_opaque_and_sized() {
        for p in PRESETS {
            let px = p.rgba();
            assert_eq!(px.len(), 256);
            assert!(px.chunks(4).all(|c| c[3] == 255));
        }
    }
}
