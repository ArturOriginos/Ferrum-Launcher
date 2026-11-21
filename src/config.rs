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

/// Версия формата config.json. Растёт, когда формат меняется несовместимо.
pub const CONFIG_VERSION: u32 = 2;

/// Неизвестные поля из конфига другой версии лаунчера: сохраняем их, чтобы не потерять при записи.
pub type Extra = serde_json::Map<String, serde_json::Value>;

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
/// Свободный id для новой сборки: не совпадает ни с другими сборками, ни с папкой на диске.
pub fn unique_instance_id(name: &str, existing: &[Instance]) -> String {
    let base = format!("{}-{}", crate::util::slug(name), now());
    let mut id = base.clone();
    let mut n = 2;
    while existing.iter().any(|i| i.id == id) || instances_dir().join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
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
    /// Скругление углов: 0 острые, 1 мягкие, 2 круглые. И свечение акцента сверху страницы.
    pub radius: u32,
    pub glow: bool,
    /// Свой фон интерфейса и затемнение поверх него, %.
    pub has_bg: bool,
    pub bg_dim: u32,
    /// Своя иконка лаунчера.
    pub has_icon: bool,
    /// Версия принятого пользовательского соглашения (пусто — не принято).
    pub accepted_terms: String,
    #[serde(flatten)]
    pub extra: Extra,
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
            radius: 1,
            glow: true,
            has_bg: false,
            bg_dim: 72,
            has_icon: false,
            accepted_terms: String::new(),
            extra: Extra::new(),
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
    /// У сборок, созданных старыми версиями, поля нет: для них выключено, ничего не ставится само.
    #[serde(default)]
    pub auto_perf: bool,
    /// Сколько секунд наиграно в этой сборке и сколько раз её запускали.
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub launches: u32,
    #[serde(flatten)]
    pub extra: Extra,
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
    #[serde(flatten)]
    pub extra: Extra,
}

impl Account {
    pub fn offline(name: &str, ttl_secs: Option<u64>) -> Self {
        Account {
            name: name.to_string(),
            uuid: offline_uuid(name),
            expires_at: ttl_secs.map(|t| now() + t),
            extra: Extra::new(),
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
    pub config_version: u32,
    pub settings: Settings,
    pub accounts: Vec<Account>,
    pub instances: Vec<Instance>,
    pub selected_account: Option<String>,
    pub selected_instance: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Config {
    /// Читает конфиг. Если формат не подошёл (файл от другой версии), делает копию
    /// `config.backup-<время>.json` и вытаскивает всё, что получится, ничего не стирая молча.
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Config::default();
        };
        let mut cfg = match serde_json::from_str::<Config>(&text) {
            Ok(cfg) => cfg,
            Err(_) => {
                let backup = data_dir().join(format!("config.backup-{}.json", stamp(now())));
                let _ = fs::write(&backup, &text);
                parse_lenient(&text).unwrap_or_default()
            }
        };
        cfg.migrate();
        cfg
    }

    /// Приводит конфиг старых версий к текущему формату.
    fn migrate(&mut self) {
        // Версия 0–1: поле auto_perf появилось позже, для старых сборок оно уже выключено (serde default).
        // Если файл записала более новая версия, номер не понижаем.
        if self.config_version < CONFIG_VERSION {
            self.config_version = CONFIG_VERSION;
        }
    }

    /// Запись через временный файл: при сбое посреди записи старый конфиг остаётся целым.
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let path = config_path();
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, s).is_ok() && fs::rename(&tmp, &path).is_err() {
                let _ = fs::remove_file(&tmp);
            }
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
    fn old_config_without_new_fields_loads() {
        // формат версии 0.1.0: нет auto_perf, accounts без expires_at, нет полей оформления
        let old = r#"{
            "settings": {"ram_mb": 6144, "auto_java": true},
            "accounts": [{"name": "Steve", "uuid": "u1"}],
            "instances": [{"id": "a", "name": "A", "mc_version": "1.20.1", "loader": "Forge"}],
            "selected_instance": "a"
        }"#;
        let cfg: Config = serde_json::from_str(old).unwrap();
        assert_eq!(cfg.settings.ram_mb, 6144);
        assert_eq!(cfg.accounts[0].expires_at, None);
        assert!(!cfg.instances[0].auto_perf, "старые сборки не должны получать автоустановку");
        assert_eq!(cfg.settings.palette, 0);
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let newer = r#"{"config_version": 9, "future_thing": {"a": 1},
            "settings": {"ram_mb": 2048, "future_setting": true}}"#;
        let cfg: Config = serde_json::from_str(newer).unwrap();
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("future_thing"));
        assert!(back.contains("future_setting"));
    }

    #[test]
    fn unknown_loader_does_not_lose_the_config() {
        let text = r#"{
            "accounts": [{"name": "Steve", "uuid": "u1"}],
            "instances": [
                {"id": "a", "name": "A", "mc_version": "1.21.1", "loader": "FutureLoader"},
                {"id": "b", "name": "B", "mc_version": "1.21.1", "loader": "Fabric"}
            ]
        }"#;
        assert!(serde_json::from_str::<Config>(text).is_err());
        let cfg = parse_lenient(text).unwrap();
        assert_eq!(cfg.instances.len(), 2);
        assert_eq!(cfg.instances[0].loader, Loader::Vanilla);
        assert_eq!(cfg.accounts.len(), 1);
    }

    #[test]
    fn broken_entry_is_skipped_not_fatal() {
        let text = r#"{"instances": [{"id": 5}, {"id": "ok", "name": "N", "mc_version": "1.21.1", "loader": "Forge"}]}"#;
        let cfg = parse_lenient(text).unwrap();
        assert_eq!(cfg.instances.len(), 1);
        assert_eq!(cfg.instances[0].id, "ok");
    }

    #[test]
    fn new_instance_ids_never_collide() {
        let first = unique_instance_id("Pack", &[]);
        let inst: Instance = serde_json::from_value(serde_json::json!({
            "id": first, "name": "Pack", "mc_version": "1.21.1", "loader": "Fabric"
        }))
        .unwrap();
        let second = unique_instance_id("Pack", &[inst]);
        assert_ne!(first, second);
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

/// Разбор «по частям»: битая сборка или аккаунт не рушат весь конфиг.
fn parse_lenient(text: &str) -> Option<Config> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    let mut cfg = Config::default();
    if let Some(s) = obj.get("settings") {
        if let Ok(x) = serde_json::from_value::<Settings>(s.clone()) {
            cfg.settings = x;
        }
    }
    if let Some(a) = obj.get("accounts").and_then(|a| a.as_array()) {
        cfg.accounts = a
            .iter()
            .filter_map(|x| serde_json::from_value::<Account>(x.clone()).ok())
            .collect();
    }
    if let Some(a) = obj.get("instances").and_then(|a| a.as_array()) {
        cfg.instances = a
            .iter()
            .filter_map(|x| {
                let mut x = x.clone();
                fix_unknown_loader(&mut x);
                serde_json::from_value::<Instance>(x).ok()
            })
            .collect();
    }
    cfg.selected_account = obj.get("selected_account").and_then(|x| x.as_str()).map(String::from);
    cfg.selected_instance = obj.get("selected_instance").and_then(|x| x.as_str()).map(String::from);
    cfg.config_version = obj.get("config_version").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    Some(cfg)
}

/// Загрузчик из будущей версии лаунчера, которого мы не знаем, считаем Vanilla.
fn fix_unknown_loader(v: &mut serde_json::Value) {
    let known = ["Vanilla", "Fabric", "Quilt", "Forge", "NeoForge"];
    let unknown = v
        .get("loader")
        .and_then(|l| l.as_str())
        .map_or(false, |l| !known.contains(&l));
    if unknown {
        v["loader"] = serde_json::json!("Vanilla");
    }
}
