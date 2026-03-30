use std::fmt;

/// All supported lighting effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Breathing,
    Wave,
    Random,
    Reactive,
    Rainbow,
    Ripple,
    ReactiveRipple,
    Marquee,
    Fireworks,
    Raindrop,
    Aurora,
    ReactiveAurora,
    UserMode, // per-key / solid color (0x33)
}

impl Effect {
    pub fn code(self) -> u8 {
        match self {
            Effect::Breathing      => 0x02,
            Effect::Wave           => 0x03,
            Effect::Random         => 0x04,
            Effect::Reactive       => 0x04,
            Effect::Rainbow        => 0x05,
            Effect::Ripple         => 0x06,
            Effect::ReactiveRipple => 0x07,
            Effect::Marquee        => 0x09,
            Effect::Raindrop       => 0x0A,
            Effect::Aurora         => 0x0E,
            Effect::ReactiveAurora => 0x0E,
            Effect::Fireworks      => 0x11,
            Effect::UserMode       => 0x33,
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "breathing"       => Some(Effect::Breathing),
            "wave"            => Some(Effect::Wave),
            "random"          => Some(Effect::Random),
            "reactive"        => Some(Effect::Reactive),
            "rainbow"         => Some(Effect::Rainbow),
            "ripple"          => Some(Effect::Ripple),
            "reactiveripple"  => Some(Effect::ReactiveRipple),
            "marquee"         => Some(Effect::Marquee),
            "fireworks"       => Some(Effect::Fireworks),
            "raindrop"        => Some(Effect::Raindrop),
            "aurora"          => Some(Effect::Aurora),
            "reactiveaurora"  => Some(Effect::ReactiveAurora),
            _ => None,
        }
    }

    /// Whether this effect supports a single-color variant.
    pub fn supports_color_variant(self) -> bool {
        matches!(
            self,
            Effect::Breathing
                | Effect::Random
                | Effect::Reactive
                | Effect::Ripple
                | Effect::ReactiveRipple
                | Effect::Aurora
                | Effect::ReactiveAurora
                | Effect::Fireworks
                | Effect::Raindrop
        )
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Effect::Breathing      => "breathing",
            Effect::Wave           => "wave",
            Effect::Random         => "random",
            Effect::Reactive       => "reactive",
            Effect::Rainbow        => "rainbow",
            Effect::Ripple         => "ripple",
            Effect::ReactiveRipple => "reactiveripple",
            Effect::Marquee        => "marquee",
            Effect::Fireworks      => "fireworks",
            Effect::Raindrop       => "raindrop",
            Effect::Aurora         => "aurora",
            Effect::ReactiveAurora => "reactiveaurora",
            Effect::UserMode       => "user",
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
/// * `save` – persist to EEPROM
pub fn effect_payload(
    effect: Effect,
    speed: u8,
    brightness: u8,
    color_variant: Option<char>,
    direction: WaveDirection,
    save: bool,
) -> [u8; 8] {
    let eff_code = effect.code();
    let spd = speed.clamp(1, 10);
    let brt = brightness_byte(brightness);
    let save_byte = if save { 0x01 } else { 0x00 };

    let (colour, modifier) = match effect {
        Effect::Rainbow => (0x00, 0x00),
        Effect::Marquee => (0x08, 0x00),
        Effect::Wave    => (0x00, direction as u8),
        Effect::Reactive | Effect::ReactiveAurora | Effect::Fireworks => {
            (color_variant_code(color_variant), 0x01)
        }
        _ => (color_variant_code(color_variant), 0x00),
    };

    [0x08, 0x02, eff_code, spd, brt, colour, modifier, save_byte]
}
