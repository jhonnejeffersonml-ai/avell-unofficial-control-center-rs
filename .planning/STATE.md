---
gsd_state_version: 1.0
milestone: v3.0.0
milestone_name: "### Phase 1: Audio-reactive LED effects"
status: executing
stopped_at: Planning complete — paused before executing Wave 1
last_updated: "2026-04-16T00:06:48.523Z"
progress:
  total_phases: 1
  completed_phases: 0
  total_plans: 3
  completed_plans: 0
  percent: 0
---

# STATE.md — avell-unofficial-control-center-rs

> Reconstructed on 2026-04-16T00:02:43.484Z from ROADMAP.md, HANDOFF.json, and git history.

## Project Reference

**What This Is:** Linux userspace driver + control tool for RGB LED keyboards and front LED lightbars on Avell gaming laptops. Controls ITE 8291 keyboard (`048d:600b`) and ITE 8233 lightbar (`048d:7001`) via USB HID, with a full ratatui TUI and CLI.

**Core Value:** Let Avell laptop users control their RGB LEDs on Linux without Windows software.

**Current Milestone:** v3.0.0

## Current Position

Phase: 01 (Audio-reactive LED effects) — EXECUTING
Plan: 1 of 3

- **Phase:** 1 of 1 — Audio-reactive LED effects
- **Plan:** 0 of 3 — Not started (all plans verified, ready for execution)
- **Status:** Executing Phase 01

```
Progress: [░░░░░░░░░░] 0%
```

## Session Continuity

Last session: 2026-04-15T20:43:40Z  
Stopped at: Planning complete — paused before executing Wave 1  
Resume file: `.planning/phases/01-audio-reactive-led-effects/.continue-here.md`  
Handoff: `.planning/HANDOFF.json`

## Recent Decisions

- **cpal 0.17.3** via ALSA host (PipeWire compat layer) — not PipeWire native
- **LED drive → usb_tx direct** — bypasses TUI 50ms poll, achieves ≤33ms LED latency
- **30fps** (33ms budget) — ITE 8291 sustains ~30fps (10 USB transfers ~15ms)
- **save: false mandatory** on all audio LED writes — 30fps EEPROM writes destroy hardware
- **setup_audio_env_for_root()** before any cpal call — pkexec resets XDG_RUNTIME_DIR
- **5 effects:** Pulse, ColorShift, Wave, Breathe, Random (auto-cycles at 8s)

## Blockers / Concerns

- ⚠️ **HUMAN ACTION REQUIRED:** `sudo apt install libasound2-dev pkg-config` — must run before any `cargo build`

## Pending Todos

_(none captured)_
