---
title: "Audio-reactive LED effects — exploração técnica"
date: 2026-04-15
context: gsd-explore session
---

## Resumo

Investigação de viabilidade para efeito rítmico nos LEDs do teclado e lightbar sincronizado com áudio.

## Decisões Técnicas

### Hardware

- **Lightbar** (`048d:7001`): `apply_color(r, g, b, brightness)` via HID `ioctl` — latência quase zero, ideal para o efeito principal
- **Teclado ITE 8291** (`048d:600b`): `apply_mono_color` = 10 USB transfers (~10-15ms); `set_brightness` = 1 transfer (mais rápido)
- **Taxa sustentável**: 30fps (frame budget de 33ms: ~15ms USB + ~18ms FFT/processamento)

### Áudio

- **Crate escolhida**: `cpal` 0.17.3
  - PipeWire + PulseAudio nativos no Linux
  - Enumera fontes incluindo `.monitor` de saída (loopback)
  - Seleção de fonte por ID estável (`device_by_id`)
- **Buffer**: 1024 amostras @ 44.1kHz ≈ 23ms — adequado para FFT
- **Análise**: FFT por bandas de frequência
  - Grave → cor A (ex: vermelho/laranja)
  - Médio → cor B (ex: verde/amarelo)
  - Agudo → cor C (ex: azul/roxo)

### Efeitos planejados

- **Pulse** — brilho pulsa conforme amplitude geral (beat detection)
- **Color-shift** — cor muda conforme banda dominante
- **Wave** — onda percorre o teclado no ritmo
- **Breathe** — respiração suave sincronizada com o BPM
- **Random** — cicla efeitos aleatoriamente

### Integração

- Nova aba/menu dentro do TUI (`aucc-ui`)
- Fonte de áudio configurável pelo usuário (não hard-coded)
- Thread dedicada para o loop de áudio (não bloqueia o TUI)

## Referências

- `ite8291r3-ctl` (Python) e OpenRGB: confirmam 30fps+ no ITE 8291
- `cpal` changelog: v0.17.0 adicionou hosts nativos PipeWire/PulseAudio
- FFT crate sugerida: `rustfft`
