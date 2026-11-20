//! Свои изображения: фон интерфейса и иконка лаунчера.
use crate::config::data_dir;
use anyhow::{anyhow, bail, Result};
use eframe::egui;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use std::{
    fs,
    path::{Path, PathBuf},
};

// ---------- Ограничения ----------

/// Иконка: квадратная, 128…2048 px, до 5 МБ.
const ICON_MAX_BYTES: u64 = 5 * 1024 * 1024;
const ICON_MIN: u32 = 128;
const ICON_MAX: u32 = 2048;
/// Фон: от 800×450 до 7680×4320 px, до 10 МБ.
const BG_MAX_BYTES: u64 = 10 * 1024 * 1024;
const BG_MIN: (u32, u32) = (800, 450);
const BG_MAX: (u32, u32) = (7680, 4320);

pub const ICON_RULES: &str = "PNG, JPG или WebP  •  до 5 МБ  •  строго квадратная (1:1)  •  от 128×128 до 2048×2048 px. Лучше всего PNG 512×512 с прозрачным фоном. Иконка самого .exe-файла останется стандартной.";
pub const BG_RULES: &str = "PNG, JPG или WebP  •  до 10 МБ  •  от 800×450 до 7680×4320 px. Лучше всего 16:9, например 1920×1080: картинка растягивается на всю область и обрезается по краям.";

// ---------- Пути ----------

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

// ---------- Проверка и сохранение ----------

fn open(path: &Path, max_bytes: u64) -> Result<DynamicImage> {
    let meta = fs::metadata(path).map_err(|_| anyhow!("Файл не найден: {}", path.display()))?;
    if !meta.is_file() {
        bail!("Это не файл: {}", path.display());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
        bail!("Формат «.{ext}» не подходит. Нужен PNG, JPG или WebP");
    }
    if meta.len() > max_bytes {
        bail!(
            "Файл слишком большой: {:.1} МБ, максимум {} МБ",
            meta.len() as f64 / 1_048_576.0,
            max_bytes / 1_048_576
        );
    }
    image::open(path).map_err(|e| anyhow!("Не удалось прочитать картинку: {e}"))
}

pub fn set_icon(path: &Path) -> Result<()> {
    let img = open(path, ICON_MAX_BYTES)?;
    let (w, h) = img.dimensions();
    if w < ICON_MIN || h < ICON_MIN {
        bail!("Иконка слишком маленькая: {w}×{h}. Минимум {ICON_MIN}×{ICON_MIN}");
    }
    if w > ICON_MAX || h > ICON_MAX {
        bail!("Иконка слишком большая: {w}×{h}. Максимум {ICON_MAX}×{ICON_MAX}");
    }
    let ratio = w as f32 / h as f32;
    if (ratio - 1.0).abs() > 0.02 {
        bail!("Иконка должна быть квадратной (1:1), а у файла {w}×{h}");
    }
    let out = img.resize_exact(256, 256, FilterType::Lanczos3);
    out.save_with_format(icon_path(), ImageFormat::Png)
        .map_err(|e| anyhow!("Не удалось сохранить иконку: {e}"))
}

pub fn set_background(path: &Path) -> Result<()> {
    let img = open(path, BG_MAX_BYTES)?;
    let (w, h) = img.dimensions();
    if w < BG_MIN.0 || h < BG_MIN.1 {
        bail!("Фон слишком маленький: {w}×{h}. Минимум {}×{}", BG_MIN.0, BG_MIN.1);
    }
    if w > BG_MAX.0 || h > BG_MAX.1 {
        bail!("Фон слишком большой: {w}×{h}. Максимум {}×{}", BG_MAX.0, BG_MAX.1);
    }
    // Храним не больше Full HD, чтобы не тратить память видеокарты.
    let img = if w > 1920 || h > 1080 {
        img.resize(1920, 1080, FilterType::Triangle)
    } else {
        img
    };
    DynamicImage::ImageRgb8(img.to_rgb8())
        .save_with_format(bg_path(), ImageFormat::Png)
        .map_err(|e| anyhow!("Не удалось сохранить фон: {e}"))
}

pub fn remove_icon() {
    let _ = fs::remove_file(icon_path());
}
pub fn remove_background() {
    let _ = fs::remove_file(bg_path());
}

// ---------- Загрузка для egui ----------

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
        assert!(err.contains("квадратной"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn icon_must_not_be_tiny() {
        let p = temp_png("tiny.png", 64, 64);
        let err = set_icon(&p).unwrap_err().to_string();
        assert!(err.contains("маленькая"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn background_must_not_be_tiny() {
        let p = temp_png("small-bg.png", 400, 300);
        let err = set_background(&p).unwrap_err().to_string();
        assert!(err.contains("маленький"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn wrong_extension_is_rejected() {
        let p = std::env::temp_dir().join(format!("ferrum-test-{}-x.bmp", std::process::id()));
        fs::write(&p, b"not an image").unwrap();
        let err = set_icon(&p).unwrap_err().to_string();
        assert!(err.contains("Формат"), "{err}");
        let _ = fs::remove_file(p);
    }
}
