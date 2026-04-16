use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use crate::audio::{BandAmplitudes, fft};

/// Set XDG_RUNTIME_DIR so cpal's ALSA backend can reach PipeWire.
/// Only modifies env when running as root (euid == 0) and PKEXEC_UID is set.
///
/// # Safety
/// Calls `set_var` which is unsafe in multi-threaded context.
/// MUST be called before any threads are spawned.
pub fn setup_audio_env_for_root() {
    if unsafe { libc::geteuid() } == 0 {
        if let Ok(uid_str) = std::env::var("PKEXEC_UID") {
            // Validate: PKEXEC_UID must be a valid non-negative integer
            if let Ok(uid) = uid_str.parse::<u32>() {
                let xdg = format!("/run/user/{uid}");
                // Safe: called before any audio threads exist
                unsafe {
                    std::env::set_var("XDG_RUNTIME_DIR", &xdg);
                    std::env::set_var("PIPEWIRE_RUNTIME_DIR", &xdg);
                    std::env::set_var("PULSE_SERVER",
                        format!("unix:{xdg}/pulse/native"));
                }
            }
            // If PKEXEC_UID is not a valid u32, skip silently (don't construct paths
            // from untrusted input — T-01-04 mitigation).
        }
    }
}

/// Enumerate all input-capable audio devices.
/// Returns Vec of (device_name, display_description).
/// Call `setup_audio_env_for_root()` before this.
pub fn list_input_devices() -> Vec<(String, String)> {
    let host = cpal::default_host();
    let devices = match host.devices() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    devices
        .filter_map(|dev| {
            // Only include devices that support input
            let has_input = dev.supported_input_configs()
                .map(|mut c| c.next().is_some())
                .unwrap_or(false);
            if !has_input {
                return None;
            }
            let name = dev.name().ok()?;
            Some((name.clone(), name))
        })
        .collect()
}

/// Build a cpal input stream that feeds mono audio into FFT processing
/// and updates shared `BandAmplitudes`.
///
/// The returned `cpal::Stream` must be kept alive (not dropped) while
/// capturing. Dropping it stops the audio callback.
///
/// `device_name`: if None, uses default input device.
pub fn build_input_stream(
    device_name: Option<&str>,
    bands: Arc<Mutex<BandAmplitudes>>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();

    let device = match device_name {
        Some(name) => {
            host.devices()
                .map_err(|e| format!("Erro ao listar dispositivos: {e}"))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| format!("Dispositivo não encontrado: {name}"))?
        }
        None => host.default_input_device()
            .ok_or_else(|| "Nenhum dispositivo de entrada padrão".to_string())?,
    };

    let config = device.default_input_config()
        .map_err(|e| format!("Erro na configuração de áudio: {e}"))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;

    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // Mix to mono
            let mono: Vec<f32> = data.chunks(channels)
                .map(|ch| ch.iter().sum::<f32>() / channels as f32)
                .collect();
            // FFT → band extraction
            if let Some(new_bands) = fft::process_fft(&mono, sample_rate) {
                if let Ok(mut b) = bands.lock() {
                    fft::smooth_bands(&mut b, &new_bands);
                }
            }
        },
        |err| {
            // Error callback — must NOT panic (T-01-05: cpal runs this on RT thread)
            eprintln!("cpal stream error: {err}");
        },
        None, // no timeout
    ).map_err(|e| format!("Erro ao criar stream de áudio: {e}"))?;

    stream.play().map_err(|e| format!("Erro ao iniciar stream: {e}"))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_setup_does_not_panic() {
        // In CI/non-root, this is a no-op (euid != 0).
        setup_audio_env_for_root();
    }

    #[test]
    fn list_devices_returns_vec() {
        // May return empty in CI (no audio devices), but should not panic.
        setup_audio_env_for_root();
        let devices = list_input_devices();
        // Just verify it's a valid Vec — content depends on environment
        let _ = devices.len();
    }
}
