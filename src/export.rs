//! Export an instance to the Modrinth format (.mrpack).
use crate::config::{data_dir, Instance, Loader};
use crate::net::Progress;
use crate::util::slug;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// We don't let the archive grow beyond this size (configs, resource packs, shaders).
const MAX_EXTRA_BYTES: u64 = 1_500 * 1024 * 1024;

pub struct ExportOpts {
    pub name: String,
    pub version: String,
    pub embed_unknown: bool,
    pub include_configs: bool,
}

pub struct ExportResult {
    pub path: PathBuf,
    pub from_modrinth: usize,
    pub embedded: usize,
    pub warnings: Vec<String>,
}

pub fn exports_dir() -> PathBuf {
    let d = data_dir().join("exports");
    let _ = fs::create_dir_all(&d);
    d
}

fn hash_file(p: &Path) -> Result<(String, String)> {
    let mut f = fs::File::open(p)?;
    let mut s1 = Sha1::new();
    let mut s5 = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        s1.update(&buf[..n]);
        s5.update(&buf[..n]);
    }
    Ok((format!("{:x}", s1.finalize()), format!("{:x}", s5.finalize())))
}

/// Extracts the loader version from the profile id (“fabric-loader-0.16.5-1.21.1” → “0.16.5”).
pub fn loader_version_from_id(loader: Loader, mc: &str, id: &str) -> Option<String> {
    let v: String = match loader {
        Loader::Vanilla => return Some(String::new()),
        Loader::Fabric => {
            let r = id.strip_prefix("fabric-loader-")?;
            r.strip_suffix(&format!("-{mc}")).unwrap_or(r).to_string()
        }
        Loader::Quilt => {
            let r = id.strip_prefix("quilt-loader-")?;
            r.strip_suffix(&format!("-{mc}")).unwrap_or(r).to_string()
        }
        Loader::NeoForge => id.strip_prefix("neoforge-")?.to_string(),
        Loader::Forge => id.split_once("-forge-")?.1.to_string(),
    };
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn loader_version(inst: &Instance) -> Result<String> {
    if !inst.loader_version.trim().is_empty() {
        return Ok(inst.loader_version.trim().to_string());
    }
    let id = fs::read_to_string(inst.dir().join(".ferrum_version"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    loader_version_from_id(inst.loader, &inst.mc_version, &id).ok_or_else(|| {
        anyhow!("Unknown loader version. Launch the instance at least once or set the version in \"Edit\"")
    })
}

fn loader_key(l: Loader) -> Option<&'static str> {
    match l {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric-loader"),
        Loader::Quilt => Some("quilt-loader"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    }
}

fn safe_version(v: &str) -> String {
    let s: String = v
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "1.0.0".to_string()
    } else {
        s
    }
}

fn add_file(w: &mut ZipWriter<fs::File>, o: &SimpleFileOptions, p: &Path, zip_name: &str) -> Result<()> {
    w.start_file(zip_name, o.clone())?;
    let mut f = fs::File::open(p)?;
    std::io::copy(&mut f, w)?;
    Ok(())
}

fn add_dir(
    w: &mut ZipWriter<fs::File>,
    o: &SimpleFileOptions,
    dir: &Path,
    prefix: &str,
    total: &mut u64,
) -> Result<()> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Ok(());
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            add_dir(w, o, &p, &format!("{prefix}/{name}"), total)?;
        } else if p.is_file() {
            *total += e.metadata().map(|m| m.len()).unwrap_or(0);
            if *total > MAX_EXTRA_BYTES {
                bail!("Settings, resource packs and shaders weigh more than 1.5 GB. Turn off \"Include settings\" or remove some of them");
            }
            add_file(w, o, &p, &format!("{prefix}/{name}"))?;
        }
    }
    Ok(())
}

pub fn run(http: &Client, inst: &Instance, opts: &ExportOpts, progress: &Progress) -> Result<ExportResult> {
    let lv = loader_version(inst)?;
    let mods_dir = inst.mods_dir();

    // 1. Compute hashes of the enabled mods.
    progress.label("Computing mod checksums…");
    let mut jars: Vec<(PathBuf, String, String, String, u64)> = vec![]; // path, name, sha1, sha512, size
    if let Ok(rd) = fs::read_dir(&mods_dir) {
        let mut paths: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map_or(false, |x| x == "jar"))
            .collect();
        paths.sort();
        progress.add_total(paths.len());
        for p in paths {
            let (s1, s5) = hash_file(&p)?;
            let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            jars.push((p, name, s1, s5, size));
            progress.tick();
        }
    }

    // 2. Ask Modrinth which of them it has.
    progress.label("Looking up mods on Modrinth…");
    let found: Value = if jars.is_empty() {
        json!({})
    } else {
        let hashes: Vec<&str> = jars.iter().map(|j| j.2.as_str()).collect();
        http.post("https://api.modrinth.com/v2/version_files")
            .json(&json!({ "hashes": hashes, "algorithm": "sha1" }))
            .send()?
            .error_for_status()?
            .json()?
    };

    let mut files: Vec<Value> = vec![];
    let mut unknown: Vec<&(PathBuf, String, String, String, u64)> = vec![];
    for j in &jars {
        let url = found[j.2.as_str()]["files"]
            .as_array()
            .and_then(|a| a.iter().find(|f| f["hashes"]["sha1"].as_str() == Some(j.2.as_str())))
            .and_then(|f| f["url"].as_str())
            .filter(|u| u.starts_with("https://cdn.modrinth.com/"));
        match url {
            Some(u) => files.push(json!({
                "path": format!("mods/{}", j.1),
                "hashes": { "sha1": j.2, "sha512": j.3 },
                "env": { "client": "required", "server": "required" },
                "downloads": [u],
                "fileSize": j.4
            })),
            None => unknown.push(j),
        }
    }

    // 3. Manifest.
    let name = if opts.name.is_empty() { inst.name.clone() } else { opts.name.clone() };
    let version = safe_version(&opts.version);
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".to_string(), json!(inst.mc_version));
    if let Some(k) = loader_key(inst.loader) {
        deps.insert(k.to_string(), json!(lv));
    }
    let index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": version,
        "name": name,
        "summary": "Exported from Ferrum Launcher",
        "files": files,
        "dependencies": deps
    });

    // 4. Write the archive.
    progress.label("Building the archive…");
    let out_path = exports_dir().join(format!("{}-{}.mrpack", slug(&name), version));
    let mut w = ZipWriter::new(fs::File::create(&out_path).context("Could not create the export file")?);
    let o = SimpleFileOptions::default();
    w.start_file("modrinth.index.json", o.clone())?;
    w.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;

    let mut warnings: Vec<String> = vec![];
    let mut embedded = 0usize;
    if opts.embed_unknown {
        for j in &unknown {
            add_file(&mut w, &o, &j.0, &format!("overrides/mods/{}", j.1))?;
            embedded += 1;
            warnings.push(format!("{}: not on Modrinth, included in the archive", j.1));
        }
    } else {
        for j in &unknown {
            warnings.push(format!("{}: not on Modrinth, left out of the archive", j.1));
        }
    }
    if opts.include_configs {
        let mut total = 0u64;
        for item in ["config", "defaultconfigs", "resourcepacks", "shaderpacks", "scripts", "kubejs"] {
            add_dir(&mut w, &o, &inst.dir().join(item), &format!("overrides/{item}"), &mut total)?;
        }
        for item in ["options.txt", "servers.dat"] {
            let p = inst.dir().join(item);
            if p.is_file() {
                add_file(&mut w, &o, &p, &format!("overrides/{item}"))?;
            }
        }
    }
    w.finish()?;

    Ok(ExportResult {
        path: out_path,
        from_modrinth: files.len(),
        embedded,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_versions_from_profile_ids() {
        assert_eq!(
            loader_version_from_id(Loader::Fabric, "1.21.1", "fabric-loader-0.16.5-1.21.1").as_deref(),
            Some("0.16.5")
        );
        assert_eq!(
            loader_version_from_id(Loader::NeoForge, "1.21.1", "neoforge-21.1.50").as_deref(),
            Some("21.1.50")
        );
        assert_eq!(
            loader_version_from_id(Loader::Forge, "1.20.1", "1.20.1-forge-47.2.0").as_deref(),
            Some("47.2.0")
        );
        assert_eq!(loader_version_from_id(Loader::Fabric, "1.21.1", "unknown"), None);
        assert_eq!(loader_version_from_id(Loader::Vanilla, "1.21.1", "").as_deref(), Some(""));
    }

    #[test]
    fn version_is_made_file_safe() {
        assert_eq!(safe_version("1.2.0"), "1.2.0");
        assert_eq!(safe_version("v 1/2"), "v-1-2");
        assert_eq!(safe_version("///"), "1.0.0");
    }

    #[test]
    fn file_hashes() {
        let p = std::env::temp_dir().join(format!("ferrum-test-hash-{}", std::process::id()));
        fs::write(&p, b"abc").unwrap();
        let (s1, s5) = hash_file(&p).unwrap();
        assert_eq!(s1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(s5.len(), 128);
        let _ = fs::remove_file(p);
    }
}
