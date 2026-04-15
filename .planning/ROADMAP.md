# ROADMAP

## Milestone: v3.0.0

### Phase 1 — Audio-reactive LED effects

**Goal:** Sincronizar LEDs do teclado e lightbar com o áudio do sistema em tempo real.

**Scope:**
- Captura de áudio via `cpal` 0.17.3 (PipeWire/PulseAudio, fonte configurável)
- Análise FFT por bandas (grave/médio/agudo) com `rustfft`
- Múltiplos efeitos visuais: `pulse`, `color-shift`, `wave`, `breathe`, `random`
- Aplicação em tempo real na lightbar (HID ioctl) e teclado ITE 8291 (USB)
- Nova aba de "Audio Sync" no TUI (`aucc-ui`)
- Thread dedicada de áudio desacoplada do loop do TUI

**Acceptance criteria:**
- [ ] Usuário pode selecionar fonte de áudio no TUI (saída do sistema, microfone, etc.)
- [ ] Pelo menos 4 efeitos visuais funcionando e selecionáveis
- [ ] Modo randômico cicla entre efeitos automaticamente
- [ ] LEDs respondem visivelmente ao áudio com latência ≤ 50ms
- [ ] Lightbar e teclado atualizados simultaneamente
- [ ] Desligar o modo para no TUI e restaura configuração anterior

**Technical notes:**
- Ver `.planning/notes/audio-reactive-leds-exploration.md`
- `apply_mono_color` sustenta ~30fps (10 USB transfers ~15ms)
- `set_brightness` pode ser usado para pulsação mais rápida (1 transfer)
- FFT com buffer de 1024 samples @ 44.1kHz ≈ 23ms de dados de frequência
