# avell-unofficial-control-center-rs

[![Gitter](https://badges.gitter.im/Unofficial-CC/Lobby.svg)](https://gitter.im/Unofficial-CC/Lobby?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)

> **This project is written in [Rust](https://www.rust-lang.org/)** 🦀, using `rusb`/`libusb` for USB HID communication and `ratatui` for the interactive terminal UI.

Linux userspace driver and control tool for RGB LED keyboards and front LED lightbars found in Avell gaming laptops and other devices using the **Integrated Technology Express ITE Device(8291) Rev 0.03** controller.

Controls both the keyboard backlight (`048d:600b`) and the front lightbar (`048d:7001`) via USB HID, with a full interactive terminal UI and a command-line interface.

## Hardware Support

| Device | USB ID | Interface | Status |
|---|---|---|---|
| ITE Device 8291 Rev 0.03 — RGB Keyboard | `048d:600b` | rusb / libusb | ✅ Full support |
| ITE Device 8233 — Front LED Lightbar | `048d:7001` | hidraw + HIDIOCSFEATURE | ✅ Full support |

> **ITE Device(8291) Rev 0.02?** See [Project StarBeat](https://github.com/kirainmoe/project-starbeat).

### Verify your keyboard controller

```bash
sudo hwinfo --short
# Expected output:
# keyboard:
#   Integrated Technology Express ITE Device(8291)
```

### Known compatible devices

The ITE Device(8291) controller is used in Tongfang gaming laptop barebones and their reseller variants:

- **Avell** Storm 470 Black, G1550 FOX, G1513 FOX-7, A65, A52 (BR)
- Tongfang GK5CN5Z / GK5CN6Z / GK5CQ7Z / GK5CP0Z (Barebone)
- Schenker XMG Neo 15 M18/M19 (DE)
- PCSpecialist Recoil II & III (UK)
- Scan/3XS LG15 Vengeance Pro (UK)
- Overpowered 15 / 15+ (US — Walmart)
- Monster Tulpar T5 (TR)
- MECHREVO Deep Sea Ghost Z2 (CN)
- Raionbook GS5 (IT)
- Illegear Onyx (MY)
- Hyperbook Pulsar Z15 (PL)
- Aftershock APEX 15 (SG)
- Origin-PC EON15-S (US/AU/NZ/Asia)
- Eluktronics Mech 15 G2 (US)
- HIDevolution EVOC 16GK5 (US)
- Obsidian GK5CP (PT)
- Vulcan JinGang GTX Standard

## Features

### Keyboard (ITE 8291 / `048d:600b`)

| Feature | CLI | TUI |
|---|---|---|
| Solid color (20 colors) | `aucc -c <color>` | ✅ |
| Horizontal alternating (2 colors) | `aucc -H <a> <b>` | ✅ |
| Vertical alternating (2 colors) | `aucc -V <a> <b>` | ✅ |
| Breathing effect | `aucc -s breathing[r\|o\|y\|g\|b\|t\|p]` | ✅ |
| Wave effect + **direction** | `aucc -s wave --direction right\|left\|up\|down` | ✅ |
| Rainbow, Marquee, Raindrop | `aucc -s rainbow\|marquee\|raindrop` | ✅ |
| Aurora, Ripple, Reactive | `aucc -s aurora\|ripple\|reactive` | ✅ |
| Fireworks, Reactive variants | `aucc -s fireworks\|reactiveripple\|reactiveaurora` | ✅ |
| Brightness (4 levels) | `aucc -b 1..4` | ✅ |
| Speed (10 levels) | `aucc --speed 1..10` | ✅ |
| **Save to EEPROM** (persist after reboot) | `aucc ... --save` | ✅ toggle |
| Disable keyboard backlight | `aucc -d` | ✅ |

### Lightbar (ITE 8233 / `048d:7001`)

| Feature | CLI | TUI |
|---|---|---|
| Solid RGB color (20 colors) | `aucc --lb-color <color>` | ✅ |
| Brightness (0–100%) | `aucc --lb-brightness <0-100>` | ✅ |
| **Sync color with keyboard** | — | ✅ one-tap |
| Disable lightbar | `aucc --lb-disable` | ✅ |
| **Persist state across reboots** | `aucc --lb-color … ` (auto-saved) | ✅ auto-saved |
| Restore saved state | `aucc --lb-restore` | — |

> The ITE 8233 lightbar hardware supports solid color only — no animations.
> Lightbar state is saved to `/etc/aucc/lightbar.conf` and restored automatically
> on boot via udev rule (activate with `sudo aucc --install`).

### Power & Telemetry

| Feature | CLI | TUI |
|---|---|---|
| Power profiles (Silent/Balanced/Turbo) | `aucc --profile silent\|balanced\|turbo` | ✅ |
| Manual TDP (PL1 in watts) | `aucc --tdp 45` | — |
| System telemetry (CPU/GPU/RAM/NVMe/Battery) | `aucc --telemetry` | ✅ dashboard |

## Installation

### Requirements

- `libusb-1.0` (system package)
- Rust toolchain (`cargo`) — installed automatically if missing

```bash
# Debian / Ubuntu / Linux Mint
sudo apt install libusb-1.0-0
```

### Install (recommended)

```bash
git clone https://github.com/rodgomesc/avell-unofficial-control-center.git
cd avell-unofficial-control-center
sudo ./install/install.sh
```

The install script:
1. Compiles `aucc` and `aucc-ui` with `cargo build --release`
2. Installs binaries to `/usr/local/bin/`
3. Installs udev rules (`/etc/udev/rules.d/70-avell-hid.rules`)
4. Installs polkit policy for passwordless root via `pkexec`
5. Creates `/etc/aucc/` for lightbar persistence config
6. Adds user to the `plugdev` group

### Install from pre-built binaries

Download `aucc` and `aucc-ui` from the [releases page](https://github.com/jhonnejeffersonml-ai/avell-unofficial-control-center-rs/releases) and run:

```bash
# Install aucc (copies binary to /usr/local/bin + installs udev rules)
sudo ./aucc --install

# Install aucc-ui (copies binary to /usr/local/bin)
sudo ./aucc-ui --install
```

To uninstall:

```bash
sudo aucc --uninstall      # removes udev rules + /usr/local/bin/aucc
sudo aucc-ui --uninstall   # removes /usr/local/bin/aucc-ui
```

### Enable lightbar boot restore

After installation, activate automatic lightbar state restore on boot:

```bash
sudo aucc --install
```

This writes the udev rule that runs `aucc --lb-restore` whenever the ITE 8233
device is detected (i.e., on every boot).

## Usage

Controlling USB HID devices requires root. The project includes a `teclado` launcher that uses `pkexec` to request elevated privileges automatically.

### Interactive TUI (recommended)

```bash
./teclado
```

Or, if installed system-wide:

```bash
pkexec aucc-ui
```

The TUI provides a full arrow-key menu with:
- **Live color preview** on the keyboard as you navigate color lists
- **Wave direction selection** (→ ← ↑ ↓) when choosing the wave effect
- **💾 Persistir toggle** in the main menu — enables EEPROM save for all subsequent operations
- **Lightbar sync** — applies the current keyboard color to the lightbar in one step
- **Power profile selection** — Silent / Balanced / Turbo
- **Telemetry dashboard** — CPU, GPU, RAM, NVMe, Battery in real-time

### CLI reference

All `aucc` commands for keyboard/power require root (`sudo aucc ...`).
Lightbar and telemetry commands work without root if the user is in the `plugdev` group.

Run `aucc --help` for the full reference with examples.

#### Solid color

```bash
sudo aucc -c green -b 4
```

Colors: `red`, `green`, `blue`, `teal`, `pink`, `purple`, `yellow`, `orange`, `white`, `olive`, `maroon`, `brown`, `gray`, `skyblue`, `navy`, `crimson`, `darkgreen`, `lightgreen`, `gold`, `violet`

#### Alternating colors

```bash
sudo aucc -H pink teal -b 3        # horizontal rows
sudo aucc -V red blue  -b 4        # vertical columns
```

#### Effects

```bash
sudo aucc -s rainbow
sudo aucc -s wave --direction left   # right (default), left, up, down
sudo aucc -s breathingr              # breathing red
sudo aucc -s rippleb -b 3 --speed 3  # ripple blue, speed 3
```

Effects: `rainbow`, `marquee`, `wave`, `raindrop`, `aurora`, `random`, `reactive`,
`breathing`, `ripple`, `reactiveripple`, `reactiveaurora`, `fireworks`

Color suffix (for supported effects): `r`=red, `o`=orange, `y`=yellow, `g`=green,
`b`=blue, `t`=teal, `p`=purple

#### Save to EEPROM (keyboard — persist after reboot)

```bash
sudo aucc -c white -b 4 --save
sudo aucc -s rainbow --save
```

Without `--save`, keyboard changes are temporary and reset on the next reboot.

#### Lightbar

```bash
aucc --lb-color white --lb-brightness 50   # set color and save
aucc --lb-disable                          # turn off and save
aucc --lb-restore                          # re-apply saved state
```

Lightbar state is **always saved automatically** when you use `--lb-color` or `--lb-disable`.

#### Power profiles

```bash
sudo aucc --profile silent     # 25W PL1 / 35W PL2
sudo aucc --profile balanced   # 45W / 65W
sudo aucc --profile turbo      # 80W / 120W
sudo aucc --tdp 45             # custom PL1 in watts
```

#### Telemetry (no root required)

```bash
aucc --telemetry
```

## Technical Notes

### Lightbar protocol (ITE 8233 / `048d:7001`)

The lightbar is controlled via `/dev/hidrawN` using the `HIDIOCSFEATURE` ioctl.

Two sequential feature reports control the lightbar:

```
[0x00, 0x14, 0x00, 0x01, R, G, B, 0x00, 0x00]   # set color
[0x00, 0x08, 0x22, 0x01, 0x01, brightness, 0x01, 0x00, 0x00]  # set brightness
```

If `/dev/hidraw2` is missing, the driver attempts to rebind `usbhid` to the device automatically.

### Keyboard protocol (ITE 8291 / `048d:600b`)

Effects use a single 8-byte `ctrl_transfer(0x21, 0x09, 0x300, 1, payload)`:

```
byte 0: 0x08  (command flag)
byte 1: 0x02  (enable) | 0x01 (disable)
byte 2: effect code (0x02–0x11, or 0x33 for user/per-key mode)
byte 3: speed (0x01–0x0A)
byte 4: brightness (0x08 / 0x16 / 0x24 / 0x32)
byte 5: color index (0x01–0x07, or 0x00/0x08 for rainbow)
byte 6: direction/modifier (wave: right=1, left=2, up=3, down=4)
byte 7: save to EEPROM (0x00 = no, 0x01 = yes)
```

Per-key RGB (user mode `0x33`) sends 8 × 64-byte bulk transfers, each encoding
16 key slots as `[0x00, R, G, B]`. The slot-to-physical-key mapping for `048d:600b`
is not yet fully documented.

## Project Status

### Implemented

- ✅ All 12 lighting effects with speed and brightness control
- ✅ Solid color, horizontal/vertical alternating colors
- ✅ Wave direction selection (right, left, up, down)
- ✅ EEPROM save (persist keyboard settings across reboots)
- ✅ Interactive TUI with live keyboard preview
- ✅ Front LED lightbar control (color + brightness)
- ✅ Lightbar sync with keyboard color
- ✅ **Lightbar software persistence** — state saved to `/etc/aucc/lightbar.conf`, restored on boot via udev
- ✅ `teclado` launcher script (pkexec-based, no sudo prompt)
- ✅ **Power profiles** — Silent / Balanced / Turbo (RAPL PL1/PL2 + CPU governor + EPP)
- ✅ **Telemetry dashboard** — CPU/GPU/RAM/NVMe/Battery in real-time (TUI)
- ✅ `--install` / `--uninstall` — instala/remove binários em `/usr/local/bin/` e gerencia regras udev diretamente pelo binário

### Known Limitations

- ❌ **Profile button LED** — The physical LED indicator next to the profile button on the
  Storm 470 chassis is controlled by the EC (Embedded Controller) firmware. Because this
  project changes RAPL/governor/EPP directly (bypassing the BIOS), the EC never learns
  about the profile switch and the button LED stays unchanged. There is no known Linux
  userspace interface for this LED: the ACPI `WMDE` method for WMI GUID
  `2BC49DEF-7B15-4F05-8BB7-EE37B9547C0B` is a stub (returns 0 for all inputs), and the
  `clevo-platform` / `tuxedo-keyboard` kernel drivers do not support this chassis.
  **The lightbar is used as the visual indicator instead.**
- ❌ **Fan control** — No `fan*_input` in hwmon; fans are EC-controlled with no
  documented sysfs/WMI interface for this chassis.
- ❌ **Battery charge limit** — `charge_control_end_threshold` not present in BAT0;
  not supported by this BIOS version.

### Planned

- Palette color customization for effects (7 color slots via `SET_PALETTE_COLOR`)
- JSON profile save/load (`~/.config/aucc/profiles/`)
- Granular brightness (1–50 levels)
- Per-key RGB mapping (requires key-slot research for `048d:600b`)
- Firmware version query (`cmd 0x80`)

## Thanks to

1. [Avell](https://avell.com.br/) — for this amazing laptop
2. [@kirainmoe](https://github.com/kirainmoe) — for help bringing macOS support
3. [@pobrn](https://github.com/pobrn/ite8291r3-ctl) — for ITE 8291 protocol research

## Contributions

Contributions of any kind are welcome.

## Donate :coffee: :hearts:

This is a project developed in free time. If you find it useful, consider [buying a coffee](https://www.buymeacoffee.com/KCZRP52U7).

