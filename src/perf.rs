//! Automatic performance mods: Sodium (Fabric / Quilt / NeoForge) and OptiFine (Forge).
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

/// Returns a short report for the launch log.
pub fn install_for(http: &Client, inst: &Instance) -> Result<String> {
    let dir = inst.mods_dir();
    fs::create_dir_all(&dir)?;
    match inst.loader {
        Loader::Fabric | Loader::Quilt | Loader::NeoForge => {
            if has_prefix(&dir, "sodium") {
                return Ok("Sodium is already installed".to_string());
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
            Ok(format!("Sodium installed: {}", log.join(", ")))
        }
        Loader::Forge => optifine(http, &inst.mc_version, &dir),
        Loader::Vanilla => Ok("No performance mods are installed for Vanilla".to_string()),
    }
}

/// The part of the string from `start` up to the nearest quote, space or `<`.
fn piece<'a>(s: &'a str, start: &str) -> Option<&'a str> {
    let i = s.find(start)?;
    let rest = &s[i..];
    let end = rest
        .find(|c: char| c == '"' || c == '\'' || c == '<' || c == ' ')
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// OptiFine has no API: we take the link from the optifine.net page, the way a browser does.
fn optifine(http: &Client, mc: &str, dir: &Path) -> Result<String> {
    if has_prefix(dir, "optifine") {
        return Ok("OptiFine is already installed".to_string());
    }
    let page = http
        .get("https://optifine.net/downloads")
        .header("User-Agent", BROWSER_UA)
        .send()?
        .error_for_status()?
        .text()?;
    let needle = format!("adloadx?f=OptiFine_{mc}_HD_U_");
    let part = piece(&page, &needle).ok_or_else(|| {
        anyhow!("OptiFine for Minecraft {mc} was not found on optifine.net (download it manually and put it in the mods folder)")
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
        .ok_or_else(|| anyhow!("Could not get a direct download link for OptiFine"))?
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
    // Make sure we downloaded a jar (zip) and not an HTML page.
    let mut head = [0u8; 2];
    fs::File::open(&dest)?.read_exact(&mut head)?;
    if &head != b"PK" {
        let _ = fs::remove_file(&dest);
        bail!("The OptiFine site did not return a jar file. Download OptiFine manually and put it in the mods folder");
    }
    Ok(format!("OptiFine installed: {file}"))
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
        // 1.20 must not match 1.20.1
        assert_eq!(piece(html, "adloadx?f=OptiFine_1.20_HD_U_"), None);
    }
}
