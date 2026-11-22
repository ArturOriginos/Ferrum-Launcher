use crate::config::{runtime_dir, Settings};
use crate::net::Progress;
use anyhow::{anyhow, bail, Result};
use reqwest::blocking::Client;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// On Windows, don't show a console window for child processes.
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn exe() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Java major version: 8, 17, 21…
pub fn java_major(java: &str) -> Option<u32> {
    let mut c = Command::new(java);
    c.arg("-version");
    quiet(&mut c);
    let out = c.output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    parse_java_major(&text)
}

/// Extracts the Java major version from the `java -version` output.
fn parse_java_major(text: &str) -> Option<u32> {
    let start = text.find('"')? + 1;
    let rest = &text[start..];
    let end = rest.find('"')?;
    let ver = &rest[..end]; // "1.8.0_392", "17.0.9", "21"
    let mut parts = ver.split(|c: char| c == '.' || c == '_' || c == '-' || c == '+');
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// Java installations found on the system.
pub fn candidates() -> Vec<String> {
    let mut v = vec!["java".to_string()];
    if let Ok(h) = std::env::var("JAVA_HOME") {
        v.push(Path::new(&h).join("bin").join(exe()).to_string_lossy().to_string());
    }
    let roots: &[&str] = if cfg!(windows) {
        &[
            r"C:\Program Files\Java",
            r"C:\Program Files\Eclipse Adoptium",
            r"C:\Program Files\Microsoft",
            r"C:\Program Files\Zulu",
            r"C:\Program Files\Amazon Corretto",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Library/Java/JavaVirtualMachines"]
    } else {
        &["/usr/lib/jvm"]
    };
    for r in roots {
        if let Ok(rd) = fs::read_dir(r) {
            for e in rd.flatten() {
                let mut home = e.path();
                if cfg!(target_os = "macos") {
                    home = home.join("Contents").join("Home");
                }
                let p = home.join("bin").join(exe());
                if p.exists() {
                    v.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
    v
}

fn find_bin(dir: &Path, depth: u32) -> Option<PathBuf> {
    let cand = dir.join("bin").join(exe());
    if cand.is_file() {
        return Some(cand);
    }
    if depth == 0 {
        return None;
    }
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_bin(&p, depth - 1) {
                return Some(f);
            }
        }
    }
    None
}

/// Downloads a JRE (Eclipse Temurin) into the launcher folder if it isn't there yet.
pub fn ensure_runtime(http: &Client, major: u32, progress: &Progress) -> Result<String> {
    let dir = runtime_dir().join(format!("java-{major}"));
    if let Some(b) = find_bin(&dir, 5) {
        return Ok(b.to_string_lossy().to_string());
    }
    progress.label(format!("Downloading Java {major}…"));
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" };
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{major}/ga/{os}/{arch}/jre/hotspot/normal/eclipse"
    );
    let bytes = http.get(&url).send()?.error_for_status()?.bytes()?;
    progress.label(format!("Extracting Java {major}…"));
    fs::create_dir_all(&dir)?;
    if cfg!(windows) {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        zip.extract(&dir)?;
    } else {
        let gz = flate2::read::GzDecoder::new(&bytes[..]);
        tar::Archive::new(gz).unpack(&dir)?;
    }
    find_bin(&dir, 5)
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| anyhow!("Could not find java in the downloaded archive"))
}

/// Picks a Java for the required game version.
pub fn pick(http: &Client, st: &Settings, major: u32, progress: &Progress) -> Result<String> {
    let major = if major == 16 { 17 } else { major };
    if !st.auto_java {
        let j = if st.java_path.trim().is_empty() {
            "java".to_string()
        } else {
            st.java_path.trim().to_string()
        };
        return match java_major(&j) {
            Some(m) if m >= major => Ok(j),
            Some(m) => bail!("This version needs Java {major}+, but Java {m} is configured"),
            None => bail!("Could not run Java: {j}"),
        };
    }
    progress.label("Looking for a suitable Java…");
    for c in candidates() {
        if java_major(&c) == Some(major) {
            return Ok(c);
        }
    }
    ensure_runtime(http, major, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_versions() {
        assert_eq!(parse_java_major("openjdk version \"21.0.1\" 2023-10-17"), Some(21));
        assert_eq!(parse_java_major("openjdk version \"17\" 2021-09-14"), Some(17));
    }

    #[test]
    fn parses_java_8() {
        assert_eq!(parse_java_major("java version \"1.8.0_392\""), Some(8));
    }

    #[test]
    fn garbage_gives_none() {
        assert_eq!(parse_java_major("command not found"), None);
    }
}
