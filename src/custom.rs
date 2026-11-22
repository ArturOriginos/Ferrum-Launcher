//! Custom images: interface background and launcher icon.
use crate::config::data_dir;
use anyhow::{anyhow, bail, Result};
use eframe::egui;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use std::{
    fs,
    path::{Path, PathBuf},
};

// ---------- Limits ----------

/// Icon: square, 128…2048 px, up to 5 MB.
const ICON_MAX_BYTES: u64 = 5 * 1024 * 1024;
const ICON_MIN: u32 = 128;
const ICON_MAX: u32 = 2048;
/// Background: 800×450 to 7680×4320 px, up to 10 MB.
const BG_MAX_BYTES: u64 = 10 * 1024 * 1024;
const BG_MIN: (u32, u32) = (800, 450);
const BG_MAX: (u32, u32) = (7680, 4320);

pub const ICON_RULES: &str = "PNG, JPG or WebP  •  up to 5 MB  •  strictly square (1:1)  •  from 128×128 to 2048×2048 px. PNG 512×512 with a transparent background works best. The icon of the .exe file itself stays the default one.";
pub const BG_RULES: &str = "PNG, JPG or WebP  •  up to 10 MB  •  from 800×450 to 7680×4320 px. 16:9 works best, e.g. 1920×1080: the image is stretched over the whole area and cropped at the edges.";

// ---------- Paths ----------

fn custom_dir() -> PathBuf {
    let d = data_dir().join("custom");
    let _ = fs::create_dir_all(&d);
    d
}
pub fn icon_path() -> PathBuf {
    custom_dir().join("icon.png")
}
pub fn bg_path() -> PathBuf {
    custom_dir().join("background.png")
}

// ---------- Validation and saving ----------

fn open(path: &Path, max_bytes: u64) -> Result<DynamicImage> {
    let meta = fs::metadata(path).map_err(|_| anyhow!("File not found: {}", path.display()))?;
    if !meta.is_file() {
        bail!("Not a file: {}", path.display());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
        bail!("Format '.{ext}' is not supported. Use PNG, JPG or WebP");
    }
    if meta.len() > max_bytes {
        bail!(
            "File is too large: {:.1} MB, maximum {} MB",
            meta.len() as f64 / 1_048_576.0,
            max_bytes / 1_048_576
        );
    }
    image::open(path).map_err(|e| anyhow!("Could not read the image: {e}"))
}

pub fn set_icon(path: &Path) -> Result<()> {
    let img = open(path, ICON_MAX_BYTES)?;
    let (w, h) = img.dimensions();
    if w < ICON_MIN || h < ICON_MIN {
        bail!("Icon is too small: {w}×{h}. Minimum is {ICON_MIN}×{ICON_MIN}");
    }
    if w > ICON_MAX || h > ICON_MAX {
        bail!("Icon is too large: {w}×{h}. Maximum is {ICON_MAX}×{ICON_MAX}");
    }
    let ratio = w as f32 / h as f32;
    if (ratio - 1.0).abs() > 0.02 {
        bail!("The icon must be square (1:1), but the file is {w}×{h}");
    }
    let out = img.resize_exact(256, 256, FilterType::Lanczos3);
    out.save_with_format(icon_path(), ImageFormat::Png)
        .map_err(|e| anyhow!("Could not save the icon: {e}"))
}

pub fn set_background(path: &Path) -> Result<()> {
    let img = open(path, BG_MAX_BYTES)?;
    let (w, h) = img.dimensions();
    if w < BG_MIN.0 || h < BG_MIN.1 {
        bail!("Background is too small: {w}×{h}. Minimum is {}×{}", BG_MIN.0, BG_MIN.1);
    }
    if w > BG_MAX.0 || h > BG_MAX.1 {
        bail!("Background is too large: {w}×{h}. Maximum is {}×{}", BG_MAX.0, BG_MAX.1);
    }
    // Store at most Full HD so we don't waste GPU memory.
    let img = if w > 1920 || h > 1080 {
        img.resize(1920, 1080, FilterType::Triangle)
    } else {
        img
    };
    DynamicImage::ImageRgb8(img.to_rgb8())
        .save_with_format(bg_path(), ImageFormat::Png)
        .map_err(|e| anyhow!("Could not save the background: {e}"))
}

pub fn remove_icon() {
    let _ = fs::remove_file(icon_path());
}
pub fn remove_background() {
    let _ = fs::remove_file(bg_path());
}

// ---------- Loading for egui ----------

pub fn load_bg() -> Option<egui::ColorImage> {
    let img = image::open(bg_path()).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        img.as_raw(),
    ))
}

pub fn load_icon(side: u32) -> Option<egui::ColorImage> {
    let img = image::open(icon_path())
        .ok()?
        .resize_exact(side, side, FilterType::Lanczos3)
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [side as usize, side as usize],
        img.as_raw(),
    ))
}

pub fn icon_data() -> Option<egui::IconData> {
    let img = image::open(icon_path())
        .ok()?
        .resize_exact(256, 256, FilterType::Lanczos3)
        .to_rgba8();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: 256,
        height: 256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_png(name: &str, w: u32, h: u32) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ferrum-test-{}-{}", std::process::id(), name));
        image::RgbaImage::new(w, h).save(&p).unwrap();
        p
    }

    #[test]
    fn icon_must_be_square() {
        let p = temp_png("wide.png", 300, 200);
        let err = set_icon(&p).unwrap_err().to_string();
        assert!(err.contains("square"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn icon_must_not_be_tiny() {
        let p = temp_png("tiny.png", 64, 64);
        let err = set_icon(&p).unwrap_err().to_string();
        assert!(err.contains("too small"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn background_must_not_be_tiny() {
        let p = temp_png("small-bg.png", 400, 300);
        let err = set_background(&p).unwrap_err().to_string();
        assert!(err.contains("too small"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn wrong_extension_is_rejected() {
        let p = std::env::temp_dir().join(format!("ferrum-test-{}-x.bmp", std::process::id()));
        fs::write(&p, b"not an image").unwrap();
        let err = set_icon(&p).unwrap_err().to_string();
        assert!(err.contains("Format"), "{err}");
        let _ = fs::remove_file(p);
    }
}
