use super::parse_kv;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const KEYBOARD_CONFIG_PATH: &str = "/etc/aucc/keyboard.conf";

/// Which kind of lighting the user last applied to the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// No stored state, or the user deliberately turned the backlight off.
    #[default]
    Off,
    Mono,
    HAlt,
    VAlt,
    Effect,
}

impl KeyboardMode {
    fn as_str(self) -> &'static str {
        match self {
            KeyboardMode::Off    => "off",
            KeyboardMode::Mono   => "mono",
            KeyboardMode::HAlt   => "halt",
            KeyboardMode::VAlt   => "valt",
            KeyboardMode::Effect => "effect",
        }
    }

    /// Unknown values fall back to Off so a corrupt file never lights the
    /// keyboard with something the user did not choose.
    fn from_str(s: &str) -> Self {
        match s {
            "mono"   => KeyboardMode::Mono,
            "halt"   => KeyboardMode::HAlt,
            "valt"   => KeyboardMode::VAlt,
            "effect" => KeyboardMode::Effect,
            _        => KeyboardMode::Off,
        }
    }
}

/// Last keyboard lighting the user applied. Reapplied by `aucc --kb-restore`
/// after the EC clears the backlight on power events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardConfig {
    pub mode: KeyboardMode,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Secondary color, used by the HAlt/VAlt modes only.
    pub r2: u8,
    pub g2: u8,
    pub b2: u8,
    /// 1–4 (hardware levels).
    pub brightness: u8,
    /// Effect name as accepted by `Effect::from_str`.
    pub effect: String,
    /// 1–10 (1 = fastest).
    pub speed: u8,
    /// right | left | up | down
    pub direction: String,
    /// Effect color-variant suffix ('r', 'o', 'y', 'g', 'b', 't', 'p').
    pub letter: Option<char>,
    pub reactive: bool,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            mode: KeyboardMode::Off,
            r: 0xff, g: 0xff, b: 0xff,
            r2: 0xff, g2: 0xff, b2: 0xff,
            brightness: 4,
            effect: "rainbow".to_string(),
            speed: 5,
            direction: "right".to_string(),
            letter: None,
            reactive: false,
        }
    }
}

fn apply_pair(cfg: &mut KeyboardConfig, key: &str, val: &str) {
    match key {
        "mode"       => cfg.mode = KeyboardMode::from_str(val),
        "r"          => { if let Ok(v) = val.parse() { cfg.r = v; } }
        "g"          => { if let Ok(v) = val.parse() { cfg.g = v; } }
        "b"          => { if let Ok(v) = val.parse() { cfg.b = v; } }
        "r2"         => { if let Ok(v) = val.parse() { cfg.r2 = v; } }
        "g2"         => { if let Ok(v) = val.parse() { cfg.g2 = v; } }
        "b2"         => { if let Ok(v) = val.parse() { cfg.b2 = v; } }
        // Out-of-range values are dropped rather than clamped: a bad file
        // should not silently change what the user configured.
        "brightness" => { if let Ok(v) = val.parse::<u8>() { if (1..=4).contains(&v)  { cfg.brightness = v; } } }
        "speed"      => { if let Ok(v) = val.parse::<u8>() { if (1..=10).contains(&v) { cfg.speed = v; } } }
        "effect"     => cfg.effect = val.to_string(),
        "direction"  => cfg.direction = val.to_string(),
        "letter"     => cfg.letter = val.chars().next(),
        "reactive"   => { if let Ok(v) = val.parse() { cfg.reactive = v; } }
        _ => {}
    }
}

/// Parse a keyboard config from a specific path. None if unreadable.
pub fn parse_keyboard_file(path: &str) -> Option<KeyboardConfig> {
    let pairs = parse_kv(path)?;
    let mut cfg = KeyboardConfig::default();
    for (k, v) in pairs {
        apply_pair(&mut cfg, &k, &v);
    }
    Some(cfg)
}

/// Load the keyboard config, falling back to "off" when absent or unreadable.
pub fn load_keyboard() -> KeyboardConfig {
    parse_keyboard_file(KEYBOARD_CONFIG_PATH).unwrap_or_default()
}

fn write_to(cfg: &KeyboardConfig, path: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    // O_NOFOLLOW: /etc/aucc is group-writable by plugdev, so a member could
    // replace this file with a symlink to an arbitrary root-owned path and have
    // the next root-side write truncate it.
    // mode 0664 so the file is group-writable and a config created by one side
    // (root keyboard commands / non-root lightbar commands) stays writable by the
    // other within plugdev. Caveat: open(2) masks this with the caller's umask,
    // so a root caller running with the usual 0022 still lands on 0644 — install
    // restores g+w on /etc/aucc, and the reverse direction (root writing a
    // user-created 0664 file) is the one that was actually breaking.
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o664)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    writeln!(f, "mode={}", cfg.mode.as_str())?;
    writeln!(f, "r={}", cfg.r)?;
    writeln!(f, "g={}", cfg.g)?;
    writeln!(f, "b={}", cfg.b)?;
    writeln!(f, "r2={}", cfg.r2)?;
    writeln!(f, "g2={}", cfg.g2)?;
    writeln!(f, "b2={}", cfg.b2)?;
    writeln!(f, "brightness={}", cfg.brightness)?;
    writeln!(f, "effect={}", cfg.effect)?;
    writeln!(f, "speed={}", cfg.speed)?;
    writeln!(f, "direction={}", cfg.direction)?;
    if let Some(l) = cfg.letter {
        writeln!(f, "letter={l}")?;
    }
    writeln!(f, "reactive={}", cfg.reactive)?;
    Ok(())
}

/// Persist to `/etc/aucc/keyboard.conf`.
pub fn save_keyboard(cfg: &KeyboardConfig) -> std::io::Result<()> {
    write_to(cfg, KEYBOARD_CONFIG_PATH)
}

/// Persist to a specific path (used by tests).
pub fn save_keyboard_to(cfg: &KeyboardConfig, path: &str) -> std::io::Result<()> {
    write_to(cfg, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> String {
        format!("/tmp/aucc_kb_test_{name}.conf")
    }

    fn cleanup(name: &str) {
        let _ = fs::remove_file(temp_path(name));
    }

    #[test]
    fn default_is_off() {
        assert_eq!(KeyboardConfig::default().mode, KeyboardMode::Off);
    }

    #[test]
    fn round_trip_mono() {
        cleanup("mono");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Mono,
            r: 255, g: 0, b: 128,
            brightness: 3,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("mono")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("mono")).unwrap(), cfg);
        cleanup("mono");
    }

    #[test]
    fn round_trip_halt() {
        cleanup("halt");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::HAlt,
            r: 255, g: 0, b: 0,
            r2: 0, g2: 0, b2: 255,
            brightness: 4,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("halt")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("halt")).unwrap(), cfg);
        cleanup("halt");
    }

    #[test]
    fn round_trip_valt() {
        cleanup("valt");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::VAlt,
            r: 0, g: 255, b: 0,
            r2: 255, g2: 255, b2: 0,
            brightness: 2,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("valt")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("valt")).unwrap(), cfg);
        cleanup("valt");
    }

    #[test]
    fn round_trip_effect_with_all_fields() {
        cleanup("effect");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 7,
            brightness: 4,
            direction: "left".to_string(),
            letter: Some('g'),
            reactive: true,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("effect")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("effect")).unwrap(), cfg);
        cleanup("effect");
    }

    #[test]
    fn round_trip_off() {
        cleanup("off");
        let cfg = KeyboardConfig { mode: KeyboardMode::Off, ..Default::default() };
        save_keyboard_to(&cfg, &temp_path("off")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("off")).unwrap().mode, KeyboardMode::Off);
        cleanup("off");
    }

    #[test]
    fn letter_none_round_trips_as_absent() {
        cleanup("noletter");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "rainbow".to_string(),
            letter: None,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("noletter")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("noletter")).unwrap().letter, None);
        cleanup("noletter");
    }

    #[test]
    fn missing_file_returns_none() {
        cleanup("missing");
        assert_eq!(parse_keyboard_file(&temp_path("missing")), None);
    }

    /// A missing file and a file that says `mode=off` must be distinguishable:
    /// the restore path leaves the keyboard untouched in the first case and
    /// deliberately turns it off in the second.
    #[test]
    fn absent_file_is_distinguishable_from_saved_off() {
        cleanup("absent_vs_off");
        assert_eq!(parse_keyboard_file(&temp_path("absent_vs_off")), None);

        let cfg = KeyboardConfig { mode: KeyboardMode::Off, ..Default::default() };
        save_keyboard_to(&cfg, &temp_path("absent_vs_off")).unwrap();
        let loaded = parse_keyboard_file(&temp_path("absent_vs_off"));
        assert_eq!(loaded.map(|c| c.mode), Some(KeyboardMode::Off));
        cleanup("absent_vs_off");
    }

    #[test]
    fn empty_file_yields_default() {
        cleanup("empty");
        fs::write(temp_path("empty"), "").unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("empty")).unwrap(), KeyboardConfig::default());
        cleanup("empty");
    }

    #[test]
    fn comments_and_blanks_are_skipped() {
        cleanup("comments");
        fs::write(temp_path("comments"), "# comentario\n\nmode=mono\nr=10\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("comments")).unwrap();
        assert_eq!(loaded.mode, KeyboardMode::Mono);
        assert_eq!(loaded.r, 10);
        cleanup("comments");
    }

    #[test]
    fn invalid_lines_are_ignored() {
        cleanup("invalid");
        fs::write(temp_path("invalid"), "mode=mono\nlixo sem igual\nr=200\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("invalid")).unwrap();
        assert_eq!(loaded.r, 200);
        assert_eq!(loaded.mode, KeyboardMode::Mono);
        cleanup("invalid");
    }

    #[test]
    fn unknown_mode_falls_back_to_off() {
        cleanup("badmode");
        fs::write(temp_path("badmode"), "mode=banana\n").unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("badmode")).unwrap().mode, KeyboardMode::Off);
        cleanup("badmode");
    }

    #[test]
    fn out_of_range_values_are_ignored() {
        cleanup("range");
        fs::write(temp_path("range"), "mode=mono\nbrightness=99\nspeed=0\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("range")).unwrap();
        assert_eq!(loaded.brightness, KeyboardConfig::default().brightness);
        assert_eq!(loaded.speed, KeyboardConfig::default().speed);
        cleanup("range");
    }
}
