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

/// Дописывает строку с временем в лог запуска.
fn note(path: &Path, msg: &str) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", stamp(now()), msg);
    }
}

/// Полный цикл: подготовить файлы, запустить игру, дождаться выхода.
/// Каждый запуск пишет свой лог в `logs/launches/`, а при краше всё нужное
/// копируется в отдельную папку в `logs/crashes/`.
pub fn run(
    http: &Client,
    st: &Settings,
    inst: &Instance,
    acc: &Account,
    progress: &Progress,
) -> Result<()> {
    let dir = launch_logs_dir();
    fs::create_dir_all(&dir)?;
    let log_path = dir.join(format!("{}_{}.log", stamp(now()), inst.id));
    note(
        &log_path,
        &format!(
            "Ferrum Launcher v{VERSION}. Сборка «{}», Minecraft {}, {}",
            inst.name,
            inst.mc_version,
            inst.loader.label()
        ),
    );
    let r = run_inner(http, st, inst, acc, progress, &log_path);
    if let Err(e) = &r {
        note(&log_path, &format!("ОШИБКА: {e:#}"));
        return Err(anyhow!("{e:#}\n\nЛог запуска:\n{}", log_path.display()));
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

    progress.label("Проверяю Minecraft…");
    minecraft::ensure_vanilla_json(http, &root, &inst.mc_version)?;
    let vanilla = minecraft::resolve(&root, &inst.mc_version)?;
    let major = vanilla["javaVersion"]["majorVersion"].as_u64().unwrap_or(8) as u32;

    let java_bin = java::pick(http, st, major, progress)?;
    note(
        log,
        &format!(
            "Java: {} (версия {})",
            java_bin,
            java::java_major(&java_bin).map_or("?".to_string(), |m| m.to_string())
        ),
    );
    minecraft::ensure_client_jar(http, &root, &vanilla, progress)?;

    let id = loaders::resolve_id(http, &root, inst, &java_bin, progress)?;
    note(log, &format!("Версия профиля: {id}"));

    // Автоустановка Sodium / OptiFine — один раз на сборку, ошибки не критичны.
    if inst.auto_perf && inst.loader != Loader::Vanilla {
        let marker = game_dir.join(".ferrum_perf");
        if !marker.exists() {
            progress.label("Ставлю оптимизацию (Sodium / OptiFine)…");
            match perf::install_for(http, inst) {
                Ok(m) => {
                    note(log, &m);
                    let _ = fs::write(&marker, "ok");
                }
                Err(e) => note(log, &format!("Автоустановка оптимизации не удалась: {e:#}")),
            }
        }
    }

    // Скин аккаунта (через CustomSkinLoader), ошибки не критичны.
    match skin::prepare_instance(http, inst, acc) {
        Ok(Some(m)) => note(log, &m),
        Ok(None) => {}
        Err(e) => note(log, &format!("Скин не применён: {e:#}")),
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
    note(log, &format!("Память: {} МБ", opts.ram_mb));
    note(log, &format!("Команда: {} {}", java_bin, args.join(" ")));
    note(log, "Запускаю игру. Ниже — вывод игры.");

    progress.label("Запускаю игру…");
    let out = fs::OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    let mut cmd = Command::new(&java_bin);
    java::quiet(&mut cmd);
    cmd.args(&args)
        .current_dir(&game_dir)
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    let started = SystemTime::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Не удалось запустить {java_bin}"))?;

    progress.running("Игра запущена");
    let status = child.wait()?;
    let code = status.code().map_or("?".to_string(), |c| c.to_string());
    note(log, &format!("Игра завершилась, код выхода: {code}"));

    if status.success() {
        progress.finish("Игра закрыта");
    } else {
        let dest = collect_crash(&game_dir, log, started, inst);
        note(log, &format!("Логи краша сохранены: {}", dest.display()));
        progress.fail(format!(
            "Игра завершилась с ошибкой (код {code}).\n\nЛоги краша сохранены в папку:\n{}",
            dest.display()
        ));
    }
    Ok(())
}

/// Собирает всё про краш в одну папку: лог запуска, latest.log, crash-reports, hs_err.
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
