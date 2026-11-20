//! Автоустановка оптимизации: Sodium (Fabric / Quilt / NeoForge) и OptiFine (Forge).
use crate::config::{Instance, Loader, ModSource};
use crate::mods;
use crate::net::{download, Job};
use anyhow::{anyhow, bail, Result};
use reqwest::blocking::Client;
use std::{fs, io::Read, path::Path};

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub(crate) fn has_prefix(dir: &Path, prefix: &str) -> bool {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy().to_lowercase().starts_with(prefix))
        })
        .unwrap_or(false)
}

/// Возвращает короткий отчёт для лога запуска.
pub fn install_for(http: &Client, inst: &Instance) -> Result<String> {
    let dir = inst.mods_dir();
    fs::create_dir_all(&dir)?;
    match inst.loader {
        Loader::Fabric | Loader::Quilt | Loader::NeoForge => {
            if has_prefix(&dir, "sodium") {
                return Ok("Sodium уже установлен".to_string());
            }
            let log = mods::install(
                http,
                ModSource::Modrinth,
                "",
                "sodium",
                &inst.mc_version,
                inst.loader,
                &dir,
            )?;
            Ok(format!("Sodium установлен: {}", log.join(", ")))
        }
        Loader::Forge => optifine(http, &inst.mc_version, &dir),
        Loader::Vanilla => Ok("Для Vanilla оптимизация не ставится".to_string()),
    }
}

/// Кусок строки от `start` до ближайшей кавычки, пробела или `<`.
fn piece<'a>(s: &'a str, start: &str) -> Option<&'a str> {
    let i = s.find(start)?;
    let rest = &s[i..];
    let end = rest
        .find(|c: char| c == '"' || c == '\'' || c == '<' || c == ' ')
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// У OptiFine нет API: берём ссылку со страницы optifine.net, как это делает браузер.
fn optifine(http: &Client, mc: &str, dir: &Path) -> Result<String> {
    if has_prefix(dir, "optifine") {
        return Ok("OptiFine уже установлен".to_string());
    }
    let page = http
        .get("https://optifine.net/downloads")
        .header("User-Agent", BROWSER_UA)
        .send()?
        .error_for_status()?
        .text()?;
    let needle = format!("adloadx?f=OptiFine_{mc}_HD_U_");
    let part = piece(&page, &needle).ok_or_else(|| {
        anyhow!("OptiFine для Minecraft {mc} не найден на optifine.net (скачайте вручную и положите в папку mods)")
    })?;
    let file = part.trim_start_matches("adloadx?f=").to_string();

    let ad = http
        .get(format!("https://optifine.net/adloadx?f={file}"))
        .header("User-Agent", BROWSER_UA)
        .header("Referer", "https://optifine.net/downloads")
        .send()?
        .error_for_status()?
        .text()?;
    let link = piece(&ad, "downloadx?f=")
        .ok_or_else(|| anyhow!("Не удалось получить прямую ссылку на OptiFine"))?
        .replace("&amp;", "&");
    let dest = dir.join(&file);
    download(
        http,
        &Job {
            url: format!("https://optifine.net/{link}"),
            dest: dest.clone(),
            sha1: None,
            size: None,
        },
    )?;
    // Убеждаемся, что скачали jar (zip), а не html-страницу.
    let mut head = [0u8; 2];
    fs::File::open(&dest)?.read_exact(&mut head)?;
    if &head != b"PK" {
        let _ = fs::remove_file(&dest);
        bail!("Сайт OptiFine вернул не jar-файл. Скачайте OptiFine вручную и положите в папку mods");
    }
    Ok(format!("OptiFine установлен: {file}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piece_stops_at_quote() {
        let html = r#"<a href="adloadx?f=OptiFine_1.20.1_HD_U_I7.jar">Download</a>"#;
        assert_eq!(
            piece(html, "adloadx?f=OptiFine_1.20.1_HD_U_"),
            Some("adloadx?f=OptiFine_1.20.1_HD_U_I7.jar")
        );
        // 1.20 не должна совпасть с 1.20.1
        assert_eq!(piece(html, "adloadx?f=OptiFine_1.20_HD_U_"), None);
    }
}
