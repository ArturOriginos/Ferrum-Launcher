//! Скины для офлайн-аккаунтов. Показывает их мод CustomSkinLoader (ставится автоматически).
use crate::config::{data_dir, Account, Instance, Loader, ModSource};
use crate::{mods, perf};
use anyhow::{anyhow, bail, Result};
use eframe::egui;
use image::{imageops::FilterType, GenericImageView, ImageFormat, RgbaImage};
use reqwest::blocking::Client;
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 200 * 1024;

pub const SKIN_RULES: &str =
    "PNG  •  строго 64×64 или 64×32 пикселя  •  до 200 КБ. Скин виден только в вашей игре.";

fn skins_dir() -> PathBuf {
    data_dir().join("skins")
}

pub fn skin_path(uuid: &str) -> PathBuf {
    skins_dir().join(format!("{uuid}.png"))
}

pub fn has(uuid: &str) -> bool {
    skin_path(uuid).is_file()
}

pub fn remove(uuid: &str) {
    let _ = fs::remove_file(skin_path(uuid));
}

pub fn set(uuid: &str, src: &Path) -> Result<()> {
    let meta = fs::metadata(src).map_err(|_| anyhow!("Файл не найден: {}", src.display()))?;
    if !meta.is_file() {
        bail!("Это не файл: {}", src.display());
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "png" {
        bail!("Скин должен быть в формате PNG, а у файла «.{ext}»");
    }
    if meta.len() > MAX_BYTES {
        bail!("Файл слишком большой: {} КБ, максимум 200 КБ", meta.len() / 1024);
    }
    let img = image::open(src).map_err(|e| anyhow!("Не удалось прочитать картинку: {e}"))?;
    let (w, h) = img.dimensions();
    if !((w, h) == (64, 64) || (w, h) == (64, 32)) {
        bail!("Скин должен быть 64×64 или 64×32 пикселя, а у файла {w}×{h}");
    }
    fs::create_dir_all(skins_dir())?;
    img.save_with_format(skin_path(uuid), ImageFormat::Png)
        .map_err(|e| anyhow!("Не удалось сохранить скин: {e}"))
}

/// Лицо игрока (голова + внешний слой), увеличенное для аватарки.
pub fn load_face(uuid: &str) -> Option<egui::ColorImage> {
    let img = image::open(skin_path(uuid)).ok()?.to_rgba8();
    if img.width() < 64 || img.height() < 16 {
        return None;
    }
    let mut face = RgbaImage::new(8, 8);
    for y in 0..8u32 {
        for x in 0..8u32 {
            let mut px = *img.get_pixel(8 + x, 8 + y);
            let hat = *img.get_pixel(40 + x, 8 + y);
            if hat[3] > 0 {
                px = hat;
            }
            face.put_pixel(x, y, px);
        }
    }
    let big = image::imageops::resize(&face, 64, 64, FilterType::Nearest);
    Some(egui::ColorImage::from_rgba_unmultiplied([64, 64], big.as_raw()))
}

/// Перед запуском: кладёт скин в сборку и ставит CustomSkinLoader, если его ещё нет.
/// Возвращает строку для лога или None, если у аккаунта нет скина.
pub fn prepare_instance(http: &Client, inst: &Instance, acc: &Account) -> Result<Option<String>> {
    let src = skin_path(&acc.uuid);
    if !src.is_file() {
        return Ok(None);
    }
    if inst.loader == Loader::Vanilla {
        return Ok(Some(
            "Скин не применён: в Vanilla нет загрузчика модов (выберите Fabric, Quilt, Forge или NeoForge)".to_string(),
        ));
    }
    let dest_dir = inst
        .dir()
        .join("CustomSkinLoader")
        .join("LocalSkin")
        .join("skins");
    fs::create_dir_all(&dest_dir)?;
    fs::copy(&src, dest_dir.join(format!("{}.png", acc.name)))?;

    let mods_dir = inst.mods_dir();
    if perf::has_prefix(&mods_dir, "customskinloader") {
        return Ok(Some("Скин скопирован в сборку".to_string()));
    }
    let log = mods::install(
        http,
        ModSource::Modrinth,
        "",
        "customskinloader",
        &inst.mc_version,
        inst.loader,
        &mods_dir,
    )?;
    Ok(Some(format!(
        "Скин скопирован, CustomSkinLoader установлен: {}",
        log.join(", ")
    )))
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
    fn skin_must_be_64x64_or_64x32() {
        let p = temp_png("skin-100.png", 100, 100);
        let err = set("test-uuid", &p).unwrap_err().to_string();
        assert!(err.contains("64×64"), "{err}");
        let _ = fs::remove_file(p);
    }

    #[test]
    fn skin_must_be_png() {
        let p = std::env::temp_dir().join(format!("ferrum-test-{}-skin.jpg", std::process::id()));
        fs::write(&p, b"x").unwrap();
        let err = set("test-uuid", &p).unwrap_err().to_string();
        assert!(err.contains("PNG"), "{err}");
        let _ = fs::remove_file(p);
    }
}
