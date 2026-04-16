use std::fmt;
use std::time::Instant;
use crate::audio::{BandAmplitudes, LedOutput};

/// Audio-reactive effect types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEffect {
    /// Brightness pulses with overall amplitude (beat detection).
    /// Uses `set_brightness` — fastest path (1 USB transfer).
    Pulse,
    /// Color shifts based on dominant frequency band.
    /// Bass → red/orange, Mid → green/yellow, Treble → blue/purple.
    ColorShift,
    /// Color wave that evolves over time, modulated by audio amplitude.
    Wave,
    /// Smooth breathing glow synchronized with audio envelope.
    Breathe,
    /// Cycles through the other 4 effects every ~8 seconds.
    Random,
}

impl AudioEffect {
    /// All concrete (non-Random) effects for cycling.
    pub const CYCLE: &'static [AudioEffect] = &[
        AudioEffect::Pulse,
        AudioEffect::ColorShift,
        AudioEffect::Wave,
        AudioEffect::Breathe,
    ];

    /// Parse from user-facing string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pulse"       => Some(AudioEffect::Pulse),
            "color-shift" => Some(AudioEffect::ColorShift),
            "wave"        => Some(AudioEffect::Wave),
            "breathe"     => Some(AudioEffect::Breathe),
            "random"      => Some(AudioEffect::Random),
            _ => None,
        }
    }

    /// User-facing label (Portuguese).
    pub fn label(self) -> &'static str {
        match self {
            AudioEffect::Pulse      => "Pulse — brilho pulsa com batida",
            AudioEffect::ColorShift => "Color-shift — cor muda com frequência",
            AudioEffect::Wave       => "Wave — onda de cor no ritmo",
            AudioEffect::Breathe    => "Breathe — respiração suave",
            AudioEffect::Random     => "Random — cicla entre efeitos",
        }
    }
}

impl fmt::Display for AudioEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AudioEffect::Pulse      => "pulse",
            AudioEffect::ColorShift => "color-shift",
            AudioEffect::Wave       => "wave",
            AudioEffect::Breathe    => "breathe",
            AudioEffect::Random     => "random",
        };
        write!(f, "{s}")
    }
}

/// Mutable state for the active effect renderer.
pub struct AudioEffectState {
    pub effect: AudioEffect,
    /// For Random: which concrete effect is currently active.
    random_idx: usize,
    /// For Random: when to switch to the next effect.
    random_deadline: Instant,
    /// For Wave/Breathe: time-based phase accumulator (radians).
    phase: f32,
}

impl AudioEffectState {
    pub fn new(effect: AudioEffect) -> Self {
        Self {
            effect,
            random_idx: 0,
            random_deadline: Instant::now(),
            phase: 0.0,
        }
    }

    /// Advance phase by `dt` seconds (call once per frame, dt ≈ 0.033).
    pub fn tick(&mut self, dt: f32) {
        self.phase += dt;
        // Prevent phase from growing unbounded
        if self.phase > 1000.0 {
            self.phase -= 1000.0;
        }
    }

    /// Render the current effect into an LED output command.
    pub fn render(&mut self, bands: &BandAmplitudes) -> LedOutput {
        match self.effect {
            AudioEffect::Pulse      => render_pulse(bands),
            AudioEffect::ColorShift => render_color_shift(bands),
            AudioEffect::Wave       => render_wave(bands, self.phase),
            AudioEffect::Breathe    => render_breathe(bands, self.phase),
            AudioEffect::Random => {
                // Check if it's time to switch effect
                if self.random_deadline.elapsed()
                    >= std::time::Duration::from_secs(8)
                {
                    self.random_idx =
                        (self.random_idx + 1) % AudioEffect::CYCLE.len();
                    self.random_deadline = Instant::now();
                }
                let current = AudioEffect::CYCLE[self.random_idx];
                match current {
                    AudioEffect::Pulse      => render_pulse(bands),
                    AudioEffect::ColorShift => render_color_shift(bands),
                    AudioEffect::Wave       => render_wave(bands, self.phase),
                    AudioEffect::Breathe    => render_breathe(bands, self.phase),
                    AudioEffect::Random     => unreachable!(),
                }
            }
        }
    }
}

// ── Effect renderers ──────────────────────────────────────────────────────────

/// Pulse: overall amplitude → brightness level (1–4).
/// Fastest path: maps to `set_brightness()` (1 USB transfer).
fn render_pulse(bands: &BandAmplitudes) -> LedOutput {
    let amp = (bands.bass * 0.5 + bands.mid * 0.3 + bands.treble * 0.2).min(1.0);
    let brightness = (amp * 4.0).ceil().clamp(1.0, 4.0) as u8;
    LedOutput::Brightness(brightness)
}

/// ColorShift: dominant band determines hue.
/// Bass → red/orange (hue 0–30°), Mid → green/yellow (hue 80–140°),
/// Treble → blue/purple (hue 220–280°).
fn render_color_shift(bands: &BandAmplitudes) -> LedOutput {
    let (hue, sat) = if bands.bass >= bands.mid && bands.bass >= bands.treble {
        // Bass dominant → red/orange, saturation from intensity
        let blend = bands.mid / (bands.bass + 0.001);
        (blend * 30.0, 1.0) // 0° (red) → 30° (orange) based on mid presence
    } else if bands.mid >= bands.treble {
        // Mid dominant → green/yellow
        let blend = bands.bass / (bands.mid + 0.001);
        (80.0 + blend * 60.0, 1.0) // 80° (green) → 140° (yellow-green)
    } else {
        // Treble dominant → blue/purple
        let blend = bands.mid / (bands.treble + 0.001);
        (220.0 + blend * 60.0, 1.0) // 220° (blue) → 280° (purple)
    };

    let overall = (bands.bass + bands.mid + bands.treble) / 3.0;
    let value = overall.clamp(0.2, 1.0); // never fully dark
    let brightness = (overall * 4.0).ceil().clamp(1.0, 4.0) as u8;
    let (r, g, b) = hsv_to_rgb(hue, sat, value);
    LedOutput::Color { r, g, b, brightness }
}

/// Wave: time-evolving hue wave modulated by audio amplitude.
fn render_wave(bands: &BandAmplitudes, phase: f32) -> LedOutput {
    let overall = (bands.bass * 0.5 + bands.mid * 0.3 + bands.treble * 0.2).min(1.0);
    // Hue rotates over time, speed modulated by bass
    let speed = 1.0 + bands.bass * 3.0; // faster rotation with more bass
    let hue = (phase * speed * 60.0) % 360.0; // ~60°/sec base rotation
    let value = (0.3 + overall * 0.7).min(1.0);
    let brightness = (overall * 4.0).ceil().clamp(1.0, 4.0) as u8;
    let (r, g, b) = hsv_to_rgb(hue, 1.0, value);
    LedOutput::Color { r, g, b, brightness }
}

/// Breathe: smooth sine envelope synchronized with audio.
fn render_breathe(bands: &BandAmplitudes, phase: f32) -> LedOutput {
    let overall = (bands.bass + bands.mid + bands.treble) / 3.0;
    // Breathing rate: 0.5–2.0 Hz depending on audio energy
    let rate = 0.5 + overall * 1.5;
    let envelope = (libm::sinf(phase * rate * 2.0 * std::f32::consts::PI) + 1.0) / 2.0;
    let value = 0.1 + envelope * 0.9 * overall.max(0.3);
    let brightness = (envelope * 4.0).ceil().clamp(1.0, 4.0) as u8;

    // Gentle hue based on dominant band
    let hue = if bands.bass >= bands.mid && bands.bass >= bands.treble {
        15.0  // warm orange
    } else if bands.mid >= bands.treble {
        110.0 // green
    } else {
        250.0 // blue-purple
    };

    let (r, g, b) = hsv_to_rgb(hue, 0.8, value);
    LedOutput::Color { r, g, b, brightness }
}

// ── HSV → RGB ─────────────────────────────────────────────────────────────────

/// Convert HSV (h: 0–360°, s: 0–1, v: 0–1) to RGB (0–255 each).
/// Standard algorithm, no crate needed.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - libm::fabsf(libm::fmodf(h_prime, 2.0) - 1.0));
    let m = v - c;

    let (r1, g1, b1) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HSV → RGB ──

    #[test]
    fn hsv_red() {
        let (r, g, b) = hsv_to_rgb(0.0, 1.0, 1.0);
        assert_eq!((r, g, b), (255, 0, 0));
    }

    #[test]
    fn hsv_green() {
        let (r, g, b) = hsv_to_rgb(120.0, 1.0, 1.0);
        assert_eq!((r, g, b), (0, 255, 0));
    }

    #[test]
    fn hsv_blue() {
        let (r, g, b) = hsv_to_rgb(240.0, 1.0, 1.0);
        assert_eq!((r, g, b), (0, 0, 255));
    }

    #[test]
    fn hsv_white() {
        let (r, g, b) = hsv_to_rgb(0.0, 0.0, 1.0);
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn hsv_black() {
        let (r, g, b) = hsv_to_rgb(0.0, 0.0, 0.0);
        assert_eq!((r, g, b), (0, 0, 0));
    }

    #[test]
    fn hsv_yellow() {
        let (r, g, b) = hsv_to_rgb(60.0, 1.0, 1.0);
        assert_eq!((r, g, b), (255, 255, 0));
    }

    // ── AudioEffect ──

    #[test]
    fn effect_from_str_valid() {
        assert_eq!(AudioEffect::from_str("pulse"),       Some(AudioEffect::Pulse));
        assert_eq!(AudioEffect::from_str("color-shift"), Some(AudioEffect::ColorShift));
        assert_eq!(AudioEffect::from_str("wave"),        Some(AudioEffect::Wave));
        assert_eq!(AudioEffect::from_str("breathe"),     Some(AudioEffect::Breathe));
        assert_eq!(AudioEffect::from_str("random"),      Some(AudioEffect::Random));
    }

    #[test]
    fn effect_from_str_invalid() {
        assert_eq!(AudioEffect::from_str("unknown"), None);
        assert_eq!(AudioEffect::from_str(""),        None);
    }

    #[test]
    fn effect_display_roundtrip() {
        for eff in AudioEffect::CYCLE {
            let s = eff.to_string();
            assert_eq!(AudioEffect::from_str(&s), Some(*eff),
                "roundtrip failed for {eff:?}");
        }
    }

    // ── Pulse ──

    #[test]
    fn pulse_silence_returns_min_brightness() {
        let bands = BandAmplitudes::default();
        match render_pulse(&bands) {
            LedOutput::Brightness(b) => assert_eq!(b, 1, "silence → brightness 1"),
            other => panic!("expected Brightness, got {other:?}"),
        }
    }

    #[test]
    fn pulse_loud_returns_max_brightness() {
        let bands = BandAmplitudes { bass: 1.0, mid: 1.0, treble: 1.0 };
        match render_pulse(&bands) {
            LedOutput::Brightness(b) => assert_eq!(b, 4, "loud → brightness 4"),
            other => panic!("expected Brightness, got {other:?}"),
        }
    }

    // ── ColorShift ──

    #[test]
    fn color_shift_bass_dominant_is_reddish() {
        let bands = BandAmplitudes { bass: 1.0, mid: 0.1, treble: 0.05 };
        match render_color_shift(&bands) {
            LedOutput::Color { r, g, b, .. } => {
                assert!(r > g && r > b, "bass-dominant should be reddish: r={r} g={g} b={b}");
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn color_shift_treble_dominant_is_bluish() {
        let bands = BandAmplitudes { bass: 0.05, mid: 0.1, treble: 1.0 };
        match render_color_shift(&bands) {
            LedOutput::Color { r, g, b, .. } => {
                assert!(b > r && b > g, "treble-dominant should be bluish: r={r} g={g} b={b}");
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    // ── Random cycling ──

    #[test]
    fn random_cycles_through_all_effects() {
        assert_eq!(AudioEffect::CYCLE.len(), 4);
        assert!(AudioEffect::CYCLE.contains(&AudioEffect::Pulse));
        assert!(AudioEffect::CYCLE.contains(&AudioEffect::ColorShift));
        assert!(AudioEffect::CYCLE.contains(&AudioEffect::Wave));
        assert!(AudioEffect::CYCLE.contains(&AudioEffect::Breathe));
        // Random should NOT be in the cycle array
        assert!(!AudioEffect::CYCLE.contains(&AudioEffect::Random));
    }

    // ── Wave + Breathe produce Color output ──

    #[test]
    fn wave_returns_color() {
        let bands = BandAmplitudes { bass: 0.5, mid: 0.3, treble: 0.2 };
        match render_wave(&bands, 1.0) {
            LedOutput::Color { r, g, b, brightness } => {
                assert!(r > 0 || g > 0 || b > 0, "wave should produce visible color");
                assert!(brightness >= 1 && brightness <= 4);
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn breathe_returns_color() {
        let bands = BandAmplitudes { bass: 0.5, mid: 0.3, treble: 0.2 };
        match render_breathe(&bands, 0.5) {
            LedOutput::Color { r, g, b, brightness } => {
                assert!(r > 0 || g > 0 || b > 0, "breathe should produce visible color");
                assert!(brightness >= 1 && brightness <= 4);
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    // ── AudioEffectState ──

    #[test]
    fn effect_state_tick_advances_phase() {
        let mut state = AudioEffectState::new(AudioEffect::Wave);
        let initial = state.phase;
        state.tick(0.033);
        assert!(state.phase > initial);
    }
}
