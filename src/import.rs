//! Importing modpacks: Modrinth (.mrpack) and CurseForge (.zip).
use crate::config::{instances_dir, now, Instance, Loader};
use crate::net::{download, Job, Progress};
use crate::util::slug;
use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

/// Limit on extracted config files (protection against zip bombs).
const MAX_OVERRIDES_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackKind {
    Mrpack,
    CurseForge,
}

#[derive(Clone, Debug)]
pub struct PackInfo {
    pub kind: PackKind,
    pub name: String,
    pub mc_version: String,
    pub loader: Loader,
    pub loader_version: String,
    pub files: usize,
}

// ---------- Reading the manifest ----------

fn read_json(z: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<Value> {
    let mut f = z.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
}

/// Quickly reads the archive and shows what is inside. Downloads nothing.
pub fn inspect(path: &Path) -> Result<PackInfo> {
    let file = fs::File::open(path).map_err(|_| anyhow!("File not found: {}", path.display()))?;
    let mut z = zip::ZipArchive::new(file)
        .map_err(|_| anyhow!("This is not a modpack archive. A .mrpack or .zip file is required"))?;
    if let Some(v) = read_json(&mut z, "modrinth.index.json") {
        return parse_mrpack(&v);
    }
    if let Some(v) = read_json(&mut z, "manifest.json") {
        return parse_cf(&v);
    }
    bail!("The archive has no modrinth.index.json or manifest.json. This is not a Modrinth or CurseForge modpack")
}

fn parse_mrpack(v: &Value) -> Result<PackInfo> {
    let deps = &v["dependencies"];
    let mc = deps["minecraft"]
        .as_str()
        .ok_or_else(|| anyhow!("The modpack does not specify a Minecraft version"))?
        .to_string();
    let (loader, lv) = if let Some(x) = deps["neoforge"].as_str() {
        (Loader::NeoForge, x)
    } else if let Some(x) = deps["forge"].as_str() {
        (Loader::Forge, x)
    } else if let Some(x) = deps["fabric-loader"].as_str() {
        (Loader::Fabric, x)
    } else if let Some(x) = deps["quilt-loader"].as_str() {
        (Loader::Quilt, x)
    } else {
        (Loader::Vanilla, "")
    };
    Ok(PackInfo {
        kind: PackKind::Mrpack,
        name: v["name"].as_str().unwrap_or("Imported modpack").to_string(),
        mc_version: mc,
        loader,
        loader_version: lv.to_string(),
        files: v["files"].as_array().map_or(0, |a| a.len()),
    })
}

fn parse_cf(v: &Value) -> Result<PackInfo> {
    let mc = v["minecraft"]["version"]
        .as_str()
        .ok_or_else(|| anyhow!("The modpack does not specify a Minecraft version"))?
        .to_string();
    let loaders = v["minecraft"]["modLoaders"].as_array();
    let pick = loaders.and_then(|a| {
        a.iter()
            .find(|l| l["primary"].as_bool().unwrap_or(false))
            .or_else(|| a.first())
    });
    let (loader, lv) = match pick.and_then(|l| l["id"].as_str()) {
        None => (Loader::Vanilla, String::new()),
        Some(id) => match id.split_once('-') {
            Some(("forge", ver)) => (Loader::Forge, ver.to_string()),
            Some(("neoforge", ver)) => (Loader::NeoForge, ver.to_string()),
            Some(("fabric", ver)) => (Loader::Fabric, ver.to_string()),
            Some(("quilt", ver)) => (Loader::Quilt, ver.to_string()),
            _ => bail!("Loader '{id}' is not supported"),
        },
    };
    Ok(PackInfo {
        kind: PackKind::CurseForge,
        name: v["name"].as_str().unwrap_or("Imported modpack").to_string(),
        mc_version: mc,
        loader,
        loader_version: lv,
        files: v["files"].as_array().map_or(0, |a| a.len()),
    })
}

// ---------- Safety ----------

/// A relative path without “..”, drive letters and absolute parts.
fn safe_rel(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => out.push(s),
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    rest.split(|c: char| c == '/' || c == ':' || c == '?' || c == '#').next()
}

/// For .mrpack the format only allows downloads from these hosts.
fn mrpack_host_ok(url: &str) -> bool {
    host_of(url).map_or(false, |h| {
        ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"].contains(&h)
    })
}

fn cf_host_ok(url: &str) -> bool {
    host_of(url).map_or(false, |h| h == "forgecdn.net" || h.ends_with(".forgecdn.net"))
}

// ---------- Import ----------

/// Performs the import. Returns the finished instance (it still has to be saved to the config)
/// and the list of files that could not be downloaded.
pub fn run(
    http: &Client,
    path: &Path,
    cf_key: &str,
    progress: &Progress,
) -> Result<(Instance, Vec<String>)> {
    let info = inspect(path)?;
    let base = format!("{}-{}", slug(&info.name), now());
    let mut id = base.clone();
    let mut n = 2;
    while instances_dir().join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let dir = instances_dir().join(&id);
    match run_inner(http, path, cf_key, progress, &info, &dir) {
        Ok(warnings) => Ok((
            Instance {
                id,
                name: info.name,
                mc_version: info.mc_version,
                loader: info.loader,
                loader_version: info.loader_version,
                ram_mb: 0,
                jvm_args: String::new(),
                last_played: 0,
                auto_perf: false,
                playtime_secs: 0,
                launches: 0,
                extra: Default::default(),
            },
            warnings,
        )),
        Err(e) => {
            // An unfinished instance doesn't stay on disk.
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

fn run_inner(
    http: &Client,
    path: &Path,
    cf_key: &str,
    progress: &Progress,
    info: &PackInfo,
    dir: &Path,
) -> Result<Vec<String>> {
    fs::create_dir_all(dir.join("mods"))?;
    let mut z = zip::ZipArchive::new(fs::File::open(path)?)?;
    let empty: Vec<Value> = vec![];
    let mut jobs: Vec<Job> = vec![];
    let mut warnings: Vec<String> = vec![];

    let prefixes: Vec<String> = match info.kind {
        PackKind::Mrpack => {
            let idx = read_json(&mut z, "modrinth.index.json").context("No modrinth.index.json")?;
            for f in idx["files"].as_array().unwrap_or(&empty) {
                if f["env"]["client"].as_str() == Some("unsupported") {
                    continue;
                }
                let Some(p) = f["path"].as_str() else { continue };
                let Some(rel) = safe_rel(p) else {
                    warnings.push(format!("{p}: unsafe path, skipped"));
                    continue;
                };
                let url = f["downloads"]
                    .as_array()
                    .and_then(|a| a.iter().filter_map(|u| u.as_str()).find(|u| mrpack_host_ok(u)));
                let Some(url) = url else {
                    warnings.push(format!("{p}: no allowed download link"));
                    continue;
                };
                jobs.push(Job {
                    url: url.to_string(),
                    dest: dir.join(rel),
                    sha1: f["hashes"]["sha1"].as_str().map(String::from),
                    size: f["fileSize"].as_u64(),
                });
            }
            vec!["overrides".to_string(), "client-overrides".to_string()]
        }
        PackKind::CurseForge => {
            let m = read_json(&mut z, "manifest.json").context("No manifest.json")?;
            let ids: Vec<u64> = m["files"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(|f| f["fileID"].as_u64())
                .collect();
            if !ids.is_empty() {
                if cf_key.is_empty() {
                    bail!("CurseForge modpacks need an API key: Settings → Mods");
                }
                progress.label("Getting the mod list from CurseForge…");
                let mut found: HashSet<u64> = HashSet::new();
                for chunk in ids.chunks(100) {
                    let resp: Value = http
                        .post("https://api.curseforge.com/v1/mods/files")
                        .header("x-api-key", cf_key)
                        .json(&json!({ "fileIds": chunk }))
                        .send()?
                        .error_for_status()?
                        .json()?;
                    for f in resp["data"].as_array().unwrap_or(&empty) {
                        let Some(fid) = f["id"].as_u64() else { continue };
                        found.insert(fid);
                        let Some(name) = f["fileName"].as_str() else { continue };
                        let Some(safe) = safe_rel(name).filter(|p| p.components().count() == 1) else {
                            warnings.push(format!("{name}: suspicious file name, skipped"));
                            continue;
                        };
                        let url = match f["downloadUrl"].as_str() {
                            Some(u) if !u.is_empty() && cf_host_ok(u) => u.to_string(),
                            // The author disabled link delivery via the API — trying the direct CDN address.
                            _ => format!(
                                "https://edge.forgecdn.net/files/{}/{}/{}",
                                fid / 1000,
                                fid % 1000,
                                name
                            ),
                        };
                        let sub = if name.to_lowercase().ends_with(".jar") {
                            "mods"
                        } else {
                            "resourcepacks"
                        };
                        let sha1 = f["hashes"]
                            .as_array()
                            .and_then(|a| a.iter().find(|h| h["algo"].as_u64() == Some(1)))
                            .and_then(|h| h["value"].as_str())
                            .map(String::from);
                        jobs.push(Job {
                            url,
                            dest: dir.join(sub).join(safe),
                            sha1,
                            size: f["fileLength"].as_u64(),
                        });
                    }
                }
                for fid in &ids {
                    if !found.contains(fid) {
                        warnings.push(format!("File {fid} not found on CurseForge"));
                    }
                }
            }
            vec![m["overrides"].as_str().unwrap_or("overrides").to_string()]
        }
    };

    progress.label("Copying the modpack settings…");
    extract_overrides(&mut z, &prefixes, dir)?;

    progress.label(format!("Downloading mods and files ({})…", jobs.len()));
    warnings.extend(download_soft(http, jobs, progress)?);
    Ok(warnings)
}

/// Copies the overrides folders into the instance. Paths are checked for escaping the folder.
fn extract_overrides(z: &mut zip::ZipArchive<fs::File>, prefixes: &[String], dir: &Path) -> Result<()> {
    let mut total: u64 = 0;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let Some(rel) = f.enclosed_name() else { continue };
        let mut out_rel: Option<PathBuf> = None;
        for p in prefixes {
            if let Ok(s) = rel.strip_prefix(p) {
                out_rel = Some(s.to_path_buf());
                break;
            }
        }
        let Some(out_rel) = out_rel else { continue };
        if out_rel.as_os_str().is_empty() {
            continue;
        }
        total += f.size();
        if total > MAX_OVERRIDES_BYTES {
            bail!("The modpack's config files are too large (over 2 GB)");
        }
        let dest = dir.join(&out_rel);
        if !dest.starts_with(dir) {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&dest)?;
        std::io::copy(&mut f, &mut out)?;
    }
    Ok(())
}

/// Parallel download: errors of individual files don't break the whole import but go into the report.
fn download_soft(http: &Client, jobs: Vec<Job>, progress: &Progress) -> Result<Vec<String>> {
    if jobs.is_empty() {
        return Ok(vec![]);
    }
    progress.add_total(jobs.len());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build()?;
    let failed: Vec<String> = pool.install(|| {
        jobs.par_iter()
            .filter_map(|j| {
                let r = download(http, j);
                progress.tick();
                r.err().map(|e| {
                    let name = j
                        .dest
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    format!("{name}: failed to download ({e})")
                })
            })
            .collect()
    });
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paths() {
        assert_eq!(safe_rel("mods/a.jar").unwrap(), PathBuf::from("mods").join("a.jar"));
        assert!(safe_rel("../evil.jar").is_none());
        assert!(safe_rel("mods/../../evil.jar").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("").is_none());
    }

    #[test]
    fn hosts_are_checked_strictly() {
        assert!(mrpack_host_ok("https://cdn.modrinth.com/data/x/versions/1/a.jar"));
        assert!(mrpack_host_ok("https://github.com/a/b/releases/download/1/a.jar"));
        assert!(!mrpack_host_ok("http://cdn.modrinth.com/a.jar"));
        assert!(!mrpack_host_ok("https://evil.com/cdn.modrinth.com/a.jar"));
        assert!(!mrpack_host_ok("https://cdn.modrinth.com@evil.com/a.jar"));
        assert!(cf_host_ok("https://edge.forgecdn.net/files/1/2/a.jar"));
        assert!(!cf_host_ok("https://forgecdn.net.evil.com/a.jar"));
        assert!(!cf_host_ok("https://evilforgecdn.net/a.jar"));
    }

    #[test]
    fn mrpack_manifest() {
        let v = json!({
            "name": "Tech", "files": [{}, {}],
            "dependencies": {"minecraft": "1.21.1", "neoforge": "21.1.50"}
        });
        let i = parse_mrpack(&v).unwrap();
        assert_eq!(i.kind, PackKind::Mrpack);
        assert_eq!(i.mc_version, "1.21.1");
        assert_eq!(i.loader, Loader::NeoForge);
        assert_eq!(i.loader_version, "21.1.50");
        assert_eq!(i.files, 2);
        assert!(parse_mrpack(&json!({"dependencies": {}})).is_err());
    }

    #[test]
    fn curseforge_manifest() {
        let v = json!({
            "name": "Pack",
            "minecraft": {"version": "1.20.1", "modLoaders": [{"id": "forge-47.2.0", "primary": true}]},
            "files": [{"projectID": 1, "fileID": 2}]
        });
        let i = parse_cf(&v).unwrap();
        assert_eq!(i.loader, Loader::Forge);
        assert_eq!(i.loader_version, "47.2.0");
        assert_eq!(i.files, 1);

        let weird = json!({
            "minecraft": {"version": "1.12.2", "modLoaders": [{"id": "liteloader-1", "primary": true}]}
        });
        assert!(parse_cf(&weird).is_err());
    }
}
