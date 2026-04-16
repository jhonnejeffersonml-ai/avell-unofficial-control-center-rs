pub mod capture;
pub mod effects;
pub mod fft;

use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use crate::ui::tui::UsbCmd;

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

/// Target frame time for the LED drive loop.
pub const FRAME_BUDGET: Duration = Duration::from_millis(33); // ~30 fps

/// Spawn the audio engine in a background thread.
///
/// The engine receives [`AudioCmd`] from the TUI and drives LEDs by sending
/// [`UsbCmd::AudioColor`] / [`UsbCmd::AudioBrightness`] **directly** to the
/// USB worker (`usb_tx`) — bypassing the TUI event loop (anti-pattern guard).
///
/// `lb_path`: lightbar hidraw path, forwarded to the color sender.
pub fn spawn_audio_engine(
    audio_rx: mpsc::Receiver<AudioCmd>,
    usb_tx: mpsc::Sender<UsbCmd>,
    lb_path: Option<PathBuf>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // T-01-01: MUST be first — sets XDG_RUNTIME_DIR before cpal init.
        capture::setup_audio_env_for_root();

        let bands = Arc::new(Mutex::new(BandAmplitudes::default()));

        // Holds the active cpal stream. Dropping it stops capture.
        let mut active_stream: Option<cpal::Stream> = None;
        let mut effect_state = effects::AudioEffectState::new(effects::AudioEffect::Pulse);

        'outer: loop {
            // Wait for the first (or next) command.
            let cmd = match audio_rx.recv() {
                Ok(c) => c,
                Err(_) => break 'outer, // sender dropped — TUI exited
            };

            match cmd {
                AudioCmd::Enable { device_name, effect } => {
                    // (Re)start capture stream
                    active_stream = None; // drop previous stream if any
                    let bands_clone = Arc::clone(&bands);
                    match capture::build_input_stream(
                        device_name.as_deref(),
                        bands_clone,
                    ) {
                        Ok(stream) => active_stream = Some(stream),
                        Err(e) => {
                            eprintln!("Audio capture error: {e}");
                            continue;
                        }
                    }
                    effect_state = effects::AudioEffectState::new(effect);

                    // LED drive sub-loop — runs until Disable or channel close.
                    'drive: loop {
                        let frame_start = Instant::now();

                        // Check for incoming commands without blocking.
                        match audio_rx.try_recv() {
                            Ok(AudioCmd::Disable) => {
                                active_stream = None;
                                break 'drive;
                            }
                            Ok(AudioCmd::SetEffect(new_effect)) => {
                                effect_state = effects::AudioEffectState::new(new_effect);
                            }
                            Ok(AudioCmd::Enable { device_name: dev, effect: eff }) => {
                                // Re-enable: restart stream with new params
                                active_stream = None;
                                let b2 = Arc::clone(&bands);
                                match capture::build_input_stream(dev.as_deref(), b2) {
                                    Ok(s) => active_stream = Some(s),
                                    Err(e) => eprintln!("Audio restart error: {e}"),
                                }
                                effect_state = effects::AudioEffectState::new(eff);
                            }
                            Err(mpsc::TryRecvError::Disconnected) => break 'outer,
                            Err(mpsc::TryRecvError::Empty) => {} // nothing pending
                        }

                        // Render frame
                        let current_bands = bands.lock()
                            .map(|b| b.clone())
                            .unwrap_or_default();

                        let led_out = effect_state.render(&current_bands);
                        effect_state.tick(1.0 / 30.0);

                        send_led_output(&led_out, &usb_tx, lb_path.as_ref());

                        // Sleep for the remaining frame budget
                        let elapsed = frame_start.elapsed();
                        if elapsed < FRAME_BUDGET {
                            thread::sleep(FRAME_BUDGET - elapsed);
                        }
                    }
                }
                AudioCmd::Disable | AudioCmd::SetEffect(_) => {
                    // No-op when engine is idle
                }
            }
        }

        // Clean up — stop capture on exit
        drop(active_stream);
    })
}

/// Send a single [`LedOutput`] frame to the USB worker and optionally lightbar.
fn send_led_output(
    output: &LedOutput,
    usb_tx: &mpsc::Sender<UsbCmd>,
    lb_path: Option<&PathBuf>,
) {
    match *output {
        LedOutput::Brightness(level) => {
            let _ = usb_tx.send(UsbCmd::AudioBrightness(level));
        }
        LedOutput::Color { r, g, b, brightness } => {
            let _ = usb_tx.send(UsbCmd::AudioColor { r, g, b, brightness });
            if let Some(path) = lb_path {
                let _ = usb_tx.send(UsbCmd::LbColor {
                    path: path.clone(),
                    r, g, b,
                    brightness,
                });
            }
        }
    }
}
