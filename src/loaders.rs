use crate::config::{data_dir, Instance, Loader};
use crate::java::quiet;
use crate::minecraft::version_json_path;
use crate::net::{download_all, get_json, Job, Progress};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{fs, io::Read, path::Path, process::Command};

/// Возвращает id версии, которую нужно запускать (vanilla / fabric-loader-… / neoforge-…).
/// Результат кэшируется в папке сборки, чтобы не ходить в сеть при каждом запуске.
pub fn resolve_id(
    http: &Client,
    root: &Path,
    inst: &Instance,
    java: &str,
    progress: &Progress,
) -> Result<String> {
    let marker = inst.dir().join(".ferrum_version");
    if let Ok(id) = fs::read_to_string(&marker) {
        let id = id.trim().to_string();
        if !id.is_empty() && version_json_path(root, &id).exists() {
            return Ok(id);
        }
    }
    let mc = inst.mc_version.as_str();
    let want = inst.loader_version.trim();
    let id = match inst.loader {
        Loader::Vanilla => mc.to_string(),
        Loader::Fabric => fabric_like(http, root, "https://meta.fabricmc.net/v2", mc, want, progress)?,
        Loader::Quilt => fabric_like(http, root, "https://meta.quiltmc.org/v3", mc, want, progress)?,
        Loader::Forge => forge(http, root, mc, want, java, progress)?,
        Loader::NeoForge => neoforge(http, root, mc, want, java, progress)?,
    };
    fs::create_dir_all(inst.dir())?;
    fs::write(marker, &id)?;
    Ok(id)
}

fn fabric_like(
    http: &Client,
    root: &Path,
    base: &str,
    mc: &str,
    want: &str,
    progress: &Progress,
) -> Result<String> {
    progress.label("Получаю версию загрузчика…");
    let ver = if want.is_empty() {
        let list = get_json(http, &format!("{base}/versions/loader/{mc}"))?;
        let arr = list.as_array().context("Некорректный ответ сервера загрузчика")?;
        let pick = arr
            .iter()
            .find(|e| {
                e["loader"]["stable"].as_bool().unwrap_or(true)
                    && !e["loader"]["version"].as_str().unwrap_or("").contains("beta")
            })
            .or_else(|| arr.first())
            .ok_or_else(|| anyhow!("Для Minecraft {mc} нет такого загрузчика"))?;
        pick["loader"]["version"]
            .as_str()
            .context("Нет версии загрузчика")?
            .to_string()
    } else {
        want.to_string()
    };
    let profile = get_json(http, &format!("{base}/versions/loader/{mc}/{ver}/profile/json"))?;
    let id = profile["id"].as_str().context("В профиле нет id")?.to_string();
    let p = version_json_path(root, &id);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(p, serde_json::to_vec(&profile)?)?;
    Ok(id)
}

fn forge(
    http: &Client,
    root: &Path,
    mc: &str,
    want: &str,
    java: &str,
    progress: &Progress,
) -> Result<String> {
    progress.label("Получаю версию Forge…");
    let ver = if want.is_empty() {
        let promos = get_json(
            http,
            "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
        )?;
        let rec = promos["promos"][format!("{mc}-recommended")].as_str();
        let lat = promos["promos"][format!("{mc}-latest")].as_str();
        rec.or(lat)
            .map(String::from)
            .ok_or_else(|| anyhow!("Forge для Minecraft {mc} не найден"))?
    } else {
        want.to_string()
    };
    let url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{ver}/forge-{mc}-{ver}-installer.jar"
    );
    run_installer(
        http,
        root,
        &url,
        &format!("forge-{mc}-{ver}-installer.jar"),
        java,
        &["--installClient", "--install-client"],
        progress,
    )
}

fn neo_prefix(mc: &str) -> Result<String> {
    let rest = mc
        .strip_prefix("1.")
        .ok_or_else(|| anyhow!("NeoForge не поддерживает {mc}"))?;
    Ok(if rest.contains('.') {
        format!("{rest}.")
    } else {
        format!("{rest}.0.")
    })
}

fn ver_key(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

fn neoforge(
    http: &Client,
    root: &Path,
    mc: &str,
    want: &str,
    java: &str,
    progress: &Progress,
) -> Result<String> {
    progress.label("Получаю версию NeoForge…");
    let ver = if want.is_empty() {
        let list = get_json(
            http,
            "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge",
        )?;
        let prefix = neo_prefix(mc)?;
        let mut all: Vec<String> = list["versions"]
            .as_array()
            .context("Некорректный список версий NeoForge")?
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .filter(|s| s.starts_with(&prefix))
            .collect();
        all.sort_by_key(|s| ver_key(s));
        all.iter()
            .rev()
            .find(|s| !s.contains('-'))
            .or(all.last())
            .cloned()
            .ok_or_else(|| anyhow!("NeoForge для Minecraft {mc} не найден"))?
    } else {
        want.to_string()
    };
    let url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar"
    );
    run_installer(
        http,
        root,
        &url,
        &format!("neoforge-{ver}-installer.jar"),
        java,
        &["--install-client", "--installClient"],
        progress,
    )
}

fn installer_version_id(jar: &Path) -> Result<String> {
    let mut z = zip::ZipArchive::new(fs::File::open(jar)?)?;
    let mut f = z.by_name("version.json")?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    let v: Value = serde_json::from_str(&s)?;
    v["id"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("В установщике нет version.json/id"))
}

fn tail(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

/// Скачивает установщик Forge/NeoForge и запускает его в режиме «установить клиент».
fn run_installer(
    http: &Client,
    root: &Path,
    url: &str,
    file: &str,
    java: &str,
    flags: &[&str],
    progress: &Progress,
) -> Result<String> {
    let jar = data_dir().join("installers").join(file);
    progress.label("Скачиваю установщик загрузчика…");
    download_all(
        http,
        vec![Job {
            url: url.to_string(),
            dest: jar.clone(),
            sha1: None,
            size: None,
        }],
        progress,
    )?;
    let id = installer_version_id(&jar)?;
    if version_json_path(root, &id).exists() {
        return Ok(id);
    }

    progress.label("Устанавливаю загрузчик (до минуты)…");
    fs::create_dir_all(root)?;
    let lp = root.join("launcher_profiles.json");
    if !lp.exists() {
        fs::write(&lp, r#"{"profiles":{},"version":3}"#)?;
    }
    let mut last_output = String::new();
    for flag in flags {
        let mut c = Command::new(java);
        c.arg("-jar").arg(&jar).arg(flag).arg(root).current_dir(root);
        quiet(&mut c);
        let out = c.output().with_context(|| format!("Не удалось запустить {java}"))?;
        last_output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if version_json_path(root, &id).exists() {
            return Ok(id);
        }
    }
    bail!("Установщик загрузчика завершился ошибкой:\n{}", tail(&last_output, 1500))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_prefix() {
        assert_eq!(neo_prefix("1.21.1").unwrap(), "21.1.");
        assert_eq!(neo_prefix("1.21").unwrap(), "21.0.");
        assert!(neo_prefix("24w14a").is_err());
    }

    #[test]
    fn versions_sort_numerically() {
        let mut v = vec!["21.1.9", "21.1.172", "21.1.10"];
        v.sort_by_key(|s| ver_key(s));
        assert_eq!(v, vec!["21.1.9", "21.1.10", "21.1.172"]);
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 10), "ab");
    }
}
