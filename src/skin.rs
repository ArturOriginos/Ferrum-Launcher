//! Skins for offline accounts. They are shown by the CustomSkinLoader mod (installed automatically).
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
    "PNG  •  strictly 64×64 or 64×32 pixels  •  up to 200 KB. The skin is only visible in your own game.";

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
    let meta = fs::metadata(src).map_err(|_| anyhow!("File not found: {}", src.display()))?;
    if !meta.is_file() {
        bail!("Not a file: {}", src.display());
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "png" {
        bail!("The skin must be a PNG, but the file is '.{ext}'");
    }
    if meta.len() > MAX_BYTES {
        bail!("File is too large: {} KB, maximum 200 KB", meta.len() / 1024);
    }
    let img = image::open(src).map_err(|e| anyhow!("Could not read the image: {e}"))?;
    let (w, h) = img.dimensions();
    if !((w, h) == (64, 64) || (w, h) == (64, 32)) {
        bail!("The skin must be 64×64 or 64×32 pixels, but the file is {w}×{h}");
    }
    fs::create_dir_all(skins_dir())?;
    img.save_with_format(skin_path(uuid), ImageFormat::Png)
        .map_err(|e| anyhow!("Could not save the skin: {e}"))
}

/// The player's face (head + outer layer), enlarged for the avatar.
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

/// Before launch: puts the skin into the instance and installs CustomSkinLoader if it isn't there yet.
/// Returns a line for the log, or None if the account has no skin.
pub fn prepare_instance(http: &Client, inst: &Instance, acc: &Account) -> Result<Option<String>> {
    let src = skin_path(&acc.uuid);
    if !src.is_file() {
        return Ok(None);
    }
    if inst.loader == Loader::Vanilla {
        return Ok(Some(
            "Skin not applied: Vanilla has no mod loader (choose Fabric, Quilt, Forge or NeoForge)".to_string(),
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
        return Ok(Some("Skin copied into the instance".to_string()));
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
        "Skin copied, CustomSkinLoader installed: {}",
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
