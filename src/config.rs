use md5::{Digest, Md5};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Время в виде «2026-11-19_14-05-09» (UTC) для имён файлов и логов.
pub fn stamp(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}_{h:02}-{m:02}-{s:02}")
}

fn default_true() -> bool {
    true
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------- Пути ----------

pub fn data_dir() -> PathBuf {
    let d = directories::ProjectDirs::from("dev", "ferrum", "FerrumLauncher")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("ferrum-data"));
    let _ = fs::create_dir_all(&d);
    d
}
/// Общая папка Minecraft: versions / libraries / assets.
pub fn game_root() -> PathBuf {
    data_dir().join("minecraft")
}
pub fn instances_dir() -> PathBuf {
    data_dir().join("instances")
}
pub fn runtime_dir() -> PathBuf {
    data_dir().join("runtime")
}
pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}
/// Лог каждого запуска игры.
pub fn launch_logs_dir() -> PathBuf {
    logs_dir().join("launches")
}
/// Отдельная папка на каждый краш: лог запуска, latest.log, crash-reports.
pub fn crash_logs_dir() -> PathBuf {
    logs_dir().join("crashes")
}
fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

// ---------- Загрузчики и источники модов ----------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loader {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    pub const ALL: [Loader; 5] = [
        Loader::Vanilla,
        Loader::Fabric,
        Loader::Quilt,
        Loader::Forge,
        Loader::NeoForge,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Loader::Vanilla => "Vanilla",
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }
    /// Категории Modrinth (Quilt умеет грузить и моды Fabric).
    pub fn modrinth_names(self) -> Vec<&'static str> {
        match self {
            Loader::Vanilla => vec![],
            Loader::Fabric => vec!["fabric"],
            Loader::Quilt => vec!["quilt", "fabric"],
            Loader::Forge => vec!["forge"],
            Loader::NeoForge => vec!["neoforge"],
        }
    }
    /// modLoaderType в API CurseForge.
    pub fn curseforge(self) -> Option<u32> {
        match self {
            Loader::Vanilla => None,
            Loader::Forge => Some(1),
            Loader::Fabric | Loader::Quilt => Some(4),
            Loader::NeoForge => Some(6),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModSource {
    Modrinth,
    CurseForge,
}

impl ModSource {
    pub fn label(self) -> &'static str {
        match self {
            ModSource::Modrinth => "Modrinth",
            ModSource::CurseForge => "CurseForge",
        }
    }
}

// ---------- Настройки ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub ram_mb: u32,
    pub jvm_args: String,
    pub auto_java: bool,
    pub java_path: String,
    pub win_w: u32,
    pub win_h: u32,
    pub fullscreen: bool,
    pub hide_launcher: bool,
    pub curseforge_key: String,
    pub mod_source: ModSource,
    pub accent: usize,
    /// Свой акцентный цвет (RGB); работает, если включён `use_custom_accent`.
    pub use_custom_accent: bool,
    pub custom_accent: [u8; 3],
    /// Номер палитры фона.
    pub palette: usize,
    /// Масштаб интерфейса: 0.9 … 1.5
    pub ui_scale: f32,
    /// Свой фон интерфейса и затемнение поверх него, %.
    pub has_bg: bool,
    pub bg_dim: u32,
    /// Своя иконка лаунчера.
    pub has_icon: bool,
    /// Версия принятого пользовательского соглашения (пусто — не принято).
    pub accepted_terms: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ram_mb: 4096,
            jvm_args: "-XX:+UseG1GC -XX:MaxGCPauseMillis=100".into(),
            auto_java: true,
            java_path: String::new(),
            win_w: 1280,
            win_h: 720,
            fullscreen: false,
            hide_launcher: true,
            curseforge_key: String::new(),
            mod_source: ModSource::Modrinth,
            accent: 0,
            use_custom_accent: false,
            custom_accent: [224, 102, 44],
            palette: 0,
            ui_scale: 1.0,
            has_bg: false,
            bg_dim: 72,
            has_icon: false,
            accepted_terms: String::new(),
        }
    }
}

// ---------- Сборки ----------

#[derive(Serialize, Deserialize, Clone)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: Loader,
    /// Пусто = последняя подходящая версия загрузчика.
    #[serde(default)]
    pub loader_version: String,
    /// 0 = брать из общих настроек.
    #[serde(default)]
    pub ram_mb: u32,
    #[serde(default)]
    pub jvm_args: String,
    #[serde(default)]
    pub last_played: u64,
    /// Автоустановка Sodium (Fabric/Quilt/NeoForge) или OptiFine (Forge).
    #[serde(default = "default_true")]
    pub auto_perf: bool,
}

impl Instance {
    pub fn dir(&self) -> PathBuf {
        instances_dir().join(&self.id)
    }
    pub fn mods_dir(&self) -> PathBuf {
        self.dir().join("mods")
    }
    pub fn effective_ram(&self, s: &Settings) -> u32 {
        if self.ram_mb > 0 {
            self.ram_mb
        } else {
            s.ram_mb
        }
    }
}

// ---------- Аккаунты ----------

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub name: String,
    pub uuid: String,
    /// Some(время) — временный аккаунт, удалится после этого момента.
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl Account {
    pub fn offline(name: &str, ttl_secs: Option<u64>) -> Self {
        Account {
            name: name.to_string(),
            uuid: offline_uuid(name),
            expires_at: ttl_secs.map(|t| now() + t),
        }
    }
}

/// UUID офлайн-игрока — так же считает сам Minecraft (MD5 от "OfflinePlayer:ник").
pub fn offline_uuid(name: &str) -> String {
    let mut h = Md5::new();
    h.update(format!("OfflinePlayer:{name}").as_bytes());
    let mut b: [u8; 16] = h.finalize().into();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn random_nick() -> String {
    const ADJ: [&str; 10] = [
        "Swift", "Brave", "Cosmic", "Lucky", "Iron", "Pixel", "Shadow", "Frost", "Rusty", "Neon",
    ];
    const NOUN: [&str; 8] = ["Fox", "Wolf", "Crab", "Golem", "Miner", "Owl", "Raven", "Creeper"];
    let mut rng = rand::thread_rng();
    let a = ADJ.choose(&mut rng).copied().unwrap_or("Pixel");
    let n = NOUN.choose(&mut rng).copied().unwrap_or("Fox");
    let num: u32 = rng.gen_range(10..100);
    format!("{a}{n}{num}")
}

// ---------- Общий конфиг ----------

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub settings: Settings,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub selected_account: Option<String>,
    pub selected_instance: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(config_path(), s);
        }
    }

    /// Удаляет временные аккаунты, у которых вышел срок.
    pub fn purge_expired(&mut self) {
        let n = now();
        self.accounts.retain(|a| a.expires_at.map_or(true, |e| e > n));
        let sel_ok = self
            .selected_account
            .as_ref()
            .map_or(false, |s| self.accounts.iter().any(|a| &a.uuid == s));
        if !sel_ok {
            self.selected_account = self.accounts.first().map(|a| a.uuid.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_epoch() {
        assert_eq!(stamp(0), "1970-01-01_00-00-00");
    }

    #[test]
    fn stamp_leap_day() {
        assert_eq!(stamp(951_782_400), "2000-02-29_00-00-00");
    }

    #[test]
    fn stamp_time_of_day() {
        assert_eq!(stamp(3 * 3600 + 25 * 60 + 9), "1970-01-01_03-25-09");
    }

    #[test]
    fn offline_uuid_shape() {
        let u = offline_uuid("Steve");
        assert_eq!(u.len(), 36);
        assert_eq!(u.chars().nth(14), Some('3'));
        assert!("89ab".contains(u.chars().nth(19).unwrap()));
        assert_eq!(u, offline_uuid("Steve"));
        assert_ne!(u, offline_uuid("Alex"));
    }

    #[test]
    fn random_nick_is_valid_minecraft_name() {
        for _ in 0..50 {
            let n = random_nick();
            assert!((3..=16).contains(&n.len()), "{n}");
            assert!(n.chars().all(|c| c.is_ascii_alphanumeric()), "{n}");
        }
    }

    #[test]
    fn expired_temporary_accounts_are_purged() {
        let mut cfg = Config::default();
        let mut old = Account::offline("OldOne", Some(10));
        old.expires_at = Some(1);
        cfg.accounts.push(old);
        cfg.accounts.push(Account::offline("Keeper", None));
        cfg.purge_expired();
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts[0].name, "Keeper");
        assert_eq!(cfg.selected_account.as_deref(), Some(cfg.accounts[0].uuid.as_str()));
    }
}
