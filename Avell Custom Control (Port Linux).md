# Arquitetura — AUCC-RS

**Avell Unofficial Control Center** — versão Rust com CLI + TUI interativa.

---

## Visão Geral

AUCC-RS é um driver userspace em Rust que controla o teclado RGB (ITE 8291) e a lightbar frontal (ITE 8233) de laptops Avell/TongFang. Fornece duas interfaces:

1. **CLI** (`aucc`) — linha de comando para scripts e uso direto
2. **TUI** (`aucc-ui`) — interface interativa em terminal com ratatui/crossterm

---

## Arquitetura do Sistema

```
┌──────────────────────────────────────────────┐
│              aucc-ui (TUI)                    │
│         ratatui + crossterm                   │
│         Thread principal: render + input      │
│         Thread USB: mpsc channel → device     │
└──────────────────┬───────────────────────────┘
                   │
           mpsc::Sender<UsbCmd>
                   │
┌──────────────────▼───────────────────────────┐
│           KeyboardDevice (rusb)               │
│         USB HID: 048d:600b (ITE 8291)         │
│         ctrl_transfer + bulk_write            │
└──────────────────────────────────────────────┘

┌──────────────────────────────────────────────┐
│              aucc (CLI)                       │
│         clap argument parser                  │
│         → KeyboardDevice / Lightbar / Power   │
└──────────────────────────────────────────────┘
```

### Separação de Privilégios

| Função | Precisa de root? | Motivo |
|---|---|---|
| Teclado RGB | ❌ Não¹ | udev rules + grupo `plugdev` |
| Lightbar | ❌ Não¹ | hidraw via udev + `plugdev` |
| Perfis de energia (RAPL) | ✅ Sim | escrita em `/sys/class/powercap` |
| Governor/EPP | ✅ Sim | escrita em `/sys/devices/system/cpu` |
| Telemetria | ❌ Não | leitura de sysfs/hwmon |

¹ Requer udev rules instaladas (`sudo aucc --install`).

---

## Estrutura de Módulos

```
aucc-rs/src/
├── lib.rs              # Library: re-exports todos os módulos
├── main.rs             # Binary: CLI (aucc)
├── config.rs           # Persistência: /etc/aucc/lightbar.conf
├── setup.rs            # Install/uninstall: udev rules, polkit
├── bin/
│   └── aucc-ui.rs      # Binary: TUI launcher (--install/--uninstall)
├── keyboard/
│   ├── mod.rs          # KeyboardDevice: rusb USB HID
│   ├── colors.rs       # 20 cores RGB + payloads (64 bytes)
│   └── effects.rs      # 11 efeitos + wave direction + reactive flag
├── lightbar/
│   └── mod.rs          # ITE 8233: hidraw + HIDIOCSFEATURE ioctl
├── power/
│   └── mod.rs          # RAPL PL1/PL2 + governor + EPP
├── telemetry/
│   └── mod.rs          # CPU (coretemp), GPU (nvidia-smi), RAM, NVMe, BAT
└── ui/
    ├── mod.rs          # Re-export
    └── tui.rs          # ratatui: navegação, render, USB worker thread
```

---

## Protocolo de Hardware

### Teclado (ITE 8291 / `048d:600b`)

Efeitos usam control transfer de 8 bytes:

```
byte 0: 0x08  (command flag)
byte 1: 0x02  (enable) | 0x01 (disable)
byte 2: effect code (0x02–0x11, ou 0x33 para per-key)
byte 3: speed (0x01–0x0A)
byte 4: brightness (0x08 / 0x16 / 0x24 / 0x32)
byte 5: color index (0x01–0x07, ou 0x00/0x08 para rainbow)
byte 6: direction/modifier (wave: right=1, left=2, up=3, down=4; reactive: 0x01)
byte 7: save to EEPROM (0x00 = no, 0x01 = yes)
```

**Nota sobre modo reativo:** Os efeitos "Reactive" e "ReactiveAurora" não possuem
códigos próprios — compartilham `0x04` (com Random) e `0x0E` (com Aurora).
A diferenciação é feita pelo **byte 6** (`modifier`): `0x00` = normal, `0x01` = reativo.

Per-key RGB (user mode `0x33`) envia 8 × bulk transfers de 64 bytes, cada um
codificando 16 slots de teclas como `[0x00, R, G, B]`.

### Lightbar (ITE 8233 / `048d:7001`)

Controlada via `/dev/hidrawN` com ioctl `HIDIOCSFEATURE`:

```
[0x00, 0x14, 0x00, 0x01, R, G, B, 0x00, 0x00]   # set color
[0x00, 0x08, 0x22, 0x01, 0x01, brightness, 0x01, 0x00, 0x00]  # set brightness
```

A lightbar **não possui EEPROM** — o estado é salvo em software em
`/etc/aucc/lightbar.conf` e restaurado via regra udev no boot.

---

## TUI: Fluxo de Navegação

```
Main Menu
├── Dashboard (telemetria) → refresh auto a cada 1s
├── Perfis de Energia → Silent / Balanced / Turbo
├── Teclado — Cor Sólida → Cor A → Brilho
├── Teclado — Alternado H → Cor A → Cor B → Brilho
├── Teclado — Alternado V → Cor A → Cor B → Brilho
├── Teclado — Efeito → [Wave → Direção] → Reativo? → Cor → Brilho → Velocidade
├── Teclado — Desligar
├── Lightbar — Cor → Cor → Brilho
├── Lightbar — Igual ao teclado
├── Lightbar — Desligar
├── 💾 Persistir: SIM/NÃO
├── Instalar / Desinstalar
└── Sair
```

### Thread USB

A TUI usa uma thread dedicada (`spawn_usb_worker`) que recebe comandos via canal
`mpsc::Sender<UsbCmd>`. Isso evita bloquear o loop de render da UI durante
comunicação USB, que pode levar centenas de milissegundos.

**Live preview:** Ao navegar por listas de cores, a TUI envia comandos USB com
`save: false` para preview instantâneo no teclado. O save só ocorre ao confirmar
com Enter.

---

## Stack Tecnológico

| Camada | Tecnologia |
|---|---|
| **Linguagem** | Rust 2021 |
| **USB HID (teclado)** | `rusb` (libusb wrapper) |
| **HID raw (lightbar)** | `libc::ioctl` com `HIDIOCSFEATURE` + `nix` |
| **CLI** | `clap` (derive) |
| **TUI** | `ratatui` + `crossterm` |
| **Cores no terminal** | `colored` |
| **Power** | sysfs: `/sys/class/powercap/intel-rapl/` |
| **Telemetria** | hwmon (coretemp, nvme), `/proc/meminfo`, `nvidia-smi` |

---

## Testes

O projeto possui **65 testes unitários** cobrindo:

- `effects.rs` (23): códigos de efeito, payloads, wave directions, brightness
- `colors.rs` (9): get_color, payloads mono/H/V
- `config.rs` (9): load/save round-trip, edge cases
- `power.rs` (14): profile limits, from_str, governors
- `telemetry.rs` (10): parsing de RAM, GPU, CPU aggregation

```bash
cargo test  # 65 passed
```

---

## Instalação

```bash
sudo ./install/install.sh        # build from source
# ou
sudo ./aucc --install             # pre-built binary
```

### O que é instalado:
- Binários: `/usr/local/bin/aucc`, `/usr/local/bin/aucc-ui`
- udev rules: `/etc/udev/rules.d/70-avell-hid.rules`
- Config dir: `/etc/aucc/`
- Grupo: usuário adicionado a `plugdev`

---

*Documento gerado em abril de 2026 — reflete a arquitetura real v2.2.0.*
