use crate::config::{data_dir, VERSION};
use crate::net::{download_all, get_json, Job, Progress};
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Clone, Debug)]
pub struct McVersion {
    pub id: String,
    pub kind: String,
    pub url: String,
}

/// List of Minecraft versions (cached on disk so it works offline).
pub fn fetch_versions(http: &Client) -> Result<Vec<McVersion>> {
    let cache = data_dir().join("version_manifest.json");
    let m = match get_json(http, MANIFEST) {
        Ok(m) => {
            let _ = fs::write(&cache, serde_json::to_vec(&m)?);
            m
        }
        Err(e) => match fs::read(&cache)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        {
            Some(m) => m,
            None => return Err(e),
        },
    };
    let list = m["versions"].as_array().context("Empty version manifest")?;
    Ok(list
        .iter()
        .filter_map(|v| {
            Some(McVersion {
                id: v["id"].as_str()?.to_string(),
                kind: v["type"].as_str()?.to_string(),
                url: v["url"].as_str()?.to_string(),
            })
        })
        .collect())
}

pub fn version_json_path(root: &Path, id: &str) -> PathBuf {
    root.join("versions").join(id).join(format!("{id}.json"))
}

pub fn ensure_vanilla_json(http: &Client, root: &Path, mc: &str) -> Result<()> {
    let p = version_json_path(root, mc);
    if p.exists() {
        return Ok(());
    }
    let list = fetch_versions(http)?;
    let v = list
        .iter()
        .find(|v| v.id == mc)
        .ok_or_else(|| anyhow!("Minecraft version {mc} not found"))?;
    let json = get_json(http, &v.url)?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&p, serde_json::to_vec(&json)?)?;
    Ok(())
}

pub fn ensure_client_jar(http: &Client, root: &Path, v: &Value, progress: &Progress) -> Result<()> {
    let id = v["id"].as_str().unwrap_or("");
    if let Some(c) = v["downloads"].get("client") {
        progress.label("Downloading the Minecraft client…");
        let job = Job {
            url: c["url"].as_str().unwrap_or("").to_string(),
            dest: root.join("versions").join(id).join(format!("{id}.jar")),
            sha1: c["sha1"].as_str().map(String::from),
            size: c["size"].as_u64(),
        };
        download_all(http, vec![job], progress)?;
    }
    Ok(())
}

// ---------- Merging profiles (inheritsFrom) ----------

/// Reads a version profile; if it inherits (Fabric/Forge…), merges it with its parent.
pub fn resolve(root: &Path, id: &str) -> Result<Value> {
    let p = version_json_path(root, id);
    let bytes = fs::read(&p).with_context(|| format!("Missing file {}", p.display()))?;
    let mut v: Value = serde_json::from_slice(&bytes)?;
    let parent = v
        .get("inheritsFrom")
        .and_then(|x| x.as_str())
        .map(String::from);
    match parent {
        Some(pid) => {
            let base = resolve(root, &pid)?;
            Ok(merge(base, v))
        }
        None => {
            v["_jar"] = json!(id);
            Ok(v)
        }
    }
}

fn merge(mut base: Value, child: Value) -> Value {
    let Value::Object(child) = child else {
        return base;
    };
    // Libraries: the loader's first, then the vanilla ones.
    let mut libs: Vec<Value> = child
        .get("libraries")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();
    libs.extend(base["libraries"].as_array().cloned().unwrap_or_default());

    for key in ["game", "jvm"] {
        let mut a: Vec<Value> = base["arguments"][key].as_array().cloned().unwrap_or_default();
        if let Some(c) = child
            .get("arguments")
            .and_then(|x| x.get(key))
            .and_then(|x| x.as_array())
        {
            a.extend(c.iter().cloned());
        }
        if !a.is_empty() {
            base["arguments"][key] = Value::Array(a);
        }
    }
    for (k, val) in child {
        if k == "libraries" || k == "arguments" {
            continue;
        }
        base[k.as_str()] = val;
    }
    base["libraries"] = Value::Array(libs);
    base
}

// ---------- Rules ----------

pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn os_matches(os: &Value) -> bool {
    if let Some(n) = os["name"].as_str() {
        if n != os_name() {
            return false;
        }
    }
    if let Some(a) = os["arch"].as_str() {
        let ok = match a {
            "x86" => cfg!(target_arch = "x86"),
            "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
            _ => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

pub fn rules_allow(rules: &Value) -> bool {
    let Some(arr) = rules.as_array() else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in arr {
        // Rules with features (demo, quick play…) are not enabled.
        let applies = r.get("features").is_none() && r.get("os").map_or(true, os_matches);
        if applies {
            allowed = r["action"].as_str() == Some("allow");
        }
    }
    allowed
}

// ---------- Libraries ----------

struct Libs {
    jobs: Vec<Job>,
    classpath: Vec<PathBuf>,
    natives: Vec<PathBuf>,
}

fn maven_path(name: &str) -> Option<String> {
    let (coords, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let p: Vec<&str> = coords.split(':').collect();
    if p.len() < 3 {
        return None;
    }
    let g = p[0].replace('.', "/");
    let (a, v) = (p[1], p[2]);
    let cls = if p.len() > 3 {
        format!("-{}", p[3])
    } else {
        String::new()
    };
    Some(format!("{g}/{a}/{v}/{a}-{v}{cls}.{ext}"))
}

/// All variants of LWJGL natives libraries that appear in manifests.
const ARCH_NATIVES: [&str; 8] = [
    "natives-windows",
    "natives-windows-arm64",
    "natives-windows-x86",
    "natives-linux",
    "natives-linux-arm64",
    "natives-linux-arm32",
    "natives-macos",
    "natives-macos-arm64",
];

fn host_native_classifier() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "-arm64"
    } else if cfg!(target_arch = "x86") {
        "-x86"
    } else {
        ""
    };
    format!("natives-{os}{arch}")
}

/// Natives libraries for another platform or architecture aren't needed: their DLLs would overwrite the right ones
/// (x64, arm64 and x86 use identical file names).
fn native_matches_host(name: &str) -> bool {
    match name.rsplit(':').next() {
        Some(c) if ARCH_NATIVES.contains(&c) => c == host_native_classifier(),
        _ => true,
    }
}

fn add_job(
    libdir: &Path,
    jobs: &mut Vec<Job>,
    seen: &mut HashSet<PathBuf>,
    url: &str,
    rel: &str,
    sha1: Option<&str>,
    size: Option<u64>,
) -> PathBuf {
    let mut dest = libdir.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        dest.push(part);
    }
    if seen.insert(dest.clone()) {
        jobs.push(Job {
            url: url.to_string(),
            dest: dest.clone(),
            sha1: sha1.map(String::from),
            size,
        });
    }
    dest
}

fn collect_libs(root: &Path, v: &Value) -> Libs {
    let libdir = root.join("libraries");
    let mut out = Libs {
        jobs: vec![],
        classpath: vec![],
        natives: vec![],
    };
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let empty = vec![];
    let libs = v["libraries"].as_array().unwrap_or(&empty);

    for lib in libs {
        if !rules_allow(&lib["rules"]) {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");

        // Old format: natives classifiers.
        if let Some(nat) = lib["natives"].get(os_name()).and_then(|x| x.as_str()) {
            let arch = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
            let key = nat.replace("${arch}", arch);
            if let Some(c) = lib["downloads"]["classifiers"].get(key.as_str()) {
                if let (Some(url), Some(path)) = (c["url"].as_str(), c["path"].as_str()) {
                    let d = add_job(
                        &libdir,
                        &mut out.jobs,
                        &mut seen,
                        url,
                        path,
                        c["sha1"].as_str(),
                        c["size"].as_u64(),
                    );
                    out.natives.push(d);
                }
            }
        }

        if let Some(dl) = lib.get("downloads") {
            if let Some(a) = dl.get("artifact") {
                if !native_matches_host(name) {
                    continue;
                }
                if let Some(path) = a["path"].as_str() {
                    let d = add_job(
                        &libdir,
                        &mut out.jobs,
                        &mut seen,
                        a["url"].as_str().unwrap_or(""),
                        path,
                        a["sha1"].as_str(),
                        a["size"].as_u64(),
                    );
                    if name.contains(":natives-") {
                        out.natives.push(d.clone());
                    }
                    if !out.classpath.contains(&d) {
                        out.classpath.push(d);
                    }
                }
            }
        } else if let Some(rel) = maven_path(name) {
            // Fabric/Quilt format: only the name and the maven repository URL.
            let base = lib["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
            let base = if base.ends_with('/') {
                base.to_string()
            } else {
                format!("{base}/")
            };
            let d = add_job(
                &libdir,
                &mut out.jobs,
                &mut seen,
                &format!("{base}{rel}"),
                &rel,
                lib["sha1"].as_str(),
                lib["size"].as_u64(),
            );
            if !out.classpath.contains(&d) {
                out.classpath.push(d);
            }
        }
    }
    out
}

fn extract_natives(jars: &[PathBuf], dir: &Path) -> Result<()> {
    // Marker “.ok2”: old folders (with mixed DLLs of different platforms) are extracted again.
    let marker = dir.join(".ok2");
    if marker.exists() {
        return Ok(());
    }
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
    for j in jars {
        if !j.exists() {
            continue;
        }
        let mut z = zip::ZipArchive::new(fs::File::open(j)?)?;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            if name.starts_with("META-INF") {
                continue;
            }
            let lower = name.to_lowercase();
            let is_native = lower.ends_with(".dll")
                || lower.ends_with(".so")
                || lower.ends_with(".dylib")
                || lower.ends_with(".jnilib");
            if !is_native {
                continue;
            }
            let fname = Path::new(&name)
                .file_name()
                .map(|s| s.to_owned())
                .unwrap_or_default();
            let mut out = fs::File::create(dir.join(fname))?;
            std::io::copy(&mut f, &mut out)?;
        }
    }
    fs::write(marker, b"ok")?;
    Ok(())
}

// ---------- Assets (sounds, languages…) ----------

fn prepare_assets(
    http: &Client,
    root: &Path,
    game_dir: &Path,
    v: &Value,
    progress: &Progress,
) -> Result<(String, PathBuf)> {
    let ai = &v["assetIndex"];
    let id = ai["id"]
        .as_str()
        .or(v["assets"].as_str())
        .unwrap_or("legacy")
        .to_string();
    let assets = root.join("assets");
    let idx_path = assets.join("indexes").join(format!("{id}.json"));
    progress.label("Checking game assets…");
    download_all(
        http,
        vec![Job {
            url: ai["url"].as_str().unwrap_or("").to_string(),
            dest: idx_path.clone(),
            sha1: ai["sha1"].as_str().map(String::from),
            size: ai["size"].as_u64(),
        }],
        progress,
    )?;
    let idx: Value = serde_json::from_slice(&fs::read(&idx_path)?)?;
    let objects = idx["objects"].as_object().context("Empty assets index")?;

    let mut jobs = vec![];
    let mut seen = HashSet::new();
    for (_, o) in objects {
        let Some(h) = o["hash"].as_str() else { continue };
        let Some(pre) = h.get(..2) else { continue };
        if !seen.insert(h.to_string()) {
            continue;
        }
        jobs.push(Job {
            url: format!("https://resources.download.minecraft.net/{pre}/{h}"),
            dest: assets.join("objects").join(pre).join(h),
            sha1: None,
            size: o["size"].as_u64(),
        });
    }
    download_all(http, jobs, progress)?;

    // Old versions (before 1.7.x) expect assets at regular paths.
    let is_virtual = idx["virtual"].as_bool().unwrap_or(false);
    let map_res = idx["map_to_resources"].as_bool().unwrap_or(false);
    let mut game_assets = assets.clone();
    if is_virtual || map_res {
        let target = if map_res {
            game_dir.join("resources")
        } else {
            assets.join("virtual").join("legacy")
        };
        for (name, o) in objects {
            if let Some(h) = o["hash"].as_str() {
                let dest = target.join(name);
                if !dest.exists() {
                    if let Some(p) = dest.parent() {
                        fs::create_dir_all(p)?;
                    }
                    fs::copy(assets.join("objects").join(h.get(..2).unwrap_or("00")).join(h), &dest)?;
                }
            }
        }
        if is_virtual {
            game_assets = target;
        }
    }
    Ok((id, game_assets))
}

// ---------- Preparing files and building arguments ----------

pub struct Prepared {
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub assets_index: String,
    pub game_assets: PathBuf,
}

pub fn prepare(
    http: &Client,
    root: &Path,
    game_dir: &Path,
    v: &Value,
    progress: &Progress,
) -> Result<Prepared> {
    let jar_id = v["_jar"].as_str().unwrap_or("").to_string();
    let jar = root.join("versions").join(&jar_id).join(format!("{jar_id}.jar"));

    progress.label("Checking libraries…");
    let mut jobs = vec![];
    if let Some(c) = v["downloads"].get("client") {
        jobs.push(Job {
            url: c["url"].as_str().unwrap_or("").to_string(),
            dest: jar.clone(),
            sha1: c["sha1"].as_str().map(String::from),
            size: c["size"].as_u64(),
        });
    }
    let libs = collect_libs(root, v);
    jobs.extend(libs.jobs);
    download_all(http, jobs, progress)?;

    let natives_dir = root.join("natives").join(v["id"].as_str().unwrap_or("default"));
    extract_natives(&libs.natives, &natives_dir)?;

    let (assets_index, game_assets) = prepare_assets(http, root, game_dir, v, progress)?;
    let mut classpath = libs.classpath;
    classpath.push(jar);
    Ok(Prepared {
        classpath,
        natives_dir,
        assets_index,
        game_assets,
    })
}

pub struct LaunchOpts {
    pub game_dir: PathBuf,
    pub ram_mb: u32,
    pub extra_jvm: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub name: String,
    pub uuid: String,
    /// The Minecraft access token of a Microsoft account; None = offline account.
    pub access_token: Option<String>,
}

fn collect_args(v: &Value) -> Vec<String> {
    let mut out = vec![];
    let Some(arr) = v.as_array() else {
        return out;
    };
    for item in arr {
        match item {
            Value::String(s) => out.push(s.clone()),
            Value::Object(_) => {
                if !rules_allow(&item["rules"]) {
                    continue;
                }
                match &item["value"] {
                    Value::String(s) => out.push(s.clone()),
                    Value::Array(a) => {
                        out.extend(a.iter().filter_map(|x| x.as_str().map(String::from)))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn subst(arg: &str, vars: &HashMap<&str, String>) -> String {
    let mut s = arg.to_string();
    for (k, val) in vars {
        s = s.replace(&format!("${{{}}}", k), val);
    }
    s
}

fn ps(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// The final command line: [JVM arguments…, main class, game arguments…]
pub fn build_args(v: &Value, p: &Prepared, root: &Path, o: &LaunchOpts) -> Vec<String> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let raw_jvm = collect_args(&v["arguments"]["jvm"]);
    // Modern Forge and NeoForge (-DignoreList) attach the client themselves via a patched jar.
    // The vanilla “1.21.1.jar” on the classpath turns into an extra module “_1._21._1” for them
    // and breaks the launch (“Modules minecraft and _1._21._1 export package…”).
    let drop_vanilla_jar = raw_jvm.iter().any(|a| a.contains("-DignoreList"));
    let jar_id = v["_jar"].as_str().unwrap_or("");
    let vanilla_jar = root
        .join("versions")
        .join(jar_id)
        .join(format!("{jar_id}.jar"));
    let cp = p
        .classpath
        .iter()
        .filter(|x| !(drop_vanilla_jar && !jar_id.is_empty() && **x == vanilla_jar))
        .map(|x| ps(x))
        .collect::<Vec<_>>()
        .join(sep);

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", o.name.clone());
    vars.insert("auth_uuid", o.uuid.clone());
    // an online (Microsoft) account passes its token; an offline one passes placeholders
    let token = o.access_token.clone().unwrap_or_else(|| "0".to_string());
    vars.insert("auth_access_token", token.clone());
    vars.insert("auth_session", token);
    vars.insert("auth_xuid", "0".into());
    vars.insert("clientid", "0".into());
    vars.insert(
        "user_type",
        if o.access_token.is_some() { "msa" } else { "legacy" }.into(),
    );
    vars.insert("user_properties", "{}".into());
    vars.insert("version_name", v["id"].as_str().unwrap_or("").to_string());
    vars.insert(
        "version_type",
        v["type"].as_str().unwrap_or("release").to_string(),
    );
    vars.insert("game_directory", ps(&o.game_dir));
    vars.insert("assets_root", ps(&root.join("assets")));
    vars.insert("assets_index_name", p.assets_index.clone());
    vars.insert("game_assets", ps(&p.game_assets));
    vars.insert("natives_directory", ps(&p.natives_dir));
    vars.insert("library_directory", ps(&root.join("libraries")));
    vars.insert("classpath_separator", sep.to_string());
    vars.insert("classpath", cp.clone());
    vars.insert("launcher_name", "FerrumLauncher".into());
    vars.insert("launcher_version", VERSION.into());

    let mut jvm: Vec<String> = vec![
        format!("-Xmx{}M", o.ram_mb),
        format!("-Xms{}M", (o.ram_mb / 4).max(256).min(o.ram_mb)),
    ];
    jvm.extend(o.extra_jvm.split_whitespace().map(String::from));

    let mut vjvm = raw_jvm;
    if vjvm.is_empty() {
        vjvm = vec![
            "-Djava.library.path=${natives_directory}".into(),
            "-cp".into(),
            "${classpath}".into(),
        ];
    }
    jvm.extend(vjvm.iter().map(|a| subst(a, &vars)));
    if !jvm.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm.push("-cp".into());
        jvm.push(cp);
    }

    let main = v["mainClass"]
        .as_str()
        .unwrap_or("net.minecraft.client.main.Main")
        .to_string();

    let raw_game: Vec<String> = if v["arguments"]["game"].is_array() {
        collect_args(&v["arguments"]["game"])
    } else {
        v["minecraftArguments"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .map(String::from)
            .collect()
    };
    let mut game: Vec<String> = raw_game.iter().map(|a| subst(a, &vars)).collect();
    if o.fullscreen {
        game.push("--fullscreen".into());
    } else if o.width > 0 && o.height > 0 {
        game.push("--width".into());
        game.push(o.width.to_string());
        game.push("--height".into());
        game.push(o.height.to_string());
    }

    let mut args = jvm;
    args.push(main);
    args.extend(game);
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_basic() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.5").unwrap(),
            "net/fabricmc/fabric-loader/0.16.5/fabric-loader-0.16.5.jar"
        );
    }

    #[test]
    fn maven_classifier_and_extension() {
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-linux").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar"
        );
        assert_eq!(maven_path("a.b:c:1.0@zip").unwrap(), "a/b/c/1.0/c-1.0.zip");
        assert!(maven_path("broken").is_none());
    }

    #[test]
    fn rules_without_rules_allow() {
        assert!(rules_allow(&Value::Null));
        assert!(rules_allow(&json!([])));
    }

    #[test]
    fn rules_follow_os_and_features() {
        let me = os_name();
        let other = if me == "windows" { "linux" } else { "windows" };
        assert!(rules_allow(&json!([{"action": "allow", "os": {"name": me}}])));
        assert!(!rules_allow(&json!([{"action": "allow", "os": {"name": other}}])));
        assert!(!rules_allow(&json!([
            {"action": "allow"},
            {"action": "disallow", "os": {"name": me}}
        ])));
        assert!(!rules_allow(&json!([
            {"action": "allow", "features": {"is_demo_user": true}}
        ])));
    }

    #[test]
    fn merge_puts_loader_libraries_first_and_joins_arguments() {
        let base = json!({
            "id": "1.21.1", "mainClass": "A", "_jar": "1.21.1",
            "libraries": [{"name": "vanilla:lib:1"}],
            "arguments": {"game": ["--a"], "jvm": ["-x"]}
        });
        let child = json!({
            "id": "fabric-1", "inheritsFrom": "1.21.1", "mainClass": "B",
            "libraries": [{"name": "fabric:lib:1"}],
            "arguments": {"game": ["--b"]}
        });
        let m = merge(base, child);
        assert_eq!(m["libraries"][0]["name"], "fabric:lib:1");
        assert_eq!(m["libraries"][1]["name"], "vanilla:lib:1");
        assert_eq!(m["arguments"]["game"], json!(["--a", "--b"]));
        assert_eq!(m["arguments"]["jvm"], json!(["-x"]));
        assert_eq!(m["mainClass"], "B");
        assert_eq!(m["id"], "fabric-1");
        assert_eq!(m["_jar"], "1.21.1");
    }

    #[test]
    fn modern_forge_does_not_get_vanilla_jar_on_classpath() {
        let root = PathBuf::from("root");
        let vanilla = root.join("versions").join("1.21.1").join("1.21.1.jar");
        let v = json!({
            "id": "neoforge-21.1.267", "_jar": "1.21.1", "type": "release",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "arguments": {
                "jvm": ["-DignoreList=client-extra,${version_name}.jar", "-cp", "${classpath}"],
                "game": ["--launchTarget", "forgeclient"]
            }
        });
        let p = Prepared {
            classpath: vec![PathBuf::from("lib.jar"), vanilla],
            natives_dir: PathBuf::from("nat"),
            assets_index: "17".into(),
            game_assets: PathBuf::from("as"),
        };
        let o = LaunchOpts {
            game_dir: PathBuf::from("gd"),
            ram_mb: 2048,
            extra_jvm: String::new(),
            width: 0,
            height: 0,
            fullscreen: false,
            name: "Steve".into(),
            uuid: "u".into(),
            access_token: None,
        };
        let a = build_args(&v, &p, &root, &o);
        let i = a.iter().position(|x| x == "-cp").unwrap();
        assert_eq!(a[i + 1], "lib.jar");
    }

    #[test]
    fn natives_for_other_platforms_are_skipped() {
        let me = host_native_classifier();
        assert!(native_matches_host(&format!("org.lwjgl:lwjgl:3.3.3:{me}")));
        for other in ARCH_NATIVES.iter().filter(|c| **c != me) {
            assert!(!native_matches_host(&format!("org.lwjgl:lwjgl:3.3.3:{other}")), "{other}");
        }
        // regular libraries and special variants are left alone
        assert!(native_matches_host("com.google.guava:guava:32.1.2-jre"));
        assert!(native_matches_host("org.lwjgl:lwjgl:3.3.3:natives-macos-patch"));
    }

    #[test]
    fn library_paths_use_one_separator() {
        let mut jobs = vec![];
        let mut seen = HashSet::new();
        let d = add_job(
            Path::new("libs"),
            &mut jobs,
            &mut seen,
            "https://x/a.jar",
            "net/neoforged/bus/8.0.5/bus-8.0.5.jar",
            None,
            None,
        );
        let expected: PathBuf = ["libs", "net", "neoforged", "bus", "8.0.5", "bus-8.0.5.jar"].iter().collect();
        assert_eq!(d, expected);
    }

    #[test]
    fn legacy_arguments_are_substituted() {
        let v = json!({
            "id": "1.8.9", "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "minecraftArguments": "--username ${auth_player_name} --gameDir ${game_directory}"
        });
        let p = Prepared {
            classpath: vec![PathBuf::from("a.jar"), PathBuf::from("b.jar")],
            natives_dir: PathBuf::from("nat"),
            assets_index: "1.8".into(),
            game_assets: PathBuf::from("as"),
        };
        let o = LaunchOpts {
            game_dir: PathBuf::from("gd"),
            ram_mb: 2048,
            extra_jvm: "-Dfoo=1".into(),
            width: 0,
            height: 0,
            fullscreen: false,
            name: "Steve".into(),
            uuid: "u".into(),
            access_token: None,
        };
        let a = build_args(&v, &p, Path::new("root"), &o);
        assert_eq!(a[0], "-Xmx2048M");
        assert!(a.contains(&"-Dfoo=1".to_string()));
        let i = a.iter().position(|x| x == "net.minecraft.client.main.Main").unwrap();
        assert!(a[..i].contains(&"-cp".to_string()));
        let rest: Vec<&str> = a[i + 1..].iter().map(|s| s.as_str()).collect();
        assert_eq!(rest, vec!["--username", "Steve", "--gameDir", "gd"]);
    }

    #[test]
    fn online_account_passes_its_token() {
        let v = json!({
            "id": "1.8.9", "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "minecraftArguments": "--accessToken ${auth_access_token} --userType ${user_type}"
        });
        let p = Prepared {
            classpath: vec![PathBuf::from("a.jar")],
            natives_dir: PathBuf::from("nat"),
            assets_index: "1.8".into(),
            game_assets: PathBuf::from("as"),
        };
        let mut o = LaunchOpts {
            game_dir: PathBuf::from("gd"),
            ram_mb: 2048,
            extra_jvm: String::new(),
            width: 0,
            height: 0,
            fullscreen: false,
            name: "Steve".into(),
            uuid: "u".into(),
            access_token: Some("SECRET".into()),
        };
        let a = build_args(&v, &p, Path::new("root"), &o);
        let i = a.iter().position(|x| x == "--accessToken").unwrap();
        assert_eq!(a[i + 1], "SECRET");
        let i = a.iter().position(|x| x == "--userType").unwrap();
        assert_eq!(a[i + 1], "msa");
        o.access_token = None;
        let a = build_args(&v, &p, Path::new("root"), &o);
        let i = a.iter().position(|x| x == "--accessToken").unwrap();
        assert_eq!(a[i + 1], "0");
        let i = a.iter().position(|x| x == "--userType").unwrap();
        assert_eq!(a[i + 1], "legacy");
    }
}
