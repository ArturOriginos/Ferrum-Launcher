use std::path::Path;
use std::process::Command;

/// Открывает папку в проводнике системы.
pub fn open_path(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(p).spawn();
}

/// Всего оперативной памяти в системе, МБ.
pub fn total_ram_mb() -> u32 {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    let mb = (s.total_memory() / 1024 / 1024) as u32;
    if mb == 0 {
        8192
    } else {
        mb
    }
}

/// Безопасное имя папки из названия сборки.
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "instance".to_string()
    } else {
        s
    }
}

/// 1234567 -> "1.2M"
pub fn fmt_num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_makes_safe_names() {
        assert_eq!(slug("My Pack!!"), "my-pack");
        assert_eq!(slug("Сборка"), "instance");
    }

    #[test]
    fn numbers_are_short() {
        assert_eq!(fmt_num(999), "999");
        assert_eq!(fmt_num(1_500), "1.5K");
        assert_eq!(fmt_num(2_500_000), "2.5M");
    }
}
