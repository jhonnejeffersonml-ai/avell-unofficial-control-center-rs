# Phase 1: Audio-reactive LED effects — Context

**Gathered:** 2026-04-15
**Status:** Ready for planning
**Source:** gsd-explore session

<domain>
## Phase Boundary

Implementar sincronização em tempo real dos LEDs do teclado e da lightbar com o áudio do sistema.

Entregáveis desta fase:
- Módulo `aucc_rs::audio` para captura de áudio e análise FFT
- Sistema de efeitos audio-reativos com pelo menos 4 efeitos + modo randômico
- Nova aba "Audio Sync" no TUI (`aucc-ui`)
- Thread de áudio desacoplada do loop do TUI
- Seleção de fonte de áudio pelo usuário

</domain>

<decisions>
## Implementation Decisions

### Análise de áudio
- **FFT por bandas de frequência** (não amplitude simples)
- 3 bandas: grave / médio / agudo → cada uma mapeada para uma cor diferente
- Buffer: 1024 amostras @ 44.1kHz ≈ 23ms por frame
- Crate: `cpal` 0.17.3 (PipeWire + PulseAudio nativos)
- FFT: `rustfft`

### Fonte de áudio
- **Configurável pelo usuário** — não hard-coded
- Opções: saída do sistema (`.monitor`), microfone, seleção de app
- Enumeração via `cpal` (`.monitor` aparece como input device no PulseAudio/PipeWire)
- Seleção persistida na configuração

### Efeitos
- Múltiplos efeitos selecionáveis pelo usuário:
  - **Pulse** — brilho pulsa conforme amplitude geral (beat)
  - **Color-shift** — cor muda conforme banda dominante (grave/médio/agudo)
  - **Wave** — onda percorre o teclado no ritmo
  - **Breathe** — respiração suave sincronizada
  - **Random** — cicla efeitos aleatoriamente
- Sistema extensível (seguir padrão do `Effect` enum existente)

### Hardware
- **Lightbar** (`048d:7001`): HID ioctl `apply_color()` — ideal (latência mínima)
- **Teclado ITE 8291** (`048d:600b`):
  - `set_brightness()` para pulsação rápida (1 USB transfer)
  - `apply_mono_color()` para troca de cor (10 transfers, ~15ms)
- Taxa alvo: **30fps** (33ms budget: ~15ms USB + ~18ms FFT)
- Lightbar e teclado atualizados **simultaneamente**

### Integração TUI
- Nova aba no TUI existente (`ratatui`-based `aucc-ui`)
- Thread dedicada de áudio (não bloqueia o loop de eventos do TUI)
- Ao desativar o modo, restaura a configuração anterior de LEDs

### the agent's Discretion
- Arquitetura interna do módulo `audio` (structs, traits)
- Estratégia de smoothing/lerp para evitar flickering
- Mapeamento exato das frequências para bandas (Hz boundaries)
- Mecanismo de comunicação entre thread de áudio e TUI (channel vs Arc<Mutex>)
- Layout visual da aba no TUI

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Hardware control
- `aucc-rs/src/lightbar/mod.rs` — `apply_color()`, `find_hidraw_path()`, HID ioctl
- `aucc-rs/src/keyboard/mod.rs` — `apply_mono_color()`, `set_brightness()`, USB transfers
- `aucc-rs/src/keyboard/effects.rs` — `Effect` enum pattern to follow

### TUI architecture
- `aucc-rs/src/ui/tui.rs` — TUI event loop, tab structure, rendering
- `aucc-rs/src/ui/mod.rs` — UI module organization

### Project config
- `aucc-rs/Cargo.toml` — existing dependencies, crate versions
- `aucc-rs/src/config.rs` — configuration persistence pattern
- `aucc-rs/src/lib.rs` — module structure

### Exploration notes
- `.planning/notes/audio-reactive-leds-exploration.md` — research findings from exploration session

</canonical_refs>

<specifics>
## Specific Ideas

- Grave → vermelho/laranja, Médio → verde/amarelo, Agudo → azul/roxo (ponto de partida, configurável futuramente)
- `ite8291r3-ctl` (Python) e OpenRGB confirmam 30fps+ no ITE 8291 com mesmo padrão de transfers
- `cpal` 0.17.0+ changelog confirma: PipeWire host nativo, `device_by_id`, `.monitor` como input device

</specifics>

<deferred>
## Deferred Ideas

- Configuração de cores por banda (o usuário escolhe o mapeamento) — fase futura
- Detecção de BPM para sincronização de tempo absoluto — fase futura
- Per-key effects (iluminar teclas individuais no ritmo) — requer `UserMode` e payload por tecla
- Visualizador de espectro no TUI dashboard — pode ser adicionado ao dashboard existente futuramente

</deferred>

---

*Phase: 01-audio-reactive-led-effects*
*Context gathered: 2026-04-15 via gsd-explore*
