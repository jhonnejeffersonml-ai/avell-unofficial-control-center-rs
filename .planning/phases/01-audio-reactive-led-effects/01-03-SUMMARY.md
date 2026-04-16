# Summary: Plan 03 — TUI Integration

**Status:** COMPLETE  
**Commit:** 2c7b2a6

## What was built

### `tui.rs` changes
- `UsbCmd::AudioColor { r, g, b, brightness }` — handled by `apply_mono_color(..., save: false)`
- `UsbCmd::AudioBrightness(u8)` — handled by `set_brightness()`
- `Screen::AudioSync` + `Screen::AudioDevice` — new navigation screens
- `AppState` fields: `audio_tx`, `audio_enabled`, `audio_effect`, `audio_devices`, `audio_device_idx`, `audio_snapshot`
- `LedSnapshot` struct — captures r/g/b/brightness before audio sync for restore
- `enable_audio_sync()` — snapshots state, spawns audio engine, sends `Enable` cmd
- `disable_audio_sync()` — sends `Disable`, restores LED state from snapshot
- `confirm()` arms for `AudioSync` (effect select, device picker, toggle) and `AudioDevice`
- `go_back()` routing: AudioSync→Main, AudioDevice→AudioSync
- `item_count()` for AudioSync (9) and AudioDevice (device list size)
- `build_list_items()` for both screens (effects with ✔ marker, device list)
- Separator skip in `move_cursor()` for AudioSync (indices 5, 7)
- `"🎵 Audio Sync"` entry in `main_items_dynamic()`
- Cleanup: `disable_audio_sync()` called before terminal restore on exit
- `screen_title()` for both new screens in Portuguese

## Tests: 29/29 passing, cargo check: 0 errors
