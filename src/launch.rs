use crate::config::*;
use crate::net::Progress;
use crate::{java, loaders, minecraft, perf, skin};
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::SystemTime,
};

/// Appends a timestamped line to the launch log.
fn note(path: &Path, msg: &str) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", stamp(now()), msg);
    }
}

/// Full cycle: prepare the files, launch the game, wait for it to exit.
/// Every launch writes its own log to `logs/launches/`, and on a crash everything needed
/// is copied into a separate folder in `logs/crashes/`.
pub fn run(
    http: &Client,
    st: &Settings,
    inst: &Instance,
    acc: &Account,
    progress: &Progress,
) -> Result<()> {
    let dir = launch_logs_dir();
    fs::create_dir_all(&dir)?;
    // Old logs don't pile up forever: we keep the latest launches and crashes.
    prune(&dir, 40);
    prune(&crash_logs_dir(), 20);
    let log_path = dir.join(format!("{}_{}.log", stamp(now()), inst.id));
    note(
        &log_path,
        &format!(
            "Ferrum Launcher v{VERSION}. Instance '{}', Minecraft {}, {}",
            inst.name,
            inst.mc_version,
            inst.loader.label()
        ),
    );
    let r = run_inner(http, st, inst, acc, progress, &log_path);
    if let Err(e) = &r {
        note(&log_path, &format!("ERROR: {e:#}"));
        return Err(anyhow!("{e:#}\n\nLaunch log:\n{}", log_path.display()));
    }
    r
}

fn run_inner(
    http: &Client,
    st: &Settings,
    inst: &Instance,
    acc: &Account,
    progress: &Progress,
    log: &Path,
) -> Result<()> {
    let root = game_root();
    let game_dir = inst.dir();
    fs::create_dir_all(game_dir.join("mods"))?;

    progress.label("Checking Minecraft…");
    minecraft::ensure_vanilla_json(http, &root, &inst.mc_version)?;
    let vanilla = minecraft::resolve(&root, &inst.mc_version)?;
    let major = vanilla["javaVersion"]["majorVersion"].as_u64().unwrap_or(8) as u32;

    let java_bin = java::pick(http, st, major, progress)?;
    note(
        log,
        &format!(
            "Java: {} (version {})",
            java_bin,
            java::java_major(&java_bin).map_or("?".to_string(), |m| m.to_string())
        ),
    );
    minecraft::ensure_client_jar(http, &root, &vanilla, progress)?;

    let id = loaders::resolve_id(http, &root, inst, &java_bin, progress)?;
    note(log, &format!("Profile version: {id}"));

    // Auto-install of Sodium / OptiFine — once per instance, errors are not critical.
    if inst.auto_perf && inst.loader != Loader::Vanilla {
        let marker = game_dir.join(".ferrum_perf");
        if !marker.exists() {
            progress.label("Installing performance mods (Sodium / OptiFine)…");
            match perf::install_for(http, inst) {
                Ok(m) => {
                    note(log, &m);
                    let _ = fs::write(&marker, "ok");
                }
                Err(e) => note(log, &format!("Performance mods auto-install failed: {e:#}")),
            }
        }
    }

    // The account's skin (via CustomSkinLoader), errors are not critical.
    match skin::prepare_instance(http, inst, acc) {
        Ok(Some(m)) => note(log, &m),
        Ok(None) => {}
        Err(e) => note(log, &format!("Skin not applied: {e:#}")),
    }

    let resolved = minecraft::resolve(&root, &id)?;
    let prep = minecraft::prepare(http, &root, &game_dir, &resolved, progress)?;

    let opts = minecraft::LaunchOpts {
        game_dir: game_dir.clone(),
        ram_mb: inst.effective_ram(st),
        extra_jvm: format!("{} {}", st.jvm_args, inst.jvm_args),
        width: st.win_w,
        height: st.win_h,
        fullscreen: st.fullscreen,
        name: acc.name.clone(),
        uuid: acc.uuid.clone(),
    };
    let args = minecraft::build_args(&resolved, &prep, &root, &opts);
    note(log, &format!("Memory: {} MB", opts.ram_mb));
    note(log, &format!("Command: {} {}", java_bin, args.join(" ")));
    note(log, "Launching the game. The game output follows.");

    progress.label("Launching the game…");
    let out = fs::OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    let mut cmd = Command::new(&java_bin);
    java::quiet(&mut cmd);
    let launch_args = launch_args(&args, major, &game_dir, log)?;
    cmd.args(&launch_args)
        .current_dir(&game_dir)
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    let started = SystemTime::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Could not run {java_bin}"))?;

    progress.running("Game running");
    let status = child.wait()?;
    let code = status.code().map_or("?".to_string(), |c| c.to_string());
    note(log, &format!("The game exited, exit code: {code}"));

    if status.success() {
        progress.finish("Game closed");
    } else {
        let dest = collect_crash(&game_dir, log, started, inst);
        note(log, &format!("Crash logs saved: {}", dest.display()));
        let hint = diagnose(&read_tail(log, 400_000));
        let mut msg = format!(
            "The game exited with an error (code {code}).\n\nCrash logs saved to the folder:\n{}",
            dest.display()
        );
        if let Some(h) = hint {
            note(log, &format!("Possible cause: {h}"));
            msg.push_str(&format!("\n\nPossible cause: {h}"));
        }
        progress.fail(msg);
    }
    Ok(())
}

/// Keeps only the `keep` newest entries in the folder. Names start with the date,
/// so the order by name matches the order by time.
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    let excess = items.len().saturating_sub(keep);
    for p in items.into_iter().take(excess) {
        if p.is_dir() {
            let _ = fs::remove_dir_all(&p);
        } else {
            let _ = fs::remove_file(&p);
        }
    }
}

/// On Windows the command line is limited to ~32 thousand characters. If there are too many arguments,
/// we pass them via an @argfile (supported in Java 9+).
fn launch_args(args: &[String], java_major: u32, game_dir: &Path, log: &Path) -> Result<Vec<String>> {
    let total: usize = args.iter().map(|a| a.len() + 3).sum();
    if cfg!(windows) && java_major >= 9 && total > 28_000 {
        let file = game_dir.join(".ferrum_args.txt");
        let mut text = String::new();
        for a in args {
            text.push('"');
            text.push_str(&a.replace('\\', "\\\\").replace('"', "\\\""));
            text.push_str("\"\n");
        }
        fs::write(&file, text)?;
        note(log, "The command line is long: arguments passed via an @argfile");
        return Ok(vec![format!("@{}", file.display())]);
    }
    Ok(args.to_vec())
}

fn read_tail(path: &Path, max: usize) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(max);
    String::from_utf8_lossy(&bytes[start..]).to_string()
}

/// Suggests a likely crash cause based on the log text.
pub fn diagnose(text: &str) -> Option<&'static str> {
    const RULES: [(&str, &str); 9] = [
        ("OutOfMemoryError", "The game ran out of memory. Increase the RAM in the instance settings or in the global settings."),
        ("Could not reserve enough space", "The system could not allocate that much memory. Reduce the RAM in the settings."),
        ("UnsupportedClassVersionError", "The Java version doesn't fit the game or the mods. Turn on \"Pick and download Java automatically\" in the settings."),
        ("UnsatisfiedLinkError", "A system library (natives) failed to load. Delete the natives folder in the launcher's data folder and launch again."),
        ("DuplicateModsFoundException", "The mods folder contains two copies of the same mod. Keep only one."),
        ("ResolutionException", "Java module conflict. Most often the mods folder has two copies of one mod or incompatible versions: keep one file per mod."),
        ("Missing or unsupported mandatory dependencies", "Dependency mods are missing or have the wrong version. The list is in latest.log."),
        ("Mod resolution failed", "The mods don't fit each other or the game version. See latest.log for details."),
        ("Mixin apply failed", "The mods conflict with each other (Mixin). Remove the most recently added mod."),
    ];
    RULES.iter().find(|(p, _)| text.contains(p)).map(|(_, h)| *h)
}

/// Gathers everything about a crash into one folder: launch log, latest.log, crash-reports, hs_err.
fn collect_crash(game_dir: &Path, launch_log: &Path, started: SystemTime, inst: &Instance) -> PathBuf {
    let dest = crash_logs_dir().join(format!("{}_{}", stamp(now()), inst.id));
    let _ = fs::create_dir_all(&dest);
    let _ = fs::copy(launch_log, dest.join("launch.log"));
    let _ = fs::copy(game_dir.join("logs").join("latest.log"), dest.join("latest.log"));

    let newer = |p: &Path| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .map_or(false, |t| t >= started)
    };
    let sources: [(PathBuf, &str); 2] = [
        (game_dir.join("crash-reports"), ""),
        (game_dir.to_path_buf(), "hs_err_pid"),
    ];
    for (d, prefix) in sources {
        if let Ok(rd) = fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_file() && name.starts_with(prefix) && newer(&p) {
                    let _ = fs::copy(&p, dest.join(&name));
                }
            }
        }
    }
    dest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_common_crashes() {
        assert!(diagnose("java.lang.OutOfMemoryError: Java heap space").unwrap().contains("memory"));
        assert!(diagnose("java.lang.module.ResolutionException: Modules a and b export package")
            .unwrap()
            .contains("module"));
        assert!(diagnose("UnsupportedClassVersionError: class file version 65").unwrap().contains("Java"));
        assert_eq!(diagnose("all good"), None);
    }

    #[test]
    fn old_logs_are_pruned() {
        let dir = std::env::temp_dir().join(format!("ferrum-test-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for n in 1..=5 {
            fs::write(dir.join(format!("a{n}.log")), b"x").unwrap();
        }
        prune(&dir, 2);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(left, vec!["a4.log", "a5.log"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_command_lines_are_passed_as_is() {
        let args = vec!["-Xmx1G".to_string(), "Main".to_string()];
        let out = launch_args(&args, 21, Path::new("."), Path::new("x.log")).unwrap();
        assert_eq!(out, args);
    }
}
