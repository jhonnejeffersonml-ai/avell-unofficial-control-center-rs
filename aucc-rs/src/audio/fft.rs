use std::sync::{Arc, OnceLock};
use rustfft::{FftPlanner, num_complex::Complex};
use crate::audio::BandAmplitudes;

pub const FFT_SIZE: usize = 1024;

// Band boundaries in Hz (perceptually motivated).
const BASS_MAX_HZ:   f32 = 250.0;
const MID_MAX_HZ:    f32 = 2000.0;
const TREBLE_MAX_HZ: f32 = 8000.0;

// Smoothing constants — asymmetric EMA.
const ATTACK:  f32 = 0.6;   // fast rise (responsive to beats)
const RELEASE: f32 = 0.15;  // slow fall (no flicker)

/// Cached FFT plan — constructed once, reused forever.
/// FftPlanner::plan_fft_forward precomputes twiddle factors (expensive).
static FFT: OnceLock<Arc<dyn rustfft::Fft<f32>>> = OnceLock::new();

fn get_fft() -> Arc<dyn rustfft::Fft<f32>> {
    Arc::clone(FFT.get_or_init(|| {
        FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE)
    }))
}

/// Process mono audio samples through FFT and extract 3-band amplitudes.
///
/// Returns `None` if `mono_samples.len() < FFT_SIZE`.
/// Uses the LAST `FFT_SIZE` samples (most recent data).
/// Applies Hann windowing to reduce spectral leakage.
pub fn process_fft(mono_samples: &[f32], sample_rate: u32) -> Option<BandAmplitudes> {
    if mono_samples.len() < FFT_SIZE {
        return None;
    }

    let slice = &mono_samples[mono_samples.len() - FFT_SIZE..];

    // Apply Hann window
    let mut buffer: Vec<Complex<f32>> = slice.iter().enumerate()
        .map(|(i, &s)| {
            let window = 0.5
                * (1.0 - libm::cosf(
                    2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32,
                ));
            Complex { re: s * window, im: 0.0 }
        })
        .collect();

    let fft = get_fft();
    fft.process(&mut buffer);

    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    let bass_end   = (BASS_MAX_HZ / bin_hz) as usize;
    let mid_end    = (MID_MAX_HZ / bin_hz) as usize;
    let treble_end = (TREBLE_MAX_HZ / bin_hz).min((FFT_SIZE / 2) as f32) as usize;

    let rms = |start: usize, end: usize| -> f32 {
        if start >= end { return 0.0; }
        let sum_sq: f32 = buffer[start..end].iter()
            .map(|c| c.norm_sqr())
            .sum();
        libm::sqrtf(sum_sq / (end - start) as f32)
    };

    // Perceptual normalization: sqrt compression
    let normalize = |v: f32| libm::sqrtf(v).min(1.0);

    Some(BandAmplitudes {
        bass:   normalize(rms(1, bass_end)),
        mid:    normalize(rms(bass_end, mid_end)),
        treble: normalize(rms(mid_end, treble_end)),
    })
}

/// Exponential moving average with asymmetric attack/release.
/// Mutates `current` in-place toward `target`.
pub fn smooth_bands(current: &mut BandAmplitudes, target: &BandAmplitudes) {
    let lerp = |cur: f32, tgt: f32| -> f32 {
        let alpha = if tgt > cur { ATTACK } else { RELEASE };
        cur + alpha * (tgt - cur)
    };
    current.bass   = lerp(current.bass,   target.bass);
    current.mid    = lerp(current.mid,    target.mid);
    current.treble = lerp(current.treble, target.treble);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_fft_returns_none_for_short_input() {
        let short = vec![0.0f32; FFT_SIZE - 1];
        assert!(process_fft(&short, 44100).is_none());
    }

    #[test]
    fn process_fft_returns_some_for_valid_input() {
        let samples = vec![0.0f32; FFT_SIZE];
        assert!(process_fft(&samples, 44100).is_some());
    }

    #[test]
    fn bands_in_range_silence() {
        let silence = vec![0.0f32; FFT_SIZE];
        let bands = process_fft(&silence, 44100).unwrap();
        assert!(bands.bass >= 0.0 && bands.bass <= 1.0);
        assert!(bands.mid >= 0.0 && bands.mid <= 1.0);
        assert!(bands.treble >= 0.0 && bands.treble <= 1.0);
    }

    #[test]
    fn bands_in_range_sine() {
        // 200 Hz sine wave (bass band) at 44100 Hz
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| libm::sinf(2.0 * std::f32::consts::PI * 200.0 * i as f32 / 44100.0))
            .collect();
        let bands = process_fft(&samples, 44100).unwrap();
        assert!(bands.bass >= 0.0 && bands.bass <= 1.0,
            "bass={} out of range", bands.bass);
        assert!(bands.mid >= 0.0 && bands.mid <= 1.0);
        assert!(bands.treble >= 0.0 && bands.treble <= 1.0);
    }

    #[test]
    fn bass_dominates_for_low_freq_sine() {
        // 100 Hz sine — should be mostly bass
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| libm::sinf(2.0 * std::f32::consts::PI * 100.0 * i as f32 / 44100.0))
            .collect();
        let bands = process_fft(&samples, 44100).unwrap();
        assert!(bands.bass > bands.mid, "bass={} should > mid={}", bands.bass, bands.mid);
        assert!(bands.bass > bands.treble, "bass={} should > treble={}", bands.bass, bands.treble);
    }

    #[test]
    fn treble_dominates_for_high_freq_sine() {
        // 4000 Hz sine — should be mostly treble
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| libm::sinf(2.0 * std::f32::consts::PI * 4000.0 * i as f32 / 44100.0))
            .collect();
        let bands = process_fft(&samples, 44100).unwrap();
        assert!(bands.treble > bands.bass, "treble={} should > bass={}", bands.treble, bands.bass);
        assert!(bands.treble > bands.mid, "treble={} should > mid={}", bands.treble, bands.mid);
    }

    #[test]
    fn smoothing_converges_attack() {
        let mut current = BandAmplitudes::default(); // all 0.0
        let target = BandAmplitudes { bass: 1.0, mid: 1.0, treble: 1.0 };
        for _ in 0..20 {
            smooth_bands(&mut current, &target);
        }
        assert!(current.bass >= 0.99, "bass={} should converge to ~1.0", current.bass);
        assert!(current.mid >= 0.99);
        assert!(current.treble >= 0.99);
    }

    #[test]
    fn smoothing_converges_release() {
        let mut current = BandAmplitudes { bass: 1.0, mid: 1.0, treble: 1.0 };
        let target = BandAmplitudes::default(); // all 0.0
        for _ in 0..50 {
            smooth_bands(&mut current, &target);
        }
        assert!(current.bass <= 0.01, "bass={} should decay to ~0.0", current.bass);
    }

    #[test]
    fn smoothing_attack_faster_than_release() {
        let mut rising = BandAmplitudes::default();
        let mut falling = BandAmplitudes { bass: 1.0, mid: 1.0, treble: 1.0 };
        let up_target = BandAmplitudes { bass: 1.0, mid: 1.0, treble: 1.0 };
        let down_target = BandAmplitudes::default();

        // After 3 frames of attack
        for _ in 0..3 {
            smooth_bands(&mut rising, &up_target);
        }
        // After 3 frames of release
        for _ in 0..3 {
            smooth_bands(&mut falling, &down_target);
        }
        // Rising should have moved more than falling has decayed
        assert!(rising.bass > (1.0 - falling.bass),
            "attack movement {} should > release movement {}", rising.bass, 1.0 - falling.bass);
    }

    #[test]
    fn uses_last_fft_size_samples() {
        // Provide MORE than FFT_SIZE samples — function should use the last FFT_SIZE
        let mut samples = vec![0.0f32; FFT_SIZE * 2];
        // Put a loud 100Hz tone only in the last FFT_SIZE samples
        for i in FFT_SIZE..(FFT_SIZE * 2) {
            samples[i] = libm::sinf(2.0 * std::f32::consts::PI * 100.0 * i as f32 / 44100.0);
        }
        let bands = process_fft(&samples, 44100).unwrap();
        assert!(bands.bass > 0.01, "should detect the sine in the last window");
    }
}
