use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use crate::audio::{BandAmplitudes, fft};

/// Temporarily redirect stderr → /dev/null for the duration of `f`.
/// Suppresses ALSA/JACK probe spam that corrupts TUI rendering.
fn with_stderr_suppressed<T, F: FnOnce() -> T>(f: F) -> T {
    unsafe {
        let devnull = libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_WRONLY);
        let saved = if devnull >= 0 { libc::dup(2) } else { -1 };
        if devnull >= 0 {
            libc::dup2(devnull, 2);
            libc::close(devnull);
        }
        let result = f();
        if saved >= 0 {
            libc::dup2(saved, 2);
            libc::close(saved);
        }
        result
    }
}

/// Parse /proc/asound/cards → map of card_id → friendly description.
/// e.g. "PCH" → "HDA Intel PCH", "NVidia" → "HDA NVidia"
fn alsa_card_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string("/proc/asound/cards") else {
        return map;
    };
    // Each card block: " 0 [PCH            ]: HDA-Intel - HDA Intel PCH"
    for line in content.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            continue;
        }
        let line = line.trim();
        if let (Some(bracket_start), Some(bracket_end)) = (line.find('['), line.find(']')) {
            let card_id = line[bracket_start + 1..bracket_end].trim().to_string();
            if let Some(dash_pos) = line.find(" - ") {
                let friendly = line[dash_pos + 3..].trim().to_string();
                if !card_id.is_empty() && !friendly.is_empty() {
                    map.insert(card_id, friendly);
                }
            }
        }
    }
    map
}

/// Returns true for ALSA device names worth showing to the user.
/// Filters out technical duplicates: plughw, surround*, front, dmix, dsnoop, null.
fn is_useful_device(name: &str) -> bool {
    if name == "null" || name == "default" {
        return false;
    }
    let prefixes_to_skip = ["plughw:", "surround", "front:", "dmix:", "dsnoop:"];
    !prefixes_to_skip.iter().any(|p| name.starts_with(p))
}

/// Build a human-readable label for an ALSA device name.
/// e.g. "sysdefault:CARD=PCH" + cards{"PCH":"HDA Intel PCH"} → "HDA Intel PCH — padrão"
fn friendly_label(name: &str, card_names: &HashMap<String, String>) -> String {
    // Extract CARD=XXX from name
    let card_id = name.split("CARD=").nth(1)
        .map(|s| s.split(',').next().unwrap_or(s).trim())
        .unwrap_or("");
    let card_label = if card_id.is_empty() {
        name.to_string()
    } else {
        card_names.get(card_id).cloned().unwrap_or_else(|| card_id.to_string())
    };

    if name.starts_with("sysdefault:") {
        format!("{card_label} — padrão")
    } else if name.starts_with("hw:") {
        format!("{card_label} — hw direto")
    } else {
        card_label
    }
}

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
/// Returns Vec of (device_name, display_label).
/// device_name is the raw ALSA name used internally; display_label is human-readable.
/// Call `setup_audio_env_for_root()` before this.
pub fn list_input_devices() -> Vec<(String, String)> {
    let card_names = alsa_card_names();
    let host = with_stderr_suppressed(|| cpal::default_host());
    let devices = match with_stderr_suppressed(|| host.devices()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    devices
        .filter_map(|dev| {
            let has_input = dev.supported_input_configs()
                .map(|mut c| c.next().is_some())
                .unwrap_or(false);
            if !has_input {
                return None;
            }
            let name = dev.name().ok()?;
            if !is_useful_device(&name) {
                return None;
            }
            let label = friendly_label(&name, &card_names);
            Some((name, label))
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
    let host = with_stderr_suppressed(|| cpal::default_host());

    let device = with_stderr_suppressed(|| match device_name {
        Some(name) => {
            host.devices()
                .map_err(|e| format!("Erro ao listar dispositivos: {e}"))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| format!("Dispositivo não encontrado: {name}"))
        }
        None => host.default_input_device()
            .ok_or_else(|| "Nenhum dispositivo de entrada padrão".to_string()),
    })?;

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
