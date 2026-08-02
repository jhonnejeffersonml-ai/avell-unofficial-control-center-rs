//! Reapply the persisted keyboard state after the EC clears the backlight
//! (boot on battery, AC<->battery transition, resume from suspend).

use crate::config::{KeyboardConfig, KeyboardMode};
use crate::keyboard::KeyboardDevice;
use crate::keyboard::effects::{Effect, WaveDirection, effect_payload};

/// Build the effect payload for a config in Effect mode.
///
/// Returns None for the other modes, and for an effect name the firmware does
/// not know. `save` is always false: restoring must not rewrite the EEPROM.
pub fn payload_for(cfg: &KeyboardConfig) -> Option<[u8; 8]> {
    if cfg.mode != KeyboardMode::Effect {
        return None;
    }
    let effect = Effect::from_str(&cfg.effect)?;
    let direction = WaveDirection::from_str(&cfg.direction).unwrap_or_default();
    Some(effect_payload(
        effect,
        cfg.speed,
        cfg.brightness,
        cfg.letter,
        direction,
        cfg.reactive,
        false,
    ))
}

/// Apply the persisted state to the device.
///
/// `save` is false on every path — the EEPROM already holds whatever the user
/// chose to persist there, and a restore is not a new user decision.
pub fn apply(dev: &KeyboardDevice, cfg: &KeyboardConfig) -> rusb::Result<()> {
    match cfg.mode {
        KeyboardMode::Off => dev.disable(),
        KeyboardMode::Mono => {
            dev.apply_mono_color(cfg.r, cfg.g, cfg.b, cfg.brightness, false)
        }
        KeyboardMode::HAlt | KeyboardMode::VAlt => dev.apply_alt_color(
            cfg.r, cfg.g, cfg.b,
            cfg.r2, cfg.g2, cfg.b2,
            cfg.brightness,
            cfg.mode == KeyboardMode::HAlt,
            false,
        ),
        KeyboardMode::Effect => match payload_for(cfg) {
            Some(payload) => dev.apply_effect(&payload),
            // Unknown effect name in the file: leave the keyboard untouched
            // rather than guessing a different effect.
            None => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{KeyboardConfig, KeyboardMode};
    use crate::keyboard::effects::{Effect, WaveDirection, effect_payload};

    #[test]
    fn payload_none_for_non_effect_modes() {
        for mode in [KeyboardMode::Off, KeyboardMode::Mono, KeyboardMode::HAlt, KeyboardMode::VAlt] {
            let cfg = KeyboardConfig { mode, ..Default::default() };
            assert_eq!(payload_for(&cfg), None, "mode {mode:?} nao deve gerar payload de efeito");
        }
    }

    #[test]
    fn payload_matches_effect_payload_for_wave() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 7,
            brightness: 3,
            direction: "left".to_string(),
            letter: None,
            reactive: false,
            ..Default::default()
        };
        let expected = effect_payload(Effect::Wave, 7, 3, None, WaveDirection::Left, false, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }

    #[test]
    fn payload_carries_letter_and_reactive() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "breathing".to_string(),
            speed: 5,
            brightness: 4,
            direction: "right".to_string(),
            letter: Some('g'),
            reactive: true,
            ..Default::default()
        };
        let expected =
            effect_payload(Effect::Breathing, 5, 4, Some('g'), WaveDirection::Right, true, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }

    #[test]
    fn payload_never_sets_the_eeprom_save_byte() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "rainbow".to_string(),
            ..Default::default()
        };
        assert_eq!(payload_for(&cfg).unwrap()[7], 0x00);
    }

    #[test]
    fn payload_none_for_unknown_effect_name() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "nao_existe".to_string(),
            ..Default::default()
        };
        assert_eq!(payload_for(&cfg), None);
    }

    #[test]
    fn unknown_direction_falls_back_to_right() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 5,
            brightness: 4,
            direction: "diagonal".to_string(),
            ..Default::default()
        };
        let expected = effect_payload(Effect::Wave, 5, 4, None, WaveDirection::Right, false, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }
}
