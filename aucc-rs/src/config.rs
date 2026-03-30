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
}

impl Default for LightbarConfig {
    fn default() -> Self {
        // Default: lightbar on, white, 50% brightness (0x32 = 50)
        Self { enabled: true, r: 0xff, g: 0xff, b: 0xff, brightness: 0x32 }
    }
}

/// Load lightbar config from `/etc/aucc/lightbar.conf`.
/// Returns `LightbarConfig::default()` if the file does not exist or cannot be parsed.
pub fn load() -> LightbarConfig {
    parse_file(CONFIG_PATH).unwrap_or_default()
}

fn parse_file(path: &str) -> Option<LightbarConfig> {
    let content = fs::read_to_string(path).ok()?;
    let mut cfg = LightbarConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            match key.trim() {
                "enabled"    => { if let Ok(v) = val.trim().parse() { cfg.enabled = v; } }
                "r"          => { if let Ok(v) = val.trim().parse() { cfg.r = v; } }
                "g"          => { if let Ok(v) = val.trim().parse() { cfg.g = v; } }
                "b"          => { if let Ok(v) = val.trim().parse() { cfg.b = v; } }
                "brightness" => { if let Ok(v) = val.trim().parse() { cfg.brightness = v; } }
                _ => {}
            }
        }
    }
    Some(cfg)
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
    Ok(())
}
