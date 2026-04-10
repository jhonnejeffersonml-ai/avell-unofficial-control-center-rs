use std::fs;
use std::io::Write;
use std::path::Path;

pub const CONFIG_PATH: &str = "/etc/aucc/lightbar.conf";

#[derive(Debug, Clone)]
pub struct LightbarConfig {
    pub enabled: bool,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub brightness: u8,
    /// Persist keyboard settings to EEPROM (survives reboot).
    pub save_eeprom: bool,
}

impl Default for LightbarConfig {
    fn default() -> Self {
        // Default: lightbar on, white, 50% brightness (0x32 = 50), save = false
        Self { enabled: true, r: 0xff, g: 0xff, b: 0xff, brightness: 0x32, save_eeprom: false }
    }
}

impl PartialEq for LightbarConfig {
    fn eq(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.r == other.r
            && self.g == other.g
            && self.b == other.b
            && self.brightness == other.brightness
            && self.save_eeprom == other.save_eeprom
    }
}

/// Load lightbar config from `/etc/aucc/lightbar.conf`.
/// Returns `LightbarConfig::default()` if the file does not exist or cannot be parsed.
pub fn load() -> LightbarConfig {
    parse_file(CONFIG_PATH).unwrap_or_default()
}

/// Load config and return Result (used by TUI to check if file exists).
pub fn load_file() -> std::io::Result<LightbarConfig> {
    let content = fs::read_to_string(CONFIG_PATH)?;
    let mut cfg = LightbarConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            match key.trim() {
                "enabled"      => { if let Ok(v) = val.trim().parse() { cfg.enabled = v; } }
                "r"            => { if let Ok(v) = val.trim().parse() { cfg.r = v; } }
                "g"            => { if let Ok(v) = val.trim().parse() { cfg.g = v; } }
                "b"            => { if let Ok(v) = val.trim().parse() { cfg.b = v; } }
                "brightness"   => { if let Ok(v) = val.trim().parse() { cfg.brightness = v; } }
                "save_eeprom"  => { if let Ok(v) = val.trim().parse() { cfg.save_eeprom = v; } }
                _ => {}
            }
        }
    }
    Ok(cfg)
}

/// Internal: parse a specific file (used by tests with temp paths).
fn parse_file_impl(path: &str) -> Option<LightbarConfig> {
    let content = fs::read_to_string(path).ok()?;
    let mut cfg = LightbarConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            match key.trim() {
                "enabled"      => { if let Ok(v) = val.trim().parse() { cfg.enabled = v; } }
                "r"            => { if let Ok(v) = val.trim().parse() { cfg.r = v; } }
                "g"            => { if let Ok(v) = val.trim().parse() { cfg.g = v; } }
                "b"            => { if let Ok(v) = val.trim().parse() { cfg.b = v; } }
                "brightness"   => { if let Ok(v) = val.trim().parse() { cfg.brightness = v; } }
                "save_eeprom"  => { if let Ok(v) = val.trim().parse() { cfg.save_eeprom = v; } }
                _ => {}
            }
        }
    }
    Some(cfg)
}

fn parse_file(path: &str) -> Option<LightbarConfig> {
    parse_file_impl(path)
}

/// Persist lightbar state to `/etc/aucc/lightbar.conf`.
/// Creates `/etc/aucc/` if it does not exist (requires root).
pub fn save(cfg: &LightbarConfig) -> std::io::Result<()> {
    if let Some(parent) = Path::new(CONFIG_PATH).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(CONFIG_PATH)?;
    writeln!(f, "enabled={}", cfg.enabled)?;
    writeln!(f, "r={}", cfg.r)?;
    writeln!(f, "g={}", cfg.g)?;
    writeln!(f, "b={}", cfg.b)?;
    writeln!(f, "brightness={}", cfg.brightness)?;
    writeln!(f, "save_eeprom={}", cfg.save_eeprom)?;
    Ok(())
}

/// Internal: save to a specific path (used by tests with temp files).
pub fn save_to(cfg: &LightbarConfig, path: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).ok();
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    writeln!(f, "enabled={}", cfg.enabled)?;
    writeln!(f, "r={}", cfg.r)?;
    writeln!(f, "g={}", cfg.g)?;
    writeln!(f, "b={}", cfg.b)?;
    writeln!(f, "brightness={}", cfg.brightness)?;
    writeln!(f, "save_eeprom={}", cfg.save_eeprom)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> String {
        format!("/tmp/aucc_test_{name}.conf")
    }

    fn cleanup(name: &str) {
        let _ = fs::remove_file(temp_path(name));
    }

    // ── save → load round-trip ──────────────────────────────────────

    #[test]
    fn round_trip_default() {
        cleanup("round_trip_default");
        let cfg = LightbarConfig::default();
        save_to(&cfg, &temp_path("round_trip_default")).unwrap();
        let loaded = parse_file(&temp_path("round_trip_default")).unwrap();
        assert_eq!(loaded, cfg);
        cleanup("round_trip_default");
    }

    #[test]
    fn round_trip_disabled() {
        cleanup("round_trip_disabled");
        let cfg = LightbarConfig { enabled: false, ..Default::default() };
        save_to(&cfg, &temp_path("round_trip_disabled")).unwrap();
        let loaded = parse_file(&temp_path("round_trip_disabled")).unwrap();
        assert_eq!(loaded, cfg);
        cleanup("round_trip_disabled");
    }

    #[test]
    fn round_trip_custom_color() {
        cleanup("round_trip_custom_color");
        let cfg = LightbarConfig { enabled: true, r: 0xFF, g: 0x00, b: 0x80, brightness: 75, save_eeprom: false };
        save_to(&cfg, &temp_path("round_trip_custom_color")).unwrap();
        let loaded = parse_file(&temp_path("round_trip_custom_color")).unwrap();
        assert_eq!(loaded, cfg);
        cleanup("round_trip_custom_color");
    }

    #[test]
    fn round_trip_save_eeprom() {
        cleanup("round_trip_save_eeprom");
        let cfg = LightbarConfig { enabled: true, save_eeprom: true, ..Default::default() };
        save_to(&cfg, &temp_path("round_trip_save_eeprom")).unwrap();
        let loaded = parse_file(&temp_path("round_trip_save_eeprom")).unwrap();
        assert!(loaded.save_eeprom);
        cleanup("round_trip_save_eeprom");
    }

    // ── load edge cases ─────────────────────────────────────────────

    #[test]
    fn load_nonexistent_file() {
        cleanup("nonexistent");
        let result = parse_file(&temp_path("nonexistent"));
        assert_eq!(result, None);
    }

    #[test]
    fn load_empty_file() {
        cleanup("empty");
        fs::write(temp_path("empty"), "").unwrap();
        let loaded = parse_file(&temp_path("empty")).unwrap();
        assert_eq!(loaded, LightbarConfig::default());
        cleanup("empty");
    }

    #[test]
    fn load_comments_only() {
        cleanup("comments");
        fs::write(temp_path("comments"), "# comment line\n# another comment\n").unwrap();
        let loaded = parse_file(&temp_path("comments")).unwrap();
        assert_eq!(loaded, LightbarConfig::default());
        cleanup("comments");
    }

    #[test]
    fn load_partial_config() {
        cleanup("partial");
        fs::write(temp_path("partial"), "enabled=false\nr=255\n").unwrap();
        let loaded = parse_file(&temp_path("partial")).unwrap();
        assert!(!loaded.enabled);
        assert_eq!(loaded.r, 255);
        // Unspecified fields get defaults
        assert_eq!(loaded.g, 0xFF);
        assert_eq!(loaded.b, 0xFF);
        assert_eq!(loaded.brightness, 0x32);
        cleanup("partial");
    }

    #[test]
    fn load_with_comments_and_blanks() {
        cleanup("mixed");
        let content = "# Lightbar config\n\nenabled=true\nr=0\ng=0\nb=255\n\n# full brightness\nbrightness=100\n";
        fs::write(temp_path("mixed"), content).unwrap();
        let loaded = parse_file(&temp_path("mixed")).unwrap();
        assert_eq!(loaded, LightbarConfig {
            enabled: true, r: 0, g: 0, b: 255, brightness: 100, save_eeprom: false
        });
        cleanup("mixed");
    }

    #[test]
    fn load_with_invalid_lines() {
        cleanup("invalid");
        let content = "enabled=true\ninvalid_line\nr=128\n";
        fs::write(temp_path("invalid"), content).unwrap();
        let loaded = parse_file(&temp_path("invalid")).unwrap();
        assert_eq!(loaded.r, 128);
        assert!(loaded.enabled);
        cleanup("invalid");
    }
}
