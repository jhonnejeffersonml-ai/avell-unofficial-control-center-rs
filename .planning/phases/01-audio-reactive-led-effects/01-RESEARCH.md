# Phase 1: Audio-Reactive LED Effects — Research

**Researched:** 2026-04-15
**Domain:** Rust audio capture + FFT + real-time LED control + ratatui TUI integration
**Confidence:** HIGH (codebase fully read; crates verified on crates.io; environment probed)

---

## Summary

This phase adds a real-time audio-sync mode to `aucc-ui`: a dedicated audio capture + FFT thread feeds band amplitudes to a separate LED-drive loop, while the existing TUI gets one new menu entry (`Screen::AudioSync`) for configuration. The TUI event loop remains unchanged — it only sends enable/disable commands over a new channel.

The primary technical challenge is **audio access from a root process**: `aucc-ui` always runs as root (enforced by `require_root()` in `src/bin/aucc-ui.rs`), while PipeWire runs in the user session at `/run/user/$UID/`. The solution is straightforward — `pkexec` sets `PKEXEC_UID` in the environment, and we set `XDG_RUNTIME_DIR=/run/user/$PKEXEC_UID` before initializing `cpal`. This is confirmed working: `pkexec env` shows `PKEXEC_UID=1000` is always set.

A secondary but critical build dependency is `libasound2-dev`. The `cpal` crate links against `alsa-sys` (which uses `pkg-config` to find `libasound`). This system package must be installed before `cargo build`; it is **not** currently in the project's install script.

**Primary recommendation:** Implement a two-thread architecture — one `cpal`-driven audio capture+FFT thread, one LED-drive thread — communicating via `Arc<Mutex<BandAmplitudes>>`. The TUI controls the whole thing via a `mpsc::Sender<AudioCmd>`. The existing `UsbCmd` channel (for the USB worker thread) is reused for LED output.

---

## User Constraints

*(No CONTEXT.md found for this phase — constraints derive from project roadmap and exploration notes.)*

### Locked Decisions (from ROADMAP + exploration notes)
- Audio analysis via FFT by frequency bands (bass/mid/treble → different colors)
- Multiple effects: pulse, color-shift, wave, breathe, random (cycles randomly)
- Audio source: user-configurable (system output monitor, microphone, app)
- Integration: new "Audio Sync" screen/menu entry in existing TUI (`aucc-ui`)
- Hardware targets: both lightbar AND keyboard simultaneously
- Crates: `cpal` 0.17.3, `rustfft` (6.x), `libm`

### the agent's Discretion
- Exact synchronization primitive (Arc<Mutex> vs channels vs atomics)
- Effect sub-system architecture (how effects are implemented internally)
- Smoothing parameters (lerp alpha, timing constants)
- Exact FFT bin boundaries for bass/mid/treble
- Whether to add a new `UsbCmd` variant or reuse `MonoColor`

### Deferred Ideas (OUT OF SCOPE)
- Per-key RGB patterns (requires bulk transfer color map support not in current code)
- BPM detection / beat detection beyond simple amplitude threshold
- Persisting audio sync config to disk

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `cpal` | 0.17.3 | Audio capture — device enumeration + input stream | Canonical Rust audio crate; already selected by project |
| `rustfft` | 6.4.1 | FFT computation (SIMD-accelerated) | Standard Rust FFT; AVX/SSE4.1 auto-detected on x86_64 |
| `libm` | 0.2.16 | `libm::powf`, `libm::log10` for perceptual scaling | Already planned by project; pure Rust, no system dep |

### Supporting (no additions needed)
| Library | Version | Purpose | Already In |
|---------|---------|---------|------------|
| `std::sync::mpsc` | stdlib | TUI→AudioEngine command channel | yes |
| `std::sync::{Arc, Mutex}` | stdlib | Shared band state between audio and LED threads | yes |
| `libc` | 0.2 | `geteuid()`, `setenv()` for root→user env setup | already in Cargo.toml |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `Arc<Mutex<BandAmplitudes>>` | `std::sync::atomic::AtomicU32` per band | Atomics eliminate lock contention but limit to integer values; Mutex is simpler and latency is fine |
| `rustfft` | `microfft` | `microfft` is allocation-free but fixed-size; `rustfft` is more flexible and faster via SIMD |
| ALSA (cpal default) | JACK | JACK needs JACK server; ALSA via PipeWire is sufficient and zero extra setup |

**Installation:**
```bash
# System dependency (required BEFORE cargo build)
sudo apt install libasound2-dev

# Rust dependencies — add to aucc-rs/Cargo.toml
# [dependencies]
# cpal = "0.17.3"
# rustfft = "6.4.1"
# libm = "0.2"   (already planned)
```

**Version verification:** [VERIFIED: crates.io API]
- `cpal`: 0.17.3 (latest as of 2026-04-15)
- `rustfft`: 6.4.1 (latest as of 2026-04-15)
- `libm`: 0.2.16 (latest as of 2026-04-15)

---

## Architecture Patterns

### Recommended Module Structure
```
aucc-rs/src/
├── audio/
│   ├── mod.rs          # public API: AudioEngine, AudioCmd, BandAmplitudes
│   ├── capture.rs      # cpal stream setup + device enumeration
│   ├── fft.rs          # FFT processing, band splitting, smoothing
│   └── effects.rs      # AudioEffect enum + render logic → (r,g,b,brightness)
├── ui/
│   └── tui.rs          # add Screen::AudioSync, AudioSyncState; extend AppState
└── ...existing...
```

### Pattern 1: Two-Thread Audio→LED Architecture

**What:** Audio capture thread feeds an `Arc<Mutex<BandAmplitudes>>`. A separate LED-drive thread reads this at ~30fps and sends `UsbCmd` to the existing USB worker.

**Why not audio→TUI→LED:** The TUI tick is 50ms (keyboard events). Audio + USB takes 15–23ms. Keeping audio→LED independent of the TUI event loop ensures ≤50ms latency regardless of TUI activity.

```rust
// Source: based on cpal enumerate.rs + record_wav.rs from RustAudio/cpal v0.17.3
// + existing spawn_usb_worker pattern in src/ui/tui.rs

pub struct BandAmplitudes {
    pub bass:   f32,   // 0.0–1.0, smoothed
    pub mid:    f32,
    pub treble: f32,
}

pub enum AudioCmd {
    Enable { device_id: Option<String>, effect: AudioEffect },
    Disable,
    SetEffect(AudioEffect),
}

pub fn spawn_audio_engine(
    audio_rx: mpsc::Receiver<AudioCmd>,
    usb_tx: mpsc::Sender<UsbCmd>,         // reuse existing channel
    lb_path: Option<PathBuf>,
) {
    thread::spawn(move || {
        // Set XDG_RUNTIME_DIR so cpal/ALSA can reach PipeWire (see Pitfall 1)
        setup_pipewire_env();

        let bands = Arc::new(Mutex::new(BandAmplitudes::default()));

        // LED drive sub-thread at 30fps
        let bands_clone = Arc::clone(&bands);
        thread::spawn(move || led_drive_loop(bands_clone, usb_tx, lb_path));

        // Command loop: wait for Enable/Disable/SetEffect
        for cmd in audio_rx { /* ... */ }
    });
}
```

**Thread count:** 3 total (TUI/main + USB worker [existing] + audio capture + LED drive). Audio capture and LED drive can be merged into one if frame timing allows, but separate is cleaner.

### Pattern 2: cpal Input Stream Setup on Linux

```rust
// Source: RustAudio/cpal v0.17.3 examples/record_wav.rs
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn build_input_stream(
    device_id: Option<String>,
    bands: Arc<Mutex<BandAmplitudes>>,
) -> anyhow::Result<cpal::Stream> {
    let host = cpal::default_host();  // ALSA on Linux

    let device = match device_id {
        Some(id) => {
            let parsed = id.parse::<cpal::DeviceId>()?;
            host.device_by_id(&parsed)?
        }
        None => host.default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no input device"))?,
    };

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
            // Mix to mono, compute FFT, update bands
            let mono: Vec<f32> = data.chunks(channels)
                .map(|ch| ch.iter().sum::<f32>() / channels as f32)
                .collect();
            if let Some(new_bands) = process_fft(&mono, sample_rate) {
                if let Ok(mut b) = bands.lock() {
                    smooth_bands(&mut b, new_bands);
                }
            }
        },
        |err| eprintln!("cpal stream error: {err}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
```

### Pattern 3: FFT Band Extraction

```rust
// Source: ejmahler/RustFFT v6.4.1 README + standard DSP practice
use rustfft::{FftPlanner, num_complex::Complex};

const FFT_SIZE: usize = 1024;

// Call ONCE at startup — FftPlanner is expensive to construct
fn make_fft() -> Arc<dyn rustfft::Fft<f32>> {
    let mut planner = FftPlanner::<f32>::new();
    planner.plan_fft_forward(FFT_SIZE)
}

fn process_fft(mono_samples: &[f32], sample_rate: u32) -> Option<BandAmplitudes> {
    if mono_samples.len() < FFT_SIZE { return None; }

    // Use the most recent FFT_SIZE samples
    let slice = &mono_samples[mono_samples.len() - FFT_SIZE..];

    // Apply Hann window to reduce spectral leakage
    let mut buffer: Vec<Complex<f32>> = slice.iter().enumerate()
        .map(|(i, &s)| {
            let window = 0.5 * (1.0 - f32::cos(2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32));
            Complex { re: s * window, im: 0.0 }
        })
        .collect();

    FFT.process(&mut buffer);  // FFT is Arc<dyn Fft>, reuse across calls

    // Bin resolution: sample_rate / FFT_SIZE ≈ 43 Hz per bin at 44.1kHz
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;

    // Band boundaries (perceptually motivated)
    let bass_end   = (250.0 / bin_hz) as usize;  // 20–250 Hz  → bins 0–5
    let mid_end    = (2000.0 / bin_hz) as usize; // 250–2kHz   → bins 6–46
    let treble_end = (8000.0 / bin_hz) as usize; // 2k–8kHz    → bins 46–185

    let rms = |range: std::ops::Range<usize>| -> f32 {
        let sum_sq: f32 = buffer[range].iter()
            .map(|c| c.norm_sqr())
            .sum();
        libm::sqrtf(sum_sq / FFT_SIZE as f32)
    };

    // Perceptual compression: sqrt for amplitude (not power)
    let normalize = |v: f32| libm::sqrtf(v).min(1.0);

    Some(BandAmplitudes {
        bass:   normalize(rms(1..bass_end)),
        mid:    normalize(rms(bass_end..mid_end)),
        treble: normalize(rms(mid_end..treble_end)),
    })
}
```

**Critical: Plan FFT ONCE**, store in a module-level `OnceLock` or pass as parameter. Calling `FftPlanner::plan_fft_forward()` on every audio callback is a performance catastrophe.

### Pattern 4: Smoothing (Anti-Flicker)

```rust
// Exponential moving average — tuned constants
const ATTACK:  f32 = 0.6;  // fast rise  (responsive to beats)
const RELEASE: f32 = 0.15; // slow fall  (natural decay, no flicker)

fn smooth_bands(current: &mut BandAmplitudes, target: BandAmplitudes) {
    let lerp = |cur: f32, tgt: f32| -> f32 {
        let alpha = if tgt > cur { ATTACK } else { RELEASE };
        cur + alpha * (tgt - cur)
    };
    current.bass   = lerp(current.bass,   target.bass);
    current.mid    = lerp(current.mid,    target.mid);
    current.treble = lerp(current.treble, target.treble);
}
```

### Pattern 5: Audio Effect System

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioEffect {
    Pulse,       // overall amplitude → brightness (uses set_brightness, 1 USB transfer)
    ColorShift,  // dominant band → hue rotation
    Wave,        // time-based wave with beat-sync amplitude
    Breathe,     // low-pass envelope → smooth glow
    Random,      // cycle through others with timer
}

pub struct AudioEffectState {
    pub effect: AudioEffect,
    phase: f32,             // for Wave/Breathe time evolution
    random_deadline: std::time::Instant,
    random_idx: usize,
}

impl AudioEffectState {
    pub fn render(&mut self, bands: &BandAmplitudes) -> LedOutput {
        match self.effect {
            AudioEffect::Pulse => {
                let amp = (bands.bass * 0.5 + bands.mid * 0.3 + bands.treble * 0.2)
                    .min(1.0);
                let brightness = (amp * 4.0).ceil().clamp(1.0, 4.0) as u8;
                LedOutput::Brightness(brightness)  // → UsbCmd via set_brightness (1 transfer)
            }
            AudioEffect::ColorShift => {
                // Dominant band determines hue
                let hue = if bands.bass > bands.mid && bands.bass > bands.treble {
                    0.0f32  // red — bass
                } else if bands.mid > bands.treble {
                    120.0   // green — mid
                } else {
                    240.0   // blue — treble
                };
                let brightness = ((bands.bass + bands.mid + bands.treble) / 3.0 * 4.0)
                    .ceil().clamp(1.0, 4.0) as u8;
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                LedOutput::Color { r, g, b, brightness }
            }
            AudioEffect::Random => {
                // Cycle effect every N seconds
                if self.random_deadline.elapsed() > std::time::Duration::from_secs(8) {
                    const CYCLE: &[AudioEffect] = &[
                        AudioEffect::Pulse, AudioEffect::ColorShift,
                        AudioEffect::Wave,  AudioEffect::Breathe,
                    ];
                    self.random_idx = (self.random_idx + 1) % CYCLE.len();
                    self.effect = CYCLE[self.random_idx];
                    self.random_deadline = std::time::Instant::now();
                }
                self.render(bands)  // delegate
            }
            // ... Wave, Breathe follow same pattern
        }
    }
}

pub enum LedOutput {
    Brightness(u8),                           // → UsbCmd::SetBrightness (1 transfer)
    Color { r: u8, g: u8, b: u8, brightness: u8 },  // → UsbCmd::MonoColor (10 transfers)
}
```

**Optimization:** Use `Brightness`-only output for `Pulse` effect — it maps to `set_brightness()` (1 USB transfer, fastest path). Use full `MonoColor` only for color-changing effects.

### Pattern 6: Adding Screen::AudioSync to TUI

The TUI uses a `Screen` enum with flat navigation. "Tab" is implemented by adding a new `Screen` variant:

```rust
// In tui.rs — add to Screen enum
pub enum Screen {
    // ... existing ...
    AudioSync,        // new: main audio sync config
    AudioDevice,      // new: device picker list
}

// Add to main_items_dynamic()
"──────────────────",
"🎵 Audio Sync — LEDs",

// Add to confirm() for Screen::Main
"🎵 Audio Sync — LEDs" => {
    state.go_to(Screen::AudioSync, 0);
}

// AppState additions
audio_tx: Option<mpsc::Sender<AudioCmd>>,
audio_enabled: bool,
audio_effect: AudioEffect,
audio_devices: Vec<(String, String)>,   // (id, description)
```

### Pattern 7: Previous Config Snapshot (Restore on Disable)

```rust
// In AppState — capture before enabling audio sync
struct LedSnapshot {
    mode: String,             // "mono", "effect", etc.
    r: u8, g: u8, b: u8,
    brightness: u8,
    effect_payload: Option<[u8; 8]>,
}

// On enable: save snapshot
// On disable: restore by sending saved UsbCmd to usb_tx
```

### Anti-Patterns to Avoid

- **Creating FftPlanner every audio callback:** Each callback arrives every ~23ms; planner construction is O(N log N) precomputation. Plan once, store result.
- **Sending LED commands from the cpal audio callback:** The cpal callback runs on a real-time audio thread with priority scheduling. USB I/O (blocking) in this callback will cause XRUN errors and audio glitches. Always pass data through `Arc<Mutex>` or a lock-free ring buffer.
- **Using `save: true` for audio sync LED updates:** EEPROM writes are slow and have limited write cycles. Audio sync should always use `save: false`.
- **Calling `apply_mono_color` at audio callback rate (~44Hz):** At 10 USB transfers × ~1.5ms each = 15ms per call. At 44 callbacks/sec this saturates USB. The LED drive loop must be capped at ~30fps (33ms interval) independently of callback rate.
- **Locking `Arc<Mutex<BandAmplitudes>>` in the TUI render path:** The TUI render runs while holding the terminal lock. Take a clone of bands before rendering, not inside `render()`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Audio device enumeration | Custom ALSA device scanner | `cpal` `host.devices()` + `device.description()` | cpal handles ALSA/PipeWire compat, device IDs, configs |
| FFT computation | DFT loop | `rustfft` | Hand-written DFT is O(N²); rustfft is O(N log N) + AVX SIMD |
| Hann windowing | Ad-hoc | Standard formula (see code example above) | Spectral leakage without window makes bands inaccurate |
| HSV→RGB conversion | Color math | Small inline function (6 lines, standard formula) | No crate needed, truly trivial, crate is overkill |
| Audio thread priority | `pthread_setschedparam` | `cpal` handles this via `audio_thread_priority` feature | cpal abstracts real-time priority internally |

**Key insight:** FFT band extraction looks simple but windowing, normalization, and logarithmic scaling are each individually non-obvious. Follow the verified pattern above exactly.

---

## Common Pitfalls

### Pitfall 1: Root Process Cannot Access PipeWire (CRITICAL)
**What goes wrong:** `cpal::default_host()` returns ALSA. The ALSA `pipewire` plugin reads `XDG_RUNTIME_DIR` to find the PipeWire socket. When `aucc-ui` runs as root (via `pkexec`), `XDG_RUNTIME_DIR` is `/run/user/0/` or unset — PipeWire socket doesn't exist there. Result: `cpal` fails to enumerate devices or opens the raw ALSA hw device (which may not capture monitor audio).

**Why it happens:** `src/bin/aucc-ui.rs` calls `require_root()` which enforces `geteuid() == 0`. PipeWire runs in the user session at `/run/user/1000/`.

**How to avoid:**
```rust
// Call this before ANY cpal initialization
fn setup_audio_env_for_root() {
    // pkexec always sets PKEXEC_UID — VERIFIED from `pkexec env` output
    if unsafe { libc::geteuid() } == 0 {
        if let Ok(uid_str) = std::env::var("PKEXEC_UID") {
            let xdg = format!("/run/user/{}", uid_str);
            // Safe: we're setting env for our own process, before any threads use it
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", &xdg);
                std::env::set_var("PIPEWIRE_RUNTIME_DIR", &xdg);
                // For PulseAudio compat layer (pipewire-pulse)
                std::env::set_var("PULSE_SERVER",
                    format!("unix:{}/pulse/native", xdg));
            }
        }
    }
}
```
**Warning signs:** `cpal` `host.devices()` returns only `hw:0,0` type devices, or returns empty list, or `default_input_device()` panics.

---

### Pitfall 2: `libasound2-dev` Not Installed (BUILD BLOCKER)
**What goes wrong:** `cargo build` fails with `The system library 'alsa' required by crate 'alsa-sys' was not found`. The runtime library (`libasound2t64`) is installed, but `cpal` needs the **dev package** (pkg-config `.pc` file + headers) at **build time**.

**How to avoid:** Add to the install script and Wave 0 setup:
```bash
sudo apt install libasound2-dev
```
**Verified:** Build was attempted in this research session and failed with exactly this error until the package is installed. [VERIFIED: local environment test]

---

### Pitfall 3: Calling Blocking USB I/O in cpal Audio Callback
**What goes wrong:** XRUN errors ("buffer underrun"), glitchy audio, and audio callback stalls if USB blocking calls happen inside the cpal data callback.

**How to avoid:** The cpal callback should ONLY: mix channels to mono, copy to a ring buffer or accumulate in a `VecDeque`, update `Arc<Mutex<BandAmplitudes>>`. A separate thread reads the bands and calls `UsbCmd`.

---

### Pitfall 4: Monitor Source Enumeration
**What goes wrong:** `host.default_input_device()` returns the microphone, not the system audio output monitor. The user wants to sync to music/video playing on the laptop speakers, not mic input.

**How to avoid:** Enumerate all devices with `host.devices()` and present them in the TUI picker. On PipeWire/ALSA, the monitor source typically has a description containing "Monitor" or has the same name as the output sink with `.monitor` suffix in the PulseAudio layer. Users need to pick it explicitly. The TUI `Screen::AudioDevice` picker should show `device.description()` for each device.

**Note:** With `XDG_RUNTIME_DIR` set correctly (Pitfall 1 fix), the PipeWire ALSA plugin exposes all PipeWire audio nodes including monitor sources. Without the fix, only raw ALSA hardware devices appear.

---

### Pitfall 5: FftPlanner Constructed Every Frame
**What goes wrong:** Planner construction precomputes twiddle factors — O(N log N). At 44 calls/second this wastes ~2–5ms per frame on CPU.

**How to avoid:** Store the planned FFT in a `OnceLock<Arc<dyn rustfft::Fft<f32>>>`:
```rust
static FFT: OnceLock<Arc<dyn rustfft::Fft<f32>>> = OnceLock::new();
fn get_fft() -> Arc<dyn rustfft::Fft<f32>> {
    Arc::clone(FFT.get_or_init(|| {
        FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE)
    }))
}
```

---

### Pitfall 6: LED Drive Rate Not Capped
**What goes wrong:** If the LED drive loop runs at the audio callback rate (~44fps), the keyboard USB bus is saturated. `apply_mono_color` takes ~15ms (10 USB transfers); at 44fps that's 660ms of USB time per second — physically impossible. Commands queue up, latency blows out.

**How to avoid:** The LED drive loop must sleep to maintain 30fps (33ms interval):
```rust
loop {
    let frame_start = Instant::now();
    // ... read bands, compute color, send UsbCmd ...
    let elapsed = frame_start.elapsed();
    if elapsed < FRAME_BUDGET {
        thread::sleep(FRAME_BUDGET - elapsed);
    }
}
const FRAME_BUDGET: Duration = Duration::from_millis(33);
```

---

### Pitfall 7: UsbCmd Queue Overflow During Audio Sync
**What goes wrong:** The existing `UsbCmd` channel is unbounded (`mpsc::channel`). During audio sync, if the LED drive loop sends faster than the USB worker processes, the channel grows without bound.

**How to avoid:** Use `mpsc::sync_channel(2)` for audio sync commands — size 2 buffer acts as a "latest value" window. If the worker falls behind, `try_send` drops stale frames instead of queuing them. Alternatively, check channel capacity before sending.

---

## Code Examples

### Device Enumeration for TUI Picker
```rust
// Source: RustAudio/cpal v0.17.3 examples/enumerate.rs (verified via GitHub raw)
use cpal::traits::{DeviceTrait, HostTrait};

pub fn list_input_devices() -> Vec<(String, String)> {
    let host = cpal::default_host();
    host.devices()
        .unwrap_or_else(|_| Box::new(std::iter::empty()))
        .filter_map(|dev| {
            // Only include devices that support input configs
            if dev.supported_input_configs().map(|c| c.count() > 0).unwrap_or(false) {
                let id = dev.id().ok()?.to_string();
                let desc = dev.description().unwrap_or_else(|_| id.clone());
                Some((id, desc))
            } else {
                None
            }
        })
        .collect()
}
```

### HSV → RGB (Inline, No Crate)
```rust
// Source: [ASSUMED] standard algorithm, verified by inspection
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - libm::fabsf(libm::fmodf(h / 60.0, 2.0) - 1.0));
    let m = v - c;
    let (r1, g1, b1) = match h as u32 {
        0..=59   => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _        => (c, 0.0, x),
    };
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}
```

### Extending UsbCmd for Audio Sync
```rust
// Extend the existing enum in tui.rs — add two new variants
enum UsbCmd {
    // ... existing variants ...
    AudioColor { r: u8, g: u8, b: u8, brightness: u8 },  // → apply_mono_color (10 transfers)
    AudioBrightness(u8),                                   // → set_brightness (1 transfer, fast)
}

// In spawn_usb_worker match arm:
UsbCmd::AudioColor { r, g, b, brightness } => {
    d.apply_mono_color(r, g, b, brightness, false)  // save=false, ALWAYS
}
UsbCmd::AudioBrightness(level) => {
    d.set_brightness(level)
}
```

### Restore Previous LED Config on Disable
```rust
// Capture state before enabling audio sync
pub struct LedSnapshot {
    r: u8, g: u8, b: u8,
    brightness: u8,
    // Captured from AppState fields
}

impl AppState {
    fn enable_audio_sync(&mut self) {
        // Save current LED state
        let snap = LedSnapshot {
            r: self.current_r,
            g: self.current_g,
            b: self.current_b,
            brightness: self.brightness,
        };
        self.audio_snapshot = Some(snap);
        // Start audio engine...
    }

    fn disable_audio_sync(&mut self) {
        if let Some(snap) = self.audio_snapshot.take() {
            let _ = self.usb_tx.send(UsbCmd::MonoColor {
                r: snap.r, g: snap.g, b: snap.b,
                brightness: snap.brightness, save: false,
            });
        }
    }
}
```

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| PipeWire | Audio capture | ✓ | active (user session) | ALSA direct hw |
| `libasound2t64` (runtime) | cpal linking | ✓ | 1.2.11 | — |
| `libasound2-dev` (headers) | cpal build | ✗ | — | **None — blocks build** |
| `pkg-config` | alsa-sys build | ✗ | — | **None — blocks build** |
| Rust 1.82+ | cpal ALSA requirement | ✓ | 1.94.1 | — |
| `/run/user/1000/pipewire-0` | Audio when root | ✓ | present | Set via PKEXEC_UID |
| `/dev/snd/*` | Direct ALSA hw | ✓ (ACL) | — | — |

**Missing dependencies with no fallback (MUST install before Wave 0 task 1):**
- `libasound2-dev` — `sudo apt install libasound2-dev pkg-config`

**Missing dependencies with fallback:**
- None identified.

---

## Codebase Integration Points (READ: critical for planner)

### Exact Changes Required in `src/ui/tui.rs`

1. **`Screen` enum** — add `AudioSync` and `AudioDevice` variants
2. **`AppState` struct** — add:
   - `audio_tx: Option<mpsc::Sender<AudioCmd>>`
   - `audio_enabled: bool`
   - `audio_effect: AudioEffect`
   - `audio_devices: Vec<(String, String)>` (populated lazily on AudioSync screen visit)
   - `audio_snapshot: Option<LedSnapshot>`
3. **`item_count()`** — add arms for `AudioSync` and `AudioDevice`
4. **`confirm()`** — add arms for `AudioSync` and `AudioDevice`
5. **`go_back()`** — add arms returning to correct previous screens
6. **`screen_title()`** — add title strings
7. **`build_list_items()`** — add list construction for both screens
8. **`render()`** — `AudioSync` can use existing list layout, add mini spectrum visualizer (optional)
9. **`main_items_dynamic()`** — add `"🎵 Audio Sync"` item and separator
10. **`UsbCmd` enum** — add `AudioColor` and `AudioBrightness` variants
11. **`spawn_usb_worker()`** — add match arms for new variants
12. **`run()`** — call `spawn_audio_engine()`, pass cloned `usb_tx` and `lb_path`

### New File: `src/audio/mod.rs`
Contains: `AudioCmd`, `BandAmplitudes`, `LedSnapshot`, `AudioEffect`, `spawn_audio_engine`

### New File: `src/audio/capture.rs`
Contains: `setup_audio_env_for_root()`, `build_input_stream()`, `list_input_devices()`

### New File: `src/audio/fft.rs`
Contains: `FFT` static, `process_fft()`, `smooth_bands()`, band constants

### New File: `src/audio/effects.rs`
Contains: `AudioEffectState`, `LedOutput`, `render_effect()`, `hsv_to_rgb()`

### `src/lib.rs` — add `pub mod audio;`

### `aucc-rs/Cargo.toml` — add `cpal = "0.17.3"` and `rustfft = "6.4.1"`

---

## State of the Art

| Old Approach | Current Approach | Notes |
|--------------|------------------|-------|
| ALSA direct on Linux | PipeWire + ALSA compat layer | PipeWire is default on Ubuntu 22.04+ |
| `cpalv0.15` ALSA only | `cpal 0.17.0+` adds native PipeWire/PulseAudio hosts | In practice still ALSA on Linux, but PipeWire ALSA is seamless |
| FftPlanner per-frame | Plan once, store in OnceLock | rustfft docs explicitly recommend this |

**Note on "native PipeWire in cpal":** The exploration notes say cpal has "native PipeWire" support. This is partially accurate. `cpal 0.17.x` on Linux still uses ALSA (`alsa` crate dependency), but the default ALSA device is routed through PipeWire's ALSA compatibility layer. There is **no separate PipeWire host module** in cpal 0.17.3 — the available Linux hosts are ALSA and (optionally) JACK. [VERIFIED: github.com/RustAudio/cpal tree listing]

---

## Open Questions

1. **Monitor source auto-detection**
   - What we know: `host.devices()` enumerates all ALSA devices including PipeWire virtual nodes; descriptions contain "Monitor" or similar when XDG_RUNTIME_DIR is set correctly
   - What's unclear: Exact string to match for monitor sources varies by PipeWire version and locale
   - Recommendation: Present all input devices to user; add "Monitor" detection heuristic as convenience default but don't rely on it

2. **AppState `current_r/g/b` fields**
   - What we know: `AppState` tracks `color_a` (index into `COLOR_NAMES`) and `brightness`, but not the actual current r/g/b values applied to the hardware at the moment
   - What's unclear: When audio sync restores the previous state, it needs actual RGB values — need to store them explicitly
   - Recommendation: Add `current_r: u8, current_g: u8, current_b: u8` to `AppState` and update them whenever `MonoColor` is sent

3. **Lightbar + keyboard simultaneous update in LED drive loop**
   - What we know: Both are routed through `UsbCmd` → USB worker; lightbar is instantaneous (HID ioctl), keyboard takes 15ms
   - What's unclear: Whether sending both in the same 33ms frame causes issues
   - Recommendation: Send `AudioBrightness` (fast, 1 transfer) for keyboard + `LbColor` for lightbar in same loop iteration; total should fit in 33ms budget

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Hann windowing formula is standard and correct | FFT Pattern code | Spectral leakage; would need to correct formula |
| A2 | HSV→RGB formula above is correct | Code Examples | Wrong colors displayed; unit test catches immediately |
| A3 | Setting `XDG_RUNTIME_DIR` before `cpal::default_host()` is sufficient for PipeWire ALSA plugin | Pitfall 1 | cpal still can't enumerate PipeWire sources; fallback is raw ALSA |
| A4 | ATTACK=0.6, RELEASE=0.15 smoothing constants are "visually good" | Smoothing Pattern | Effects too sluggish or too flickery; tunable at runtime |
| A5 | `pkg-config` is unavailable and must be installed along with `libasound2-dev` | Environment | May already be installed; check with `which pkg-config` |

---

## Security Domain

> `security_enforcement` not configured → treating as enabled.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | n/a |
| V3 Session Management | no | n/a |
| V4 Access Control | yes | Process already runs as root; audio thread should not spawn external processes |
| V5 Input Validation | yes | Device ID string from TUI must be validated before passing to `cpal::DeviceId::parse()` |
| V6 Cryptography | no | n/a |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Audio device ID injection | Tampering | Use `cpal::DeviceId::parse()` which validates format; don't pass raw strings to shell |
| Path traversal via PKEXEC_UID | Elevation of Privilege | Validate `PKEXEC_UID` is a non-negative integer before constructing paths |
| XRUN panic crashing root process | Denial of Service | cpal error callback must not panic; use `eprintln!` + graceful degradation |

---

## Validation Architecture

> `workflow.nyquist_validation` not configured → treating as enabled.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`cargo test`) |
| Config file | none — use `#[cfg(test)]` modules |
| Quick run command | `cd aucc-rs && cargo test audio` |
| Full suite command | `cd aucc-rs && cargo test` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-01 | Audio device list is non-empty when PipeWire running | integration | `cargo test audio::tests::list_devices_nonempty` | ❌ Wave 0 |
| REQ-02 | FFT band extraction returns values in [0.0, 1.0] | unit | `cargo test audio::fft::tests::bands_in_range` | ❌ Wave 0 |
| REQ-03 | Smoothing converges: target=1.0, after 10 frames value ≥ 0.9 | unit | `cargo test audio::fft::tests::smoothing_converges` | ❌ Wave 0 |
| REQ-04 | HSV→RGB: hue=0 → red, hue=120 → green, hue=240 → blue | unit | `cargo test audio::effects::tests::hsv_rgb_roundtrip` | ❌ Wave 0 |
| REQ-05 | AudioEffect::Random cycles through all 4 effects | unit | `cargo test audio::effects::tests::random_cycles` | ❌ Wave 0 |
| REQ-06 | setup_audio_env_for_root sets XDG_RUNTIME_DIR from PKEXEC_UID | unit | `cargo test audio::capture::tests::env_setup` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cd aucc-rs && cargo test audio 2>&1 | tail -10`
- **Per wave merge:** `cd aucc-rs && cargo test 2>&1 | tail -20`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `aucc-rs/src/audio/mod.rs` — module skeleton
- [ ] `aucc-rs/src/audio/fft.rs` — `#[cfg(test)] mod tests` with REQ-02, REQ-03
- [ ] `aucc-rs/src/audio/effects.rs` — `#[cfg(test)] mod tests` with REQ-04, REQ-05
- [ ] `aucc-rs/src/audio/capture.rs` — `#[cfg(test)] mod tests` with REQ-01, REQ-06
- [ ] System package: `sudo apt install libasound2-dev pkg-config`

---

## Sources

### Primary (HIGH confidence)
- `aucc-rs/src/ui/tui.rs` — full read; TUI architecture, event loop, UsbCmd pattern
- `aucc-rs/src/keyboard/mod.rs` — USB API, transfer counts, timing
- `aucc-rs/src/keyboard/effects.rs` — Effect enum, payload format
- `aucc-rs/src/lightbar/mod.rs` — HID ioctl API
- `aucc-rs/src/bin/aucc-ui.rs` — `require_root()` enforcement, pkexec usage
- `install/org.avell.aucc.policy` — confirmed pkexec is the launch method
- `github.com/RustAudio/cpal` Cargo.toml @ v0.17.3 — feature flags, Linux deps (alsa 0.11)
- `github.com/RustAudio/cpal` host/mod.rs directory listing @ v0.17.3 — confirmed no pipewire host module
- `github.com/RustAudio/cpal` README @ v0.17.3 — Linux = ALSA or JACK
- `github.com/RustAudio/cpal` examples/enumerate.rs + record_wav.rs — API patterns
- `github.com/ejmahler/RustFFT` README @ master — usage pattern, OnceLock recommendation
- `crates.io` API — version verification for cpal 0.17.3, rustfft 6.4.1, libm 0.2.16
- `pkexec env` executed on target machine — confirmed PKEXEC_UID=1000 is set
- Local `cargo build` attempt with cpal — confirmed `libasound2-dev` is required and missing

### Secondary (MEDIUM confidence)
- PipeWire ALSA plugin behavior (XDG_RUNTIME_DIR) — from local `arecord -L` output showing `pipewire` in ALSA device list + `/run/user/1000/pipewire-0` socket presence

### Tertiary (LOW confidence)
- ATTACK/RELEASE smoothing constants (0.6/0.15) — [ASSUMED] from common practice in audio visualizer implementations; verify perceptually during development

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — crates.io verified, build tested locally
- Architecture: HIGH — based on direct codebase read of existing patterns
- Pitfall 1 (root/audio): HIGH — verified pkexec sets PKEXEC_UID on this machine, PipeWire socket confirmed
- Pitfall 2 (libasound2-dev): HIGH — build failure reproduced locally
- Smoothing constants: LOW — assumed from training knowledge

**Research date:** 2026-04-15
**Valid until:** 2026-05-15 (cpal/rustfft are stable; PipeWire behavior is stable)
