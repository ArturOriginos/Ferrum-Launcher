use crate::config::{Loader, ModSource};
use crate::net::{download, Job};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

/// Ключ CurseForge. Он только пользовательский: вшивать общий ключ в публичную сборку
/// нельзя по правилам CurseForge, поэтому каждый вводит свой в настройках.
pub fn curseforge_key(from_settings: &str) -> String {
    from_settings.trim().to_string()
}

#[derive(Clone, Debug)]
pub struct ModHit {
    pub source: ModSource,
    pub id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
}

pub fn search(
    http: &Client,
    source: ModSource,
    cf_key: &str,
    query: &str,
    mc: &str,
    loader: Loader,
) -> Result<Vec<ModHit>> {
    match source {
        ModSource::Modrinth => modrinth_search(http, query, mc, loader),
        ModSource::CurseForge => cf_search(http, cf_key, query, mc, loader),
    }
}

pub fn install(
    http: &Client,
    source: ModSource,
    cf_key: &str,
    id: &str,
    mc: &str,
    loader: Loader,
    dir: &Path,
) -> Result<Vec<String>> {
    fs::create_dir_all(dir)?;
    let mut done = HashSet::new();
    let mut log = Vec::new();
    match source {
        ModSource::Modrinth => modrinth_install(http, id, mc, loader, dir, &mut done, &mut log)?,
        ModSource::CurseForge => cf_install(http, cf_key, id, mc, loader, dir, &mut done, &mut log)?,
    }
    Ok(log)
}

// ---------- Modrinth ----------

fn quoted_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn modrinth_search(http: &Client, query: &str, mc: &str, loader: Loader) -> Result<Vec<ModHit>> {
    let mut facets = vec![
        r#"["project_type:mod"]"#.to_string(),
        format!(r#"["versions:{mc}"]"#),
    ];
    let cats: Vec<String> = loader
        .modrinth_names()
        .iter()
        .map(|n| format!("categories:{n}"))
        .collect();
    if !cats.is_empty() {
        facets.push(quoted_list(&cats));
    }
    let facets = format!("[{}]", facets.join(","));
    let v: Value = http
        .get("https://api.modrinth.com/v2/search")
        .query(&[("query", query), ("facets", facets.as_str()), ("limit", "25")])
        .send()?
        .error_for_status()?
        .json()?;
    Ok(v["hits"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| ModHit {
                    source: ModSource::Modrinth,
                    id: h["project_id"].as_str().unwrap_or("").to_string(),
                    title: h["title"].as_str().unwrap_or("?").to_string(),
                    author: h["author"].as_str().unwrap_or("").to_string(),
                    description: h["description"].as_str().unwrap_or("").to_string(),
                    downloads: h["downloads"].as_u64().unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default())
}

fn modrinth_install(
    http: &Client,
    id: &str,
    mc: &str,
    loader: Loader,
    dir: &Path,
    done: &mut HashSet<String>,
    log: &mut Vec<String>,
) -> Result<()> {
    if !done.insert(id.to_string()) {
        return Ok(());
    }
    let names: Vec<String> = loader.modrinth_names().iter().map(|s| s.to_string()).collect();
    let vs: Value = http
        .get(format!("https://api.modrinth.com/v2/project/{id}/version"))
        .query(&[
            ("game_versions", format!("[\"{mc}\"]")),
            ("loaders", quoted_list(&names)),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let ver = vs
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow!("Нет версии мода под {mc} / {}", loader.label()))?;
    let files = ver["files"].as_array().context("У версии нет файлов")?;
    let f = files
        .iter()
        .find(|f| f["primary"].as_bool().unwrap_or(false))
        .or(files.first())
        .context("У версии нет файлов")?;
    let name = f["filename"].as_str().context("Нет имени файла")?;
    download(
        http,
        &Job {
            url: f["url"].as_str().context("Нет ссылки на файл")?.to_string(),
            dest: dir.join(name),
            sha1: f["hashes"]["sha1"].as_str().map(String::from),
            size: None,
        },
    )?;
    log.push(name.to_string());

    // Обязательные зависимости.
    for d in ver["dependencies"].as_array().into_iter().flatten() {
        if d["dependency_type"].as_str() == Some("required") {
            if let Some(pid) = d["project_id"].as_str() {
                let _ = modrinth_install(http, pid, mc, loader, dir, done, log);
            }
        }
    }
    Ok(())
}

// ---------- CurseForge ----------

const CF: &str = "https://api.curseforge.com/v1";

fn cf_get(http: &Client, key: &str, url: &str, params: &[(&str, String)]) -> Result<Value> {
    if key.is_empty() {
        bail!("Нужен API-ключ CurseForge. Получите его на console.curseforge.com и вставьте в «Настройки», либо переключитесь на Modrinth.");
    }
    Ok(http
        .get(url)
        .header("x-api-key", key)
        .query(params)
        .send()?
        .error_for_status()?
        .json::<Value>()?)
}

fn cf_search(http: &Client, key: &str, q: &str, mc: &str, loader: Loader) -> Result<Vec<ModHit>> {
    let mut params: Vec<(&str, String)> = vec![
        ("gameId", "432".to_string()),
        ("classId", "6".to_string()),
        ("searchFilter", q.to_string()),
        ("gameVersion", mc.to_string()),
        ("pageSize", "25".to_string()),
        ("sortField", "2".to_string()),
        ("sortOrder", "desc".to_string()),
    ];
    if let Some(t) = loader.curseforge() {
        params.push(("modLoaderType", t.to_string()));
    }
    let v = cf_get(http, key, &format!("{CF}/mods/search"), &params)?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| ModHit {
                    source: ModSource::CurseForge,
                    id: h["id"].as_u64().map(|n| n.to_string()).unwrap_or_default(),
                    title: h["name"].as_str().unwrap_or("?").to_string(),
                    author: h["authors"][0]["name"].as_str().unwrap_or("").to_string(),
                    description: h["summary"].as_str().unwrap_or("").to_string(),
                    downloads: h["downloadCount"].as_f64().unwrap_or(0.0) as u64,
                })
                .collect()
        })
        .unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
fn cf_install(
    http: &Client,
    key: &str,
    id: &str,
    mc: &str,
    loader: Loader,
    dir: &Path,
    done: &mut HashSet<String>,
    log: &mut Vec<String>,
) -> Result<()> {
    if !done.insert(id.to_string()) {
        return Ok(());
    }
    let mut params: Vec<(&str, String)> = vec![
        ("gameVersion", mc.to_string()),
        ("pageSize", "20".to_string()),
    ];
    if let Some(t) = loader.curseforge() {
        params.push(("modLoaderType", t.to_string()));
    }
    let v = cf_get(http, key, &format!("{CF}/mods/{id}/files"), &params)?;
    let mut files: Vec<&Value> = v["data"].as_array().context("Нет списка файлов")?.iter().collect();
    files.sort_by(|a, b| {
        b["fileDate"]
            .as_str()
            .unwrap_or("")
            .cmp(a["fileDate"].as_str().unwrap_or(""))
    });
    // Предпочитаем релизы (releaseType = 1).
    let f = files
        .iter()
        .find(|f| f["releaseType"].as_u64() == Some(1))
        .or(files.first())
        .ok_or_else(|| anyhow!("Нет файлов мода под {mc} / {}", loader.label()))?;
    let name = f["fileName"].as_str().context("Нет имени файла")?;
    let url = match f["downloadUrl"].as_str() {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => {
            // Автор запретил выдачу ссылки через API — берём прямой адрес CDN.
            let fid = f["id"].as_u64().context("Нет id файла")?;
            format!("https://edge.forgecdn.net/files/{}/{}/{}", fid / 1000, fid % 1000, name)
        }
    };
    download(
        http,
        &Job {
            url,
            dest: dir.join(name),
            sha1: None,
            size: None,
        },
    )?;
    log.push(name.to_string());

    for d in f["dependencies"].as_array().into_iter().flatten() {
        // relationType 3 = обязательная зависимость
        if d["relationType"].as_u64() == Some(3) {
            if let Some(mid) = d["modId"].as_u64() {
                let _ = cf_install(http, key, &mid.to_string(), mc, loader, dir, done, log);
            }
        }
    }
    Ok(())
}

// ---------- Установленные моды ----------

#[derive(Clone)]
pub struct InstalledMod {
    pub file: String,
    pub enabled: bool,
}

pub fn list_installed(dir: &Path) -> Vec<InstalledMod> {
    let mut v: Vec<InstalledMod> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".jar") {
                        Some(InstalledMod { file: name, enabled: true })
                    } else if name.ends_with(".jar.disabled") {
                        Some(InstalledMod { file: name, enabled: false })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|m| m.file.to_lowercase());
    v
}

pub fn toggle(dir: &Path, m: &InstalledMod) {
    let from = dir.join(&m.file);
    let to = if m.enabled {
        dir.join(format!("{}.disabled", m.file))
    } else {
        dir.join(m.file.trim_end_matches(".disabled"))
    };
    let _ = fs::rename(from, to);
}

pub fn remove(dir: &Path, m: &InstalledMod) {
    let _ = fs::remove_file(dir.join(&m.file));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_list_format() {
        assert_eq!(quoted_list(&["a".to_string(), "b".to_string()]), "[\"a\",\"b\"]");
    }

    #[test]
    fn curseforge_key_prefers_settings() {
        assert_eq!(curseforge_key("  my-key "), "my-key");
    }

    #[test]
    fn installed_mods_list_and_toggle() {
        let dir = std::env::temp_dir().join(format!("ferrum-test-mods-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.jar"), b"x").unwrap();
        fs::write(dir.join("b.jar.disabled"), b"x").unwrap();
        fs::write(dir.join("c.txt"), b"x").unwrap();
        let list = list_installed(&dir);
        assert_eq!(list.len(), 2);
        assert!(list[0].enabled && list[0].file == "a.jar");
        assert!(!list[1].enabled && list[1].file == "b.jar.disabled");
        toggle(&dir, &list[0]);
        toggle(&dir, &list[1]);
        assert!(dir.join("a.jar.disabled").exists());
        assert!(dir.join("b.jar").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
