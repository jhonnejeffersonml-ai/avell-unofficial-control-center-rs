# Summary: Plan 02 — cpal Capture + Audio Engine Pipeline

**Status:** COMPLETE  
**Commit:** 2c7b2a6

## What was built

### `capture.rs`
- `setup_audio_env_for_root()` — translates `PKEXEC_UID` → `XDG_RUNTIME_DIR` before cpal init (anti-pattern guard T-01-01)
- `list_input_devices()` — enumerates cpal input devices via `host.devices()`
- `build_input_stream(device_name, bands)` — builds cpal input stream, mixes to mono, feeds FFT, updates shared `Arc<Mutex<BandAmplitudes>>`

### `mod.rs`
- `FRAME_BUDGET: Duration` — 33ms constant for 30fps LED drive
- `spawn_audio_engine(audio_rx, usb_tx, lb_path)` — background thread with:
  - Setup-first pattern: `setup_audio_env_for_root()` as first line
  - Inner LED drive loop (~30fps): renders effect, sends `UsbCmd::AudioColor`/`AudioBrightness` directly to USB worker
  - Command handling: `Enable`, `Disable`, `SetEffect` via `try_recv()` in drive loop
- `send_led_output()` — maps `LedOutput` → `UsbCmd`, forwards to lightbar if path present

## Tests: 29/29 passing (2 new in capture::tests)
