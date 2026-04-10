use std::fmt;

/// All supported lighting effects.
///
/// # Hardware note on "reactive" mode
///
/// The ITE 8291 controller does **not** have separate effect codes for
/// "reactive" variants. Effects like `Random` and `Reactive` share the same
/// base code (`0x04`), as do `Aurora` and `ReactiveAurora` (`0x0E`).
/// The difference is encoded in **byte 6** (the `modifier` field):
/// `0x00` = normal, `0x01` = reactive.
///
/// This enum exposes only the base effects; the `reactive` flag on
/// `effect_payload()` controls the modifier byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Breathing,
    Wave,
    Random,       // base for reactive variant (code 0x04)
    Rainbow,
    Ripple,
    ReactiveRipple, // distinct code (0x07)
    Marquee,
    Fireworks,
    Raindrop,
    Aurora,       // base for reactive variant (code 0x0E)
    UserMode,     // per-key / solid color (0x33)
}

impl Effect {
    pub fn code(self) -> u8 {
        match self {
            Effect::Breathing       => 0x02,
            Effect::Wave            => 0x03,
            Effect::Random          => 0x04,  // same as legacy "Reactive"
            Effect::Rainbow         => 0x05,
            Effect::Ripple          => 0x06,
            Effect::ReactiveRipple  => 0x07,  // distinct from Ripple
            Effect::Marquee         => 0x09,
            Effect::Raindrop        => 0x0A,
            Effect::Aurora          => 0x0E,  // same as legacy "ReactiveAurora"
            Effect::Fireworks       => 0x11,
            Effect::UserMode        => 0x33,
        }
    }

    /// Parse an effect name from CLI input.
    ///
    /// For backwards compatibility, accepts legacy aliases:
    /// - `"reactive"` → `Effect::Random` (with reactive flag — caller must handle)
    /// - `"reactiveaurora"` → `Effect::Aurora` (with reactive flag — caller must handle)
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "breathing"       => Some(Effect::Breathing),
            "wave"            => Some(Effect::Wave),
            "random" | "reactive"  => Some(Effect::Random),
            "rainbow"         => Some(Effect::Rainbow),
            "ripple"          => Some(Effect::Ripple),
            "reactiveripple"  => Some(Effect::ReactiveRipple),
            "marquee"         => Some(Effect::Marquee),
            "fireworks"       => Some(Effect::Fireworks),
            "raindrop"        => Some(Effect::Raindrop),
            "aurora" | "reactiveaurora" => Some(Effect::Aurora),
            _ => None,
        }
    }

    /// Returns true if this effect name was a "reactive" alias in the legacy API.
    /// The caller should set `reactive: true` on `effect_payload()` when this is true.
    pub fn is_reactive_alias(name: &str) -> bool {
        matches!(name, "reactive" | "reactiveaurora")
    }

    /// Whether this effect supports a single-color variant.
    pub fn supports_color_variant(self) -> bool {
        matches!(
            self,
            Effect::Breathing
                | Effect::Random
                | Effect::Ripple
                | Effect::ReactiveRipple
                | Effect::Aurora
                | Effect::Fireworks
                | Effect::Raindrop
        )
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Effect::Breathing       => "breathing",
            Effect::Wave            => "wave",
            Effect::Random          => "random",
            Effect::Rainbow         => "rainbow",
            Effect::Ripple          => "ripple",
            Effect::ReactiveRipple  => "reactiveripple",
            Effect::Marquee         => "marquee",
            Effect::Fireworks       => "fireworks",
            Effect::Raindrop        => "raindrop",
            Effect::Aurora          => "aurora",
            Effect::UserMode        => "user",
        };
        write!(f, "{}", s)
    }
}

/// Wave direction — encoded in byte 6 of the effect payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveDirection {
    #[default]
    Right = 0x01,
    Left  = 0x02,
    Up    = 0x03,
    Down  = 0x04,
}

impl WaveDirection {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "right" => Some(WaveDirection::Right),
            "left"  => Some(WaveDirection::Left),
            "up"    => Some(WaveDirection::Up),
            "down"  => Some(WaveDirection::Down),
            _ => None,
        }
    }
}

/// Color variant suffix letter → palette index.
pub fn color_variant_code(letter: Option<char>) -> u8 {
    match letter {
        Some('r') => 0x01, // red
        Some('o') => 0x02, // orange
        Some('y') => 0x03, // yellow
        Some('g') => 0x04, // green
        Some('b') => 0x05, // blue
        Some('t') => 0x06, // teal
        Some('p') => 0x07, // purple
        _         => 0x08, // rainbow (default)
    }
}

/// Brightness level (1–4) to hardware byte.
pub fn brightness_byte(level: u8) -> u8 {
    match level.clamp(1, 4) {
        1 => 0x08,
        2 => 0x16,
        3 => 0x24,
        _ => 0x32,
    }
}

/// Build the 8-byte control transfer payload for an effect.
///
/// # Arguments
/// * `effect` – lighting effect
/// * `speed` – 1 (fastest) to 10 (slowest)
/// * `brightness` – 1–4
/// * `color_variant` – single-letter suffix ('r', 'o', …) or None
/// * `direction` – wave direction (only used for Wave effect)
/// * `reactive` – enable reactive mode (byte 6 modifier = 0x01)
/// * `save` – persist to EEPROM
///
/// # Payload layout (8 bytes)
/// ```text
/// byte 0: 0x08  (command flag)
/// byte 1: 0x02  (enable)
/// byte 2: effect code (0x02–0x11, or 0x33 for user mode)
/// byte 3: speed (0x01–0x0A)
/// byte 4: brightness (0x08 / 0x16 / 0x24 / 0x32)
/// byte 5: color index (0x00–0x08)
/// byte 6: direction/modifier (wave: 0x01–0x04; reactive: 0x01)
/// byte 7: save to EEPROM (0x00 = no, 0x01 = yes)
/// ```
pub fn effect_payload(
    effect: Effect,
    speed: u8,
    brightness: u8,
    color_variant: Option<char>,
    direction: WaveDirection,
    reactive: bool,
    save: bool,
) -> [u8; 8] {
    let eff_code = effect.code();
    let spd = speed.clamp(1, 10);
    let brt = brightness_byte(brightness);
    let save_byte = if save { 0x01 } else { 0x00 };

    let modifier = if reactive { 0x01 } else { 0x00 };

    let (colour, final_modifier) = match effect {
        Effect::Rainbow => (0x00, 0x00),
        Effect::Marquee => (0x08, 0x00),
        Effect::Wave    => (0x00, direction as u8),
        Effect::Fireworks => (color_variant_code(color_variant), modifier),
        _ => (color_variant_code(color_variant), modifier),
    };

    [0x08, 0x02, eff_code, spd, brt, colour, final_modifier, save_byte]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Effect::code() ───────────────────────────────────────────────

    #[test]
    fn effect_codes() {
        assert_eq!(Effect::Breathing.code(), 0x02);
        assert_eq!(Effect::Wave.code(), 0x03);
        assert_eq!(Effect::Random.code(), 0x04);
        assert_eq!(Effect::Rainbow.code(), 0x05);
        assert_eq!(Effect::Ripple.code(), 0x06);
        assert_eq!(Effect::ReactiveRipple.code(), 0x07);
        assert_eq!(Effect::Marquee.code(), 0x09);
        assert_eq!(Effect::Raindrop.code(), 0x0A);
        assert_eq!(Effect::Aurora.code(), 0x0E);
        assert_eq!(Effect::Fireworks.code(), 0x11);
        assert_eq!(Effect::UserMode.code(), 0x33);
    }

    // ── Effect::from_str() ──────────────────────────────────────────

    #[test]
    fn from_str_valid() {
        assert_eq!(Effect::from_str("breathing"), Some(Effect::Breathing));
        assert_eq!(Effect::from_str("wave"), Some(Effect::Wave));
        assert_eq!(Effect::from_str("random"), Some(Effect::Random));
        assert_eq!(Effect::from_str("rainbow"), Some(Effect::Rainbow));
        assert_eq!(Effect::from_str("ripple"), Some(Effect::Ripple));
        assert_eq!(Effect::from_str("reactiveripple"), Some(Effect::ReactiveRipple));
        assert_eq!(Effect::from_str("marquee"), Some(Effect::Marquee));
        assert_eq!(Effect::from_str("fireworks"), Some(Effect::Fireworks));
        assert_eq!(Effect::from_str("raindrop"), Some(Effect::Raindrop));
        assert_eq!(Effect::from_str("aurora"), Some(Effect::Aurora));
    }

    #[test]
    fn from_str_reactive_aliases() {
        // "reactive" maps to Effect::Random (same hardware code 0x04)
        assert_eq!(Effect::from_str("reactive"), Some(Effect::Random));
        // "reactiveaurora" maps to Effect::Aurora (same hardware code 0x0E)
        assert_eq!(Effect::from_str("reactiveaurora"), Some(Effect::Aurora));
    }

    #[test]
    fn from_str_invalid() {
        assert_eq!(Effect::from_str("unknown"), None);
        assert_eq!(Effect::from_str(""), None);
        assert_eq!(Effect::from_str("breathingr"), None);
    }

    #[test]
    fn is_reactive_alias() {
        assert!(Effect::is_reactive_alias("reactive"));
        assert!(Effect::is_reactive_alias("reactiveaurora"));
        assert!(!Effect::is_reactive_alias("random"));
        assert!(!Effect::is_reactive_alias("aurora"));
        assert!(!Effect::is_reactive_alias("rainbow"));
    }

    #[test]
    fn effect_display() {
        assert_eq!(Effect::Breathing.to_string(), "breathing");
        assert_eq!(Effect::Random.to_string(), "random");
        assert_eq!(Effect::Aurora.to_string(), "aurora");
        assert_eq!(Effect::ReactiveRipple.to_string(), "reactiveripple");
        assert_eq!(Effect::UserMode.to_string(), "user");
    }

    // ── Effect::supports_color_variant() ────────────────────────────

    #[test]
    fn supports_color_variant_true() {
        assert!(Effect::Breathing.supports_color_variant());
        assert!(Effect::Random.supports_color_variant());
        assert!(Effect::Ripple.supports_color_variant());
        assert!(Effect::ReactiveRipple.supports_color_variant());
        assert!(Effect::Aurora.supports_color_variant());
        assert!(Effect::Fireworks.supports_color_variant());
        assert!(Effect::Raindrop.supports_color_variant());
    }

    #[test]
    fn supports_color_variant_false() {
        assert!(!Effect::Wave.supports_color_variant());
        assert!(!Effect::Rainbow.supports_color_variant());
        assert!(!Effect::Marquee.supports_color_variant());
        assert!(!Effect::UserMode.supports_color_variant());
    }

    // ── WaveDirection ───────────────────────────────────────────────

    #[test]
    fn wave_direction_codes() {
        assert_eq!(WaveDirection::Right as u8, 0x01);
        assert_eq!(WaveDirection::Left as u8, 0x02);
        assert_eq!(WaveDirection::Up as u8, 0x03);
        assert_eq!(WaveDirection::Down as u8, 0x04);
    }

    #[test]
    fn wave_direction_from_str() {
        assert_eq!(WaveDirection::from_str("right"), Some(WaveDirection::Right));
        assert_eq!(WaveDirection::from_str("left"), Some(WaveDirection::Left));
        assert_eq!(WaveDirection::from_str("up"), Some(WaveDirection::Up));
        assert_eq!(WaveDirection::from_str("down"), Some(WaveDirection::Down));
        assert_eq!(WaveDirection::from_str("invalid"), None);
    }

    #[test]
    fn wave_direction_default() {
        assert_eq!(WaveDirection::default(), WaveDirection::Right);
    }

    // ── color_variant_code() ────────────────────────────────────────

    #[test]
    fn color_variant_codes() {
        assert_eq!(color_variant_code(Some('r')), 0x01);
        assert_eq!(color_variant_code(Some('o')), 0x02);
        assert_eq!(color_variant_code(Some('y')), 0x03);
        assert_eq!(color_variant_code(Some('g')), 0x04);
        assert_eq!(color_variant_code(Some('b')), 0x05);
        assert_eq!(color_variant_code(Some('t')), 0x06);
        assert_eq!(color_variant_code(Some('p')), 0x07);
        assert_eq!(color_variant_code(None), 0x08);
        assert_eq!(color_variant_code(Some('x')), 0x08); // unknown → rainbow
    }

    // ── brightness_byte() ──────────────────────────────────────────

    #[test]
    fn brightness_levels() {
        assert_eq!(brightness_byte(1), 0x08);
        assert_eq!(brightness_byte(2), 0x16);
        assert_eq!(brightness_byte(3), 0x24);
        assert_eq!(brightness_byte(4), 0x32);
    }

    #[test]
    fn brightness_clamp_below_range() {
        assert_eq!(brightness_byte(0), 0x08); // clamped to 1
    }

    #[test]
    fn brightness_clamp_above_range() {
        assert_eq!(brightness_byte(10), 0x32); // clamped to 4
    }

    // ── effect_payload() ───────────────────────────────────────────

    #[test]
    fn payload_rainbow() {
        let p = effect_payload(Effect::Rainbow, 5, 3, None, WaveDirection::Right, false, false);
        assert_eq!(p, [0x08, 0x02, 0x05, 0x05, 0x24, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn payload_wave_right() {
        let p = effect_payload(Effect::Wave, 3, 4, None, WaveDirection::Right, false, true);
        assert_eq!(p, [0x08, 0x02, 0x03, 0x03, 0x32, 0x00, 0x01, 0x01]);
    }

    #[test]
    fn payload_wave_left() {
        let p = effect_payload(Effect::Wave, 3, 4, None, WaveDirection::Left, false, false);
        assert_eq!(p, [0x08, 0x02, 0x03, 0x03, 0x32, 0x00, 0x02, 0x00]);
    }

    #[test]
    fn payload_breathing_red() {
        let p = effect_payload(Effect::Breathing, 5, 2, Some('r'), WaveDirection::Right, false, false);
        assert_eq!(p, [0x08, 0x02, 0x02, 0x05, 0x16, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn payload_reactive_ripple_green() {
        // ReactiveRipple has distinct code 0x07
        let p = effect_payload(Effect::ReactiveRipple, 7, 3, Some('g'), WaveDirection::Right, false, true);
        assert_eq!(p, [0x08, 0x02, 0x07, 0x07, 0x24, 0x04, 0x00, 0x01]);
    }

    #[test]
    fn payload_reactive_mode_random() {
        // Random with reactive=true → modifier 0x01
        let p_normal = effect_payload(Effect::Random, 5, 3, None, WaveDirection::Right, false, false);
        let p_reactive = effect_payload(Effect::Random, 5, 3, None, WaveDirection::Right, true, false);
        assert_eq!(p_normal[6], 0x00); // non-reactive
        assert_eq!(p_reactive[6], 0x01); // reactive
        // All other bytes should be identical
        assert_eq!(p_normal[0..6], p_reactive[0..6]);
        assert_eq!(p_normal[7], p_reactive[7]);
    }

    #[test]
    fn payload_reactive_mode_aurora() {
        // Aurora with reactive flag
        let p_normal = effect_payload(Effect::Aurora, 5, 3, None, WaveDirection::Right, false, false);
        let p_reactive = effect_payload(Effect::Aurora, 5, 3, None, WaveDirection::Right, true, false);
        assert_eq!(p_normal[2], 0x0E); // same code
        assert_eq!(p_reactive[2], 0x0E);
        assert_eq!(p_normal[6], 0x00); // non-reactive
        assert_eq!(p_reactive[6], 0x01); // reactive
    }

    #[test]
    fn payload_fireworks_purple() {
        let p = effect_payload(Effect::Fireworks, 8, 4, Some('p'), WaveDirection::Right, true, true);
        assert_eq!(p, [0x08, 0x02, 0x11, 0x08, 0x32, 0x07, 0x01, 0x01]);
    }

    #[test]
    fn payload_marquee() {
        let p = effect_payload(Effect::Marquee, 1, 1, None, WaveDirection::Right, false, false);
        assert_eq!(p, [0x08, 0x02, 0x09, 0x01, 0x08, 0x08, 0x00, 0x00]);
    }

    #[test]
    fn payload_save_flag() {
        let p_no_save = effect_payload(Effect::Breathing, 5, 3, None, WaveDirection::Right, false, false);
        let p_save = effect_payload(Effect::Breathing, 5, 3, None, WaveDirection::Right, false, true);
        assert_eq!(p_no_save[7], 0x00);
        assert_eq!(p_save[7], 0x01);
        // All other bytes identical
        assert_eq!(p_no_save[0..7], p_save[0..7]);
    }
}
