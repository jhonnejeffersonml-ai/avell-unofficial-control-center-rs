pub mod capture;
pub mod effects;
pub mod fft;

/// Amplitudes per frequency band, normalized to 0.0–1.0.
/// Shared between the audio capture callback and the LED drive loop
/// via `Arc<Mutex<BandAmplitudes>>`.
#[derive(Debug, Clone, Default)]
pub struct BandAmplitudes {
    pub bass:   f32,
    pub mid:    f32,
    pub treble: f32,
}

/// Commands sent from TUI → audio engine thread.
pub enum AudioCmd {
    /// Start audio capture with optional device name and selected effect.
    Enable {
        device_name: Option<String>,
        effect: effects::AudioEffect,
    },
    /// Stop audio capture and signal LED restore.
    Disable,
    /// Change the active effect without restarting capture.
    SetEffect(effects::AudioEffect),
}

/// Output from the effect renderer → sent to UsbCmd channel.
#[derive(Debug, Clone, Copy)]
pub enum LedOutput {
    /// Only change brightness (1 USB transfer via `set_brightness`).
    Brightness(u8),
    /// Change color + brightness (10 USB transfers via `apply_mono_color`).
    Color { r: u8, g: u8, b: u8, brightness: u8 },
}
