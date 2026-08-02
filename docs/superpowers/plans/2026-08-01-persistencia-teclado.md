# Persistência da configuração do teclado — Plano de Implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fazer a configuração de iluminação do teclado sobreviver a boot na
bateria, resume de suspend e transições AC↔bateria, mudando apenas quando o
usuário mudar a configuração.

**Architecture:** O EC apaga o backlight em eventos de energia, ignorando a
EEPROM do ITE 8291. A correção persiste o estado do teclado em
`/etc/aucc/keyboard.conf` e o reaplica via um serviço systemd único
(`aucc-restore.service`) disparado por udev no boot, na troca de energia e pelo
sleep hook no resume.

**Tech Stack:** Rust 2021 (rusb, clap, ratatui), systemd, udev.

## Global Constraints

- Spec de referência: `docs/superpowers/specs/2026-08-01-persistencia-teclado-design.md`.
- Comentários de código em inglês; mensagens ao usuário e docs em pt-BR (padrão do repo).
- Commits Conventional, subject ≤72 caracteres, imperativo, sem ponto final.
- Nunca usar `--no-verify`.
- Caminho do config do teclado: exatamente `/etc/aucc/keyboard.conf`.
- Nome da unit: exatamente `aucc-restore.service`. A unit antiga
  `aucc-lightbar-restore.service` deve ser desabilitada e removida na migração.
- `RemainAfterExit=no` na unit nova — requisito funcional, não estilo.
- Rodar `cargo test` a partir de `aucc-rs/`.

## Estrutura de arquivos

| Arquivo | Responsabilidade |
|---|---|
| `src/config/mod.rs` (criar) | Re-exporta os dois configs; parser `key=value` compartilhado |
| `src/config/lightbar.rs` (criar) | `LightbarConfig` — movido de `src/config.rs`, sem mudança de comportamento |
| `src/config/keyboard.rs` (criar) | `KeyboardConfig`: modelo, parse, save |
| `src/config.rs` (remover) | Substituído pelo diretório `src/config/` |
| `src/keyboard/restore.rs` (criar) | Traduz `KeyboardConfig` em chamadas ao dispositivo |
| `src/main.rs` (modificar) | Flags `--kb-restore` e `--restore`; grava estado em todo apply |
| `src/ui/tui.rs` (modificar) | Grava estado em `apply_final` e no Disable |
| `src/setup.rs` (modificar) | Unit nova, migração, regra udev de `power_supply` |
| `install/70-avell-hid.rules`, `install/install.sh` (modificar) | Espelham `setup.rs` — os dois são fonte da mesma config |
| `README.md` (modificar) | Documenta a persistência do teclado |

---

### Task 1: Dividir `config.rs` em módulo, sem mudar comportamento

Refatoração pura, preparando espaço para o config do teclado e eliminando a
duplicação entre `load_file` e `parse_file_impl` (hoje o mesmo parser escrito
duas vezes).

**Files:**
- Create: `aucc-rs/src/config/mod.rs`
- Create: `aucc-rs/src/config/lightbar.rs`
- Delete: `aucc-rs/src/config.rs`

**Interfaces:**
- Consumes: nada.
- Produces: `config::parse_kv(path: &str) -> Option<Vec<(String, String)>>`;
  `config::LightbarConfig`, `config::load()`, `config::load_file()`,
  `config::save()`, `config::save_to()`, `config::CONFIG_PATH` — todos com as
  mesmas assinaturas de hoje (`main.rs` e `tui.rs` não mudam).

- [ ] **Step 1: Rodar os testes atuais e registrar o baseline**

Run: `cd aucc-rs && cargo test config 2>&1 | tail -20`
Expected: PASS — 9 testes de config passando. Anote o número; ele não pode cair.

- [ ] **Step 2: Criar `src/config/mod.rs` com o parser compartilhado**

```rust
//! Persisted state for the lightbar and the keyboard.
//!
//! Both configs use the same trivial `key=value` line format so that they can
//! be inspected and edited by hand under /etc/aucc/.

pub mod keyboard;
pub mod lightbar;

pub use lightbar::{
    CONFIG_PATH, LightbarConfig, load, load_file, save, save_to,
};

use std::fs;

/// Parse a `key=value` file into pairs, skipping blanks and `#` comments.
///
/// Returns None only when the file cannot be read.
pub fn parse_kv(path: &str) -> Option<Vec<(String, String)>> {
    let content = fs::read_to_string(path).ok()?;
    Some(parse_kv_str(&content))
}

/// Parse `key=value` lines from an in-memory string.
pub fn parse_kv_str(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            out.push((key.trim().to_string(), val.trim().to_string()));
        }
    }
    out
}
```

- [ ] **Step 3: Criar `src/config/lightbar.rs` movendo o conteúdo atual**

Mova o conteúdo integral de `src/config.rs` (struct, `Default`, `PartialEq`,
`load`, `load_file`, `save`, `save_to`, e o módulo `mod tests` inteiro) para
`src/config/lightbar.rs`. Substitua os três parsers (`load_file`,
`parse_file_impl`, `parse_file`) por um único que usa `parse_kv`:

```rust
use super::parse_kv;
use std::fs;
use std::io::Write;
use std::path::Path;

pub const CONFIG_PATH: &str = "/etc/aucc/lightbar.conf";

// ... struct LightbarConfig, impl Default, impl PartialEq: inalterados ...

fn apply_pair(cfg: &mut LightbarConfig, key: &str, val: &str) {
    match key {
        "enabled"     => { if let Ok(v) = val.parse() { cfg.enabled = v; } }
        "r"           => { if let Ok(v) = val.parse() { cfg.r = v; } }
        "g"           => { if let Ok(v) = val.parse() { cfg.g = v; } }
        "b"           => { if let Ok(v) = val.parse() { cfg.b = v; } }
        "brightness"  => { if let Ok(v) = val.parse() { cfg.brightness = v; } }
        "save_eeprom" => { if let Ok(v) = val.parse() { cfg.save_eeprom = v; } }
        _ => {}
    }
}

fn parse_file(path: &str) -> Option<LightbarConfig> {
    let pairs = parse_kv(path)?;
    let mut cfg = LightbarConfig::default();
    for (k, v) in pairs {
        apply_pair(&mut cfg, &k, &v);
    }
    Some(cfg)
}

/// Load lightbar config from `/etc/aucc/lightbar.conf`.
/// Returns `LightbarConfig::default()` if the file is missing or unparseable.
pub fn load() -> LightbarConfig {
    parse_file(CONFIG_PATH).unwrap_or_default()
}

/// Load config and return Result (used by the TUI to check the file exists).
pub fn load_file() -> std::io::Result<LightbarConfig> {
    parse_file(CONFIG_PATH)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, CONFIG_PATH))
}

// ... save() e save_to(): inalterados ...
```

No `mod tests` movido, o único ajuste é que `parse_file` continua acessível via
`use super::*;` — os testes não mudam de conteúdo.

- [ ] **Step 4: Remover o arquivo antigo**

```bash
cd aucc-rs && git rm src/config.rs
```

- [ ] **Step 5: Rodar os testes**

Run: `cd aucc-rs && cargo test 2>&1 | tail -20`
Expected: PASS — mesmo número de testes de config do Step 1, zero falhas.
`cargo build` sem warnings novos (`main.rs` e `tui.rs` continuam compilando sem
edição, porque `config::*` foi re-exportado).

- [ ] **Step 6: Commit**

```bash
git add aucc-rs/src/config.rs aucc-rs/src/config/
git commit -m "refactor(config): dividir em modulo e deduplicar o parser"
```

---

### Task 2: `KeyboardConfig` — modelo, parse e save

**Files:**
- Create: `aucc-rs/src/config/keyboard.rs`
- Modify: `aucc-rs/src/config/mod.rs` (adicionar os re-exports)

**Interfaces:**
- Consumes: `config::parse_kv` (Task 1).
- Produces:
  - `config::keyboard::KEYBOARD_CONFIG_PATH: &str = "/etc/aucc/keyboard.conf"`
  - `pub enum KeyboardMode { Off, Mono, HAlt, VAlt, Effect }`
  - `pub struct KeyboardConfig { mode: KeyboardMode, r: u8, g: u8, b: u8, r2: u8, g2: u8, b2: u8, brightness: u8, effect: String, speed: u8, direction: String, letter: Option<char>, reactive: bool }`
  - `KeyboardConfig::default()` → `mode: Off` (nenhum arquivo = nada a restaurar)
  - `pub fn load_keyboard() -> KeyboardConfig`
  - `pub fn save_keyboard(cfg: &KeyboardConfig) -> std::io::Result<()>`
  - `pub fn save_keyboard_to(cfg: &KeyboardConfig, path: &str) -> std::io::Result<()>`
  - `pub fn parse_keyboard_file(path: &str) -> Option<KeyboardConfig>` (usado nos testes)

- [ ] **Step 1: Escrever os testes que falham**

Crie `aucc-rs/src/config/keyboard.rs` contendo **apenas** o bloco de testes
abaixo mais os `use` necessários; o código de produção vem no Step 3.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> String {
        format!("/tmp/aucc_kb_test_{name}.conf")
    }

    fn cleanup(name: &str) {
        let _ = fs::remove_file(temp_path(name));
    }

    #[test]
    fn default_is_off() {
        assert_eq!(KeyboardConfig::default().mode, KeyboardMode::Off);
    }

    #[test]
    fn round_trip_mono() {
        cleanup("mono");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Mono,
            r: 255, g: 0, b: 128,
            brightness: 3,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("mono")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("mono")).unwrap(), cfg);
        cleanup("mono");
    }

    #[test]
    fn round_trip_halt() {
        cleanup("halt");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::HAlt,
            r: 255, g: 0, b: 0,
            r2: 0, g2: 0, b2: 255,
            brightness: 4,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("halt")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("halt")).unwrap(), cfg);
        cleanup("halt");
    }

    #[test]
    fn round_trip_valt() {
        cleanup("valt");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::VAlt,
            r: 0, g: 255, b: 0,
            r2: 255, g2: 255, b2: 0,
            brightness: 2,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("valt")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("valt")).unwrap(), cfg);
        cleanup("valt");
    }

    #[test]
    fn round_trip_effect_with_all_fields() {
        cleanup("effect");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 7,
            brightness: 4,
            direction: "left".to_string(),
            letter: Some('g'),
            reactive: true,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("effect")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("effect")).unwrap(), cfg);
        cleanup("effect");
    }

    #[test]
    fn round_trip_off() {
        cleanup("off");
        let cfg = KeyboardConfig { mode: KeyboardMode::Off, ..Default::default() };
        save_keyboard_to(&cfg, &temp_path("off")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("off")).unwrap().mode, KeyboardMode::Off);
        cleanup("off");
    }

    #[test]
    fn letter_none_round_trips_as_absent() {
        cleanup("noletter");
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "rainbow".to_string(),
            letter: None,
            ..Default::default()
        };
        save_keyboard_to(&cfg, &temp_path("noletter")).unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("noletter")).unwrap().letter, None);
        cleanup("noletter");
    }

    #[test]
    fn missing_file_returns_none() {
        cleanup("missing");
        assert_eq!(parse_keyboard_file(&temp_path("missing")), None);
    }

    #[test]
    fn empty_file_yields_default() {
        cleanup("empty");
        fs::write(temp_path("empty"), "").unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("empty")).unwrap(), KeyboardConfig::default());
        cleanup("empty");
    }

    #[test]
    fn comments_and_blanks_are_skipped() {
        cleanup("comments");
        fs::write(temp_path("comments"), "# comentario\n\nmode=mono\nr=10\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("comments")).unwrap();
        assert_eq!(loaded.mode, KeyboardMode::Mono);
        assert_eq!(loaded.r, 10);
        cleanup("comments");
    }

    #[test]
    fn invalid_lines_are_ignored() {
        cleanup("invalid");
        fs::write(temp_path("invalid"), "mode=mono\nlixo sem igual\nr=200\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("invalid")).unwrap();
        assert_eq!(loaded.r, 200);
        assert_eq!(loaded.mode, KeyboardMode::Mono);
        cleanup("invalid");
    }

    #[test]
    fn unknown_mode_falls_back_to_off() {
        cleanup("badmode");
        fs::write(temp_path("badmode"), "mode=banana\n").unwrap();
        assert_eq!(parse_keyboard_file(&temp_path("badmode")).unwrap().mode, KeyboardMode::Off);
        cleanup("badmode");
    }

    #[test]
    fn out_of_range_values_are_ignored() {
        cleanup("range");
        fs::write(temp_path("range"), "mode=mono\nbrightness=99\nspeed=0\n").unwrap();
        let loaded = parse_keyboard_file(&temp_path("range")).unwrap();
        assert_eq!(loaded.brightness, KeyboardConfig::default().brightness);
        assert_eq!(loaded.speed, KeyboardConfig::default().speed);
        cleanup("range");
    }
}
```

- [ ] **Step 2: Rodar os testes para confirmar que falham**

Adicione `pub mod keyboard;` em `src/config/mod.rs` antes de rodar.

Run: `cd aucc-rs && cargo test config::keyboard 2>&1 | tail -20`
Expected: FAIL na compilação — `cannot find type KeyboardConfig in this scope`.

- [ ] **Step 3: Implementar o mínimo para passar**

Adicione no topo de `src/config/keyboard.rs`:

```rust
use super::parse_kv;
use std::fs;
use std::io::Write;
use std::path::Path;

pub const KEYBOARD_CONFIG_PATH: &str = "/etc/aucc/keyboard.conf";

/// Which kind of lighting the user last applied to the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// No stored state, or the user deliberately turned the backlight off.
    #[default]
    Off,
    Mono,
    HAlt,
    VAlt,
    Effect,
}

impl KeyboardMode {
    fn as_str(self) -> &'static str {
        match self {
            KeyboardMode::Off    => "off",
            KeyboardMode::Mono   => "mono",
            KeyboardMode::HAlt   => "halt",
            KeyboardMode::VAlt   => "valt",
            KeyboardMode::Effect => "effect",
        }
    }

    /// Unknown values fall back to Off so a corrupt file never lights the
    /// keyboard with something the user did not choose.
    fn from_str(s: &str) -> Self {
        match s {
            "mono"   => KeyboardMode::Mono,
            "halt"   => KeyboardMode::HAlt,
            "valt"   => KeyboardMode::VAlt,
            "effect" => KeyboardMode::Effect,
            _        => KeyboardMode::Off,
        }
    }
}

/// Last keyboard lighting the user applied. Reapplied by `aucc --kb-restore`
/// after the EC clears the backlight on power events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardConfig {
    pub mode: KeyboardMode,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Secondary color, used by the HAlt/VAlt modes only.
    pub r2: u8,
    pub g2: u8,
    pub b2: u8,
    /// 1–4 (hardware levels).
    pub brightness: u8,
    /// Effect name as accepted by `Effect::from_str`.
    pub effect: String,
    /// 1–10 (1 = fastest).
    pub speed: u8,
    /// right | left | up | down
    pub direction: String,
    /// Effect color-variant suffix ('r', 'o', 'y', 'g', 'b', 't', 'p').
    pub letter: Option<char>,
    pub reactive: bool,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            mode: KeyboardMode::Off,
            r: 0xff, g: 0xff, b: 0xff,
            r2: 0xff, g2: 0xff, b2: 0xff,
            brightness: 4,
            effect: "rainbow".to_string(),
            speed: 5,
            direction: "right".to_string(),
            letter: None,
            reactive: false,
        }
    }
}

fn apply_pair(cfg: &mut KeyboardConfig, key: &str, val: &str) {
    match key {
        "mode"       => cfg.mode = KeyboardMode::from_str(val),
        "r"          => { if let Ok(v) = val.parse() { cfg.r = v; } }
        "g"          => { if let Ok(v) = val.parse() { cfg.g = v; } }
        "b"          => { if let Ok(v) = val.parse() { cfg.b = v; } }
        "r2"         => { if let Ok(v) = val.parse() { cfg.r2 = v; } }
        "g2"         => { if let Ok(v) = val.parse() { cfg.g2 = v; } }
        "b2"         => { if let Ok(v) = val.parse() { cfg.b2 = v; } }
        // Out-of-range values are dropped rather than clamped: a bad file
        // should not silently change what the user configured.
        "brightness" => { if let Ok(v) = val.parse::<u8>() { if (1..=4).contains(&v)  { cfg.brightness = v; } } }
        "speed"      => { if let Ok(v) = val.parse::<u8>() { if (1..=10).contains(&v) { cfg.speed = v; } } }
        "effect"     => cfg.effect = val.to_string(),
        "direction"  => cfg.direction = val.to_string(),
        "letter"     => cfg.letter = val.chars().next(),
        "reactive"   => { if let Ok(v) = val.parse() { cfg.reactive = v; } }
        _ => {}
    }
}

/// Parse a keyboard config from a specific path. None if unreadable.
pub fn parse_keyboard_file(path: &str) -> Option<KeyboardConfig> {
    let pairs = parse_kv(path)?;
    let mut cfg = KeyboardConfig::default();
    for (k, v) in pairs {
        apply_pair(&mut cfg, &k, &v);
    }
    Some(cfg)
}

/// Load the keyboard config, falling back to "off" when absent or unreadable.
pub fn load_keyboard() -> KeyboardConfig {
    parse_keyboard_file(KEYBOARD_CONFIG_PATH).unwrap_or_default()
}

fn write_to(cfg: &KeyboardConfig, path: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    writeln!(f, "mode={}", cfg.mode.as_str())?;
    writeln!(f, "r={}", cfg.r)?;
    writeln!(f, "g={}", cfg.g)?;
    writeln!(f, "b={}", cfg.b)?;
    writeln!(f, "r2={}", cfg.r2)?;
    writeln!(f, "g2={}", cfg.g2)?;
    writeln!(f, "b2={}", cfg.b2)?;
    writeln!(f, "brightness={}", cfg.brightness)?;
    writeln!(f, "effect={}", cfg.effect)?;
    writeln!(f, "speed={}", cfg.speed)?;
    writeln!(f, "direction={}", cfg.direction)?;
    if let Some(l) = cfg.letter {
        writeln!(f, "letter={l}")?;
    }
    writeln!(f, "reactive={}", cfg.reactive)?;
    Ok(())
}

/// Persist to `/etc/aucc/keyboard.conf`.
pub fn save_keyboard(cfg: &KeyboardConfig) -> std::io::Result<()> {
    write_to(cfg, KEYBOARD_CONFIG_PATH)
}

/// Persist to a specific path (used by tests).
pub fn save_keyboard_to(cfg: &KeyboardConfig, path: &str) -> std::io::Result<()> {
    write_to(cfg, path)
}
```

Em `src/config/mod.rs`, adicione o re-export após o de lightbar:

```rust
pub use keyboard::{
    KEYBOARD_CONFIG_PATH, KeyboardConfig, KeyboardMode, load_keyboard, save_keyboard,
    save_keyboard_to,
};
```

- [ ] **Step 4: Rodar os testes**

Run: `cd aucc-rs && cargo test config::keyboard 2>&1 | tail -20`
Expected: PASS — 12 testes.

- [ ] **Step 5: Commit**

```bash
git add aucc-rs/src/config/
git commit -m "feat(config): adicionar KeyboardConfig persistido em keyboard.conf"
```

---

### Task 3: Traduzir `KeyboardConfig` em comandos de dispositivo

Separa a parte pura (config → payload de efeito) da parte que fala com o USB,
para que a tradução seja testável sem hardware.

**Files:**
- Create: `aucc-rs/src/keyboard/restore.rs`
- Modify: `aucc-rs/src/keyboard/mod.rs:1-2` (adicionar `pub mod restore;`)

**Interfaces:**
- Consumes: `config::{KeyboardConfig, KeyboardMode}` (Task 2);
  `keyboard::effects::{Effect, WaveDirection, effect_payload}`;
  `keyboard::KeyboardDevice::{apply_mono_color, apply_alt_color, apply_effect, disable}`.
- Produces:
  - `keyboard::restore::payload_for(cfg: &KeyboardConfig) -> Option<[u8; 8]>`
  - `keyboard::restore::apply(dev: &KeyboardDevice, cfg: &KeyboardConfig) -> rusb::Result<()>`

- [ ] **Step 1: Escrever os testes que falham**

Crie `aucc-rs/src/keyboard/restore.rs` com apenas este bloco de testes:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{KeyboardConfig, KeyboardMode};
    use crate::keyboard::effects::{Effect, WaveDirection, effect_payload};

    #[test]
    fn payload_none_for_non_effect_modes() {
        for mode in [KeyboardMode::Off, KeyboardMode::Mono, KeyboardMode::HAlt, KeyboardMode::VAlt] {
            let cfg = KeyboardConfig { mode, ..Default::default() };
            assert_eq!(payload_for(&cfg), None, "mode {mode:?} nao deve gerar payload de efeito");
        }
    }

    #[test]
    fn payload_matches_effect_payload_for_wave() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 7,
            brightness: 3,
            direction: "left".to_string(),
            letter: None,
            reactive: false,
            ..Default::default()
        };
        let expected = effect_payload(Effect::Wave, 7, 3, None, WaveDirection::Left, false, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }

    #[test]
    fn payload_carries_letter_and_reactive() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "breathing".to_string(),
            speed: 5,
            brightness: 4,
            direction: "right".to_string(),
            letter: Some('g'),
            reactive: true,
            ..Default::default()
        };
        let expected =
            effect_payload(Effect::Breathing, 5, 4, Some('g'), WaveDirection::Right, true, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }

    #[test]
    fn payload_never_sets_the_eeprom_save_byte() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "rainbow".to_string(),
            ..Default::default()
        };
        assert_eq!(payload_for(&cfg).unwrap()[7], 0x00);
    }

    #[test]
    fn payload_none_for_unknown_effect_name() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "nao_existe".to_string(),
            ..Default::default()
        };
        assert_eq!(payload_for(&cfg), None);
    }

    #[test]
    fn unknown_direction_falls_back_to_right() {
        let cfg = KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: "wave".to_string(),
            speed: 5,
            brightness: 4,
            direction: "diagonal".to_string(),
            ..Default::default()
        };
        let expected = effect_payload(Effect::Wave, 5, 4, None, WaveDirection::Right, false, false);
        assert_eq!(payload_for(&cfg), Some(expected));
    }
}
```

- [ ] **Step 2: Rodar os testes para confirmar que falham**

Adicione `pub mod restore;` no topo de `src/keyboard/mod.rs` (junto de
`pub mod colors;` e `pub mod effects;`) antes de rodar.

Run: `cd aucc-rs && cargo test keyboard::restore 2>&1 | tail -20`
Expected: FAIL na compilação — `cannot find function payload_for in this scope`.

- [ ] **Step 3: Implementar**

Adicione no topo de `src/keyboard/restore.rs`:

```rust
//! Reapply the persisted keyboard state after the EC clears the backlight
//! (boot on battery, AC<->battery transition, resume from suspend).

use crate::config::{KeyboardConfig, KeyboardMode};
use crate::keyboard::KeyboardDevice;
use crate::keyboard::effects::{Effect, WaveDirection, effect_payload};

/// Build the effect payload for a config in Effect mode.
///
/// Returns None for the other modes, and for an effect name the firmware does
/// not know. `save` is always false: restoring must not rewrite the EEPROM.
pub fn payload_for(cfg: &KeyboardConfig) -> Option<[u8; 8]> {
    if cfg.mode != KeyboardMode::Effect {
        return None;
    }
    let effect = Effect::from_str(&cfg.effect)?;
    let direction = WaveDirection::from_str(&cfg.direction).unwrap_or_default();
    Some(effect_payload(
        effect,
        cfg.speed,
        cfg.brightness,
        cfg.letter,
        direction,
        cfg.reactive,
        false,
    ))
}

/// Apply the persisted state to the device.
///
/// `save` is false on every path — the EEPROM already holds whatever the user
/// chose to persist there, and a restore is not a new user decision.
pub fn apply(dev: &KeyboardDevice, cfg: &KeyboardConfig) -> rusb::Result<()> {
    match cfg.mode {
        KeyboardMode::Off => dev.disable(),
        KeyboardMode::Mono => {
            dev.apply_mono_color(cfg.r, cfg.g, cfg.b, cfg.brightness, false)
        }
        KeyboardMode::HAlt | KeyboardMode::VAlt => dev.apply_alt_color(
            cfg.r, cfg.g, cfg.b,
            cfg.r2, cfg.g2, cfg.b2,
            cfg.brightness,
            cfg.mode == KeyboardMode::HAlt,
            false,
        ),
        KeyboardMode::Effect => match payload_for(cfg) {
            Some(payload) => dev.apply_effect(&payload),
            // Unknown effect name in the file: leave the keyboard untouched
            // rather than guessing a different effect.
            None => Ok(()),
        },
    }
}
```

- [ ] **Step 4: Rodar os testes**

Run: `cd aucc-rs && cargo test keyboard::restore 2>&1 | tail -20`
Expected: PASS — 6 testes.

- [ ] **Step 5: Commit**

```bash
git add aucc-rs/src/keyboard/
git commit -m "feat(keyboard): traduzir KeyboardConfig em comandos do dispositivo"
```

---

### Task 4: CLI — gravar em todo apply e adicionar `--kb-restore` / `--restore`

**Files:**
- Modify: `aucc-rs/src/main.rs` (bloco `--off` em 236-262; `run_lightbar` em 316-358; `run` em 382-417; struct `Cli` em 109-209; `main` em 219-312)

**Interfaces:**
- Consumes: `config::{KeyboardConfig, KeyboardMode, load_keyboard, save_keyboard}` (Task 2); `keyboard::restore` (Task 3).
- Produces: flags de CLI `--kb-restore` e `--restore`; `/etc/aucc/keyboard.conf`
  escrito por `--color`, `-H`, `-V`, `--style`, `--disable` e `--off`.

- [ ] **Step 1: Adicionar as flags à struct `Cli`**

No import do topo (linha 1), troque para:

```rust
use aucc_rs::config::{self, KeyboardConfig, KeyboardMode, LightbarConfig};
```

Depois do campo `lb_brightness` (linha 189), adicione:

```rust
    /// [Teclado] Restaurar estado salvo de /etc/aucc/keyboard.conf
    ///
    /// Reaplica a última configuração aplicada pelo usuário. Executado
    /// automaticamente no boot, ao trocar entre AC e bateria e após suspend.
    #[arg(long)]
    kb_restore: bool,

    /// Restaurar teclado e lightbar (usado pelo systemd)
    #[arg(long)]
    restore: bool,
```

E acrescente `"kb_restore"` e `"restore"` à lista do `ArgGroup` na linha 108.

- [ ] **Step 2: Compilar para confirmar que as flags existem e ainda não fazem nada**

Run: `cd aucc-rs && cargo build 2>&1 | tail -5 && ./target/debug/aucc --help | grep -E "restore"`
Expected: build OK; o help lista `--kb-restore` e `--restore`.

- [ ] **Step 3: Gravar o estado em cada comando de teclado**

Em `run()` (linha 382), grave o config antes de cada `return Ok(())`. O arquivo
é escrito com `let _ =` porque uma falha ao persistir não deve impedir que a
luz seja aplicada — o comando já surtiu efeito no hardware.

```rust
fn run(dev: &KeyboardDevice, cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if cli.disable {
        dev.disable()?;
        let _ = config::save_keyboard(&KeyboardConfig {
            mode: KeyboardMode::Off,
            ..config::load_keyboard()
        });
        println!("{}", "Teclado desligado.".dimmed());
        return Ok(());
    }
    if let Some(style) = &cli.style {
        let (name, letter, reactive) = split_style(style);
        let effect = Effect::from_str(name).ok_or_else(|| format!("Efeito desconhecido: '{name}'"))?;
        let dir = cli.direction.to_wave_dir();
        dev.apply_effect(&effect_payload(effect, cli.speed, cli.brightness, letter, dir, reactive, cli.save))?;
        let _ = config::save_keyboard(&KeyboardConfig {
            mode: KeyboardMode::Effect,
            effect: name.to_string(),
            speed: cli.speed,
            brightness: cli.brightness,
            direction: format!("{:?}", cli.direction).to_lowercase(),
            letter,
            reactive,
            ..Default::default()
        });
        println!("{}", format!("Efeito '{style}' aplicado.").green());
        return Ok(());
    }
    if let Some(c) = &cli.color {
        let (r, g, b) = get_color(c).ok_or_else(|| format!("Cor desconhecida: '{c}'"))?;
        dev.apply_mono_color(r, g, b, cli.brightness, cli.save)?;
        let _ = config::save_keyboard(&KeyboardConfig {
            mode: KeyboardMode::Mono,
            r, g, b,
            brightness: cli.brightness,
            ..Default::default()
        });
        println!("{}", format!("Cor '{c}' aplicada.").green());
        return Ok(());
    }
    if let Some(cols) = &cli.h_alt {
        let (ra, ga, ba) = get_color(&cols[0]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[0]))?;
        let (rb, gb, bb) = get_color(&cols[1]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[1]))?;
        dev.apply_alt_color(ra, ga, ba, rb, gb, bb, cli.brightness, true, cli.save)?;
        let _ = config::save_keyboard(&KeyboardConfig {
            mode: KeyboardMode::HAlt,
            r: ra, g: ga, b: ba,
            r2: rb, g2: gb, b2: bb,
            brightness: cli.brightness,
            ..Default::default()
        });
        println!("{}", format!("Alternado H: {} / {} aplicado.", cols[0], cols[1]).green());
        return Ok(());
    }
    if let Some(cols) = &cli.v_alt {
        let (ra, ga, ba) = get_color(&cols[0]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[0]))?;
        let (rb, gb, bb) = get_color(&cols[1]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[1]))?;
        dev.apply_alt_color(ra, ga, ba, rb, gb, bb, cli.brightness, false, cli.save)?;
        let _ = config::save_keyboard(&KeyboardConfig {
            mode: KeyboardMode::VAlt,
            r: ra, g: ga, b: ba,
            r2: rb, g2: gb, b2: bb,
            brightness: cli.brightness,
            ..Default::default()
        });
        println!("{}", format!("Alternado V: {} / {} aplicado.", cols[0], cols[1]).green());
        return Ok(());
    }
    Ok(())
}
```

`format!("{:?}", cli.direction).to_lowercase()` produz exatamente `right`,
`left`, `up`, `down` — os nomes que `WaveDirection::from_str` aceita.

No bloco `--off` de `main()` (linha 245), após `dev.disable()` bem-sucedido,
adicione a mesma gravação:

```rust
        if let Err(e) = dev.disable() {
            eprintln!("{} {e}", "Erro teclado:".red().bold());
        } else {
            let _ = config::save_keyboard(&KeyboardConfig {
                mode: KeyboardMode::Off,
                ..config::load_keyboard()
            });
            println!("{}", "Teclado desligado.".dimmed());
        }
```

- [ ] **Step 4: Implementar `--kb-restore` e `--restore`**

Adicione estas funções ao final de `main.rs`:

```rust
// ── restore ───────────────────────────────────────────────────────────────────

/// Reapply the persisted keyboard state. Logs to stderr so the systemd unit
/// leaves a trace in journalctl.
fn run_kb_restore() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = config::load_keyboard();
    eprintln!(
        "aucc --kb-restore: mode={:?} rgb=({},{},{}) brightness={} effect={}",
        cfg.mode, cfg.r, cfg.g, cfg.b, cfg.brightness, cfg.effect
    );
    let dev = KeyboardDevice::open()?;
    aucc_rs::keyboard::restore::apply(&dev, &cfg)?;
    Ok(())
}

/// Restore both devices. Used by aucc-restore.service. A failure on one device
/// must not stop the other, so errors are reported and collected, not raised
/// on the first occurrence.
fn run_restore() -> Result<(), Box<dyn std::error::Error>> {
    let mut failed = Vec::new();

    match lightbar::find_hidraw_path() {
        Some(path) => {
            let cfg = config::load();
            eprintln!(
                "aucc --restore: lightbar enabled={} rgb=({},{},{}) brightness={}",
                cfg.enabled, cfg.r, cfg.g, cfg.b, cfg.brightness
            );
            let res = if cfg.enabled {
                lightbar::apply_color(&path, cfg.r, cfg.g, cfg.b, cfg.brightness)
            } else {
                lightbar::disable(&path)
            };
            if let Err(e) = res {
                eprintln!("{} {e}", "Erro lightbar:".red().bold());
                failed.push("lightbar");
            }
        }
        None => eprintln!("aucc --restore: lightbar nao encontrada, ignorando"),
    }

    if let Err(e) = run_kb_restore() {
        eprintln!("{} {e}", "Erro teclado:".red().bold());
        failed.push("teclado");
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("falha ao restaurar: {}", failed.join(", ")).into())
    }
}
```

Em `main()`, logo após o bloco `--telemetry` (linha 233) e **antes** do bloco
`--off`, adicione o roteamento. Ambos precisam de root (o teclado é USB direto):

```rust
    if cli.restore || cli.kb_restore {
        require_root();
        let result = if cli.restore { run_restore() } else { run_kb_restore() };
        if let Err(e) = result {
            eprintln!("{} {e}", "Erro:".red().bold());
            std::process::exit(1);
        }
        return;
    }
```

- [ ] **Step 5: Verificar manualmente no hardware**

```bash
cd aucc-rs && cargo build --release
sudo ./target/release/aucc --color red --brightness 3
cat /etc/aucc/keyboard.conf
```
Expected: teclado vermelho; o arquivo mostra `mode=mono`, `r=255`, `g=0`,
`b=0`, `brightness=3`.

```bash
sudo ./target/release/aucc --disable
cat /etc/aucc/keyboard.conf | head -1
sudo ./target/release/aucc --kb-restore
```
Expected: `mode=off` após o disable; o `--kb-restore` mantém o teclado apagado
(restaurar "apagado" é restaurar corretamente).

```bash
sudo ./target/release/aucc --style breathingg --speed 3
sudo ./target/release/aucc --disable
sudo ./target/release/aucc --kb-restore
```
Expected: o efeito breathing verde volta após o restore.

- [ ] **Step 6: Rodar a suíte completa**

Run: `cd aucc-rs && cargo test 2>&1 | tail -20`
Expected: PASS, zero falhas.

- [ ] **Step 7: Commit**

```bash
git add aucc-rs/src/main.rs
git commit -m "feat(cli): persistir estado do teclado e adicionar --kb-restore"
```

---

### Task 5: TUI — gravar o estado ao aplicar

**Files:**
- Modify: `aucc-rs/src/ui/tui.rs:332-400` (`apply_final`), `aucc-rs/src/ui/tui.rs:435` (Disable)

**Interfaces:**
- Consumes: `config::{KeyboardConfig, KeyboardMode, save_keyboard}` (Task 2).
- Produces: nada consumido por outras tasks.

Só `apply_final` persiste. `live_preview` (linha 273) manda comandos enquanto o
usuário navega pela lista de cores — persistir ali gravaria cada cor sobre a
qual o cursor passa, que não é uma escolha do usuário.

- [ ] **Step 1: Persistir em `apply_final`**

Em cada braço de `apply_final`, após o `usb_tx.send(...)`, adicione a gravação
correspondente. Braço `Some("effect")`:

```rust
                let _ = config::save_keyboard(&config::KeyboardConfig {
                    mode: config::KeyboardMode::Effect,
                    effect: self.effect.to_string(),
                    speed: self.speed,
                    brightness: self.brightness,
                    direction: format!("{:?}", self.wave_dir).to_lowercase(),
                    letter: self.eff_variant,
                    reactive: self.reactive,
                    ..Default::default()
                });
```

Braço `Some("h_alt")` (e o `Some("v_alt")`, trocando `HAlt` por `VAlt`):

```rust
                let _ = config::save_keyboard(&config::KeyboardConfig {
                    mode: config::KeyboardMode::HAlt,
                    r: ra, g: ga, b: ba,
                    r2: rb, g2: gb, b2: bb,
                    brightness: self.brightness,
                    ..Default::default()
                });
```

Braço `_` (cor sólida):

```rust
                let _ = config::save_keyboard(&config::KeyboardConfig {
                    mode: config::KeyboardMode::Mono,
                    r, g, b,
                    brightness: self.brightness,
                    ..Default::default()
                });
```

`self.effect.to_string()` usa o `Display` de `Effect`, que emite exatamente os
nomes aceitos por `Effect::from_str` (`breathing`, `wave`, `rainbow`, …).

- [ ] **Step 2: Persistir no Disable**

Na linha 435, após `self.usb_tx.send(UsbCmd::Disable)`:

```rust
                        let _ = self.usb_tx.send(UsbCmd::Disable);
                        let _ = config::save_keyboard(&config::KeyboardConfig {
                            mode: config::KeyboardMode::Off,
                            ..config::load_keyboard()
                        });
```

- [ ] **Step 3: Compilar e verificar no hardware**

Run: `cd aucc-rs && cargo build --release && sudo ./target/release/aucc-ui`

Na TUI: aplique uma cor sólida, saia, e confira:
```bash
cat /etc/aucc/keyboard.conf
```
Expected: `mode=mono` com o RGB da cor escolhida.

Repita navegando pela lista de cores **sem** confirmar: o arquivo não deve
mudar até você aplicar (isso valida que `live_preview` não persiste).

- [ ] **Step 4: Rodar a suíte**

Run: `cd aucc-rs && cargo test 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add aucc-rs/src/ui/tui.rs
git commit -m "feat(tui): persistir estado do teclado ao aplicar"
```

---

### Task 6: systemd e udev — gatilhos de boot, energia e resume

**Files:**
- Modify: `aucc-rs/src/setup.rs` (constantes `UDEV_RULES`, `LIGHTBAR_RESTORE_SERVICE`, `LIGHTBAR_SLEEP_HOOK`, `RESTORE_SERVICE_PATH`, e as funções `install`/`uninstall`)
- Modify: `install/70-avell-hid.rules`
- Modify: `install/install.sh` (funções `install_systemd` e `print_summary`)

**Interfaces:**
- Consumes: a flag `--restore` (Task 4).
- Produces: `/etc/systemd/system/aucc-restore.service`;
  `setup::RESTORE_SERVICE_PATH` passa a apontar para ela;
  `setup::OLD_RESTORE_SERVICE_PATH` (novo) para a migração.

- [ ] **Step 1: Atualizar as constantes em `setup.rs`**

```rust
const UDEV_RULES: &str = "\
# udev rules for Avell Storm 470 (TongFang chassis) HID devices — managed by aucc
#
# Grants read/write access to members of the 'plugdev' group so that
# keyboard RGB and lightbar control work WITHOUT root privileges.
#
# The SYSTEMD_WANTS entries trigger aucc-restore.service when the devices
# appear (boot or reconnect) and when the AC adapter is plugged or unplugged.
# The EC clears the keyboard backlight on those power events, so the state has
# to be reapplied. Resume from suspend is handled by
# /lib/systemd/system-sleep/aucc-lightbar.

# ITE Device 8291 — RGB Keyboard (048d:600b)
SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"600b\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\", TAG+=\"systemd\", \\
    ENV{SYSTEMD_WANTS}+=\"aucc-restore.service\"

# ITE Device 8233 — Front LED Lightbar (048d:7001)
SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"7001\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\", TAG+=\"systemd\", \\
    ENV{SYSTEMD_WANTS}+=\"aucc-restore.service\"

SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"7001\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\"

# AC adapter plugged/unplugged — the EC turns the keyboard backlight off here.
SUBSYSTEM==\"power_supply\", ACTION==\"change\", ATTR{type}==\"Mains\", \\
    TAG+=\"systemd\", ENV{SYSTEMD_WANTS}+=\"aucc-restore.service\"
";

const RESTORE_SERVICE: &str = "\
[Unit]
Description=Restore Avell keyboard and lightbar state
After=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/bin/aucc --restore
StandardError=journal

[Install]
WantedBy=multi-user.target
";

// Called by systemd-sleep with args (pre|post) (suspend|hibernate|...).
const RESTORE_SLEEP_HOOK: &str = "\
#!/bin/sh
# Restore Avell keyboard and lightbar after resume — managed by aucc
[ \"$1\" = \"post\" ] && /usr/local/bin/aucc --restore
";

pub const UDEV_RULE_PATH: &str       = "/etc/udev/rules.d/70-avell-hid.rules";
pub const RESTORE_SERVICE_PATH: &str = "/etc/systemd/system/aucc-restore.service";
/// Pre-0.2 unit name, removed on install so the old one does not linger.
pub const OLD_RESTORE_SERVICE_PATH: &str =
    "/etc/systemd/system/aucc-lightbar-restore.service";
pub const SLEEP_HOOK_PATH: &str      = "/lib/systemd/system-sleep/aucc-lightbar";
pub const INSTALL_BIN_PATH: &str     = "/usr/local/bin/aucc";
pub const INSTALL_UI_BIN_PATH: &str  = "/usr/local/bin/aucc-ui";
```

**`RemainAfterExit=yes` foi removido de propósito.** Com ele, o systemd trata a
unit como já ativa e ignora um novo `SYSTEMD_WANTS` — o gatilho de troca de
energia nunca dispararia. Não reintroduzir.

Substitua as referências a `LIGHTBAR_RESTORE_SERVICE` e `LIGHTBAR_SLEEP_HOOK`
no corpo de `install()` por `RESTORE_SERVICE` e `RESTORE_SLEEP_HOOK`, e as
strings `"aucc-lightbar-restore.service"` nos comandos `systemctl` por
`"aucc-restore.service"`.

- [ ] **Step 2: Adicionar a migração em `install()`**

Antes do passo que escreve a unit nova (passo 4 da função), adicione:

```rust
    // 3b. Migration: drop the pre-0.2 unit so two services do not race to
    // restore the same devices.
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-lightbar-restore.service"])
        .status();
    let _ = fs::remove_file(OLD_RESTORE_SERVICE_PATH);
```

E em `uninstall()`, troque o `systemctl disable --now` para a unit nova e
inclua o path antigo na lista de arquivos removidos:

```rust
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-restore.service"])
        .status();
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-lightbar-restore.service"])
        .status();

    for path in [RESTORE_SERVICE_PATH, OLD_RESTORE_SERVICE_PATH, SLEEP_HOOK_PATH] {
```

- [ ] **Step 3: Sincronizar `install/70-avell-hid.rules`**

Copie o conteúdo literal da constante `UDEV_RULES` (sem os escapes de string
Rust) para `install/70-avell-hid.rules`, mantendo o cabeçalho de instruções de
instalação que já existe no arquivo.

- [ ] **Step 4: Sincronizar `install/install.sh`**

Em `install_systemd()`, substitua o heredoc da unit e o do sleep hook pelos
novos conteúdos (mesmo texto do Step 1), troque o nome do arquivo para
`$SYSTEMD_DIR/aucc-restore.service`, adicione a migração antes de escrever e
ajuste o `systemctl enable`:

```bash
    # Migration: remove the pre-0.2 unit.
    systemctl disable --now aucc-lightbar-restore.service 2>/dev/null || true
    rm -f "$SYSTEMD_DIR/aucc-lightbar-restore.service"

    cat > "$SYSTEMD_DIR/aucc-restore.service" <<'EOF'
[Unit]
Description=Restore Avell keyboard and lightbar state
After=systemd-udev-settle.service

[Service]
Type=oneshot
ExecStart=/usr/local/bin/aucc --restore
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    cat > "$SLEEP_HOOK_DIR/aucc-lightbar" <<'EOF'
#!/bin/sh
# Restore Avell keyboard and lightbar after resume — managed by aucc
[ "$1" = "post" ] && /usr/local/bin/aucc --restore
EOF
    chmod +x "$SLEEP_HOOK_DIR/aucc-lightbar"

    systemctl daemon-reload
    systemctl enable --now aucc-restore.service
    info "systemd service habilitado: aucc-restore.service"
```

Em `install_udev()`, o `udevadm trigger` também precisa cobrir o novo
subsistema:

```bash
    udevadm trigger --subsystem-match=usb --subsystem-match=hidraw --subsystem-match=power_supply
```

Em `print_summary()`, troque a linha da lightbar por:
```
    echo "  Restaurar tudo:     aucc --restore  (automático no boot, na troca AC/bateria e após suspend)"
```

- [ ] **Step 5: Instalar e verificar o estado do systemd**

```bash
cd aucc-rs && cargo build --release
sudo ./target/release/aucc --install
systemctl status aucc-restore.service --no-pager
systemctl list-unit-files 'aucc*'
```
Expected: `aucc-restore.service` habilitada e com execução bem-sucedida;
`aucc-lightbar-restore.service` **não** aparece mais na listagem.

```bash
systemctl show aucc-restore.service -p RemainAfterExit
```
Expected: `RemainAfterExit=no`.

- [ ] **Step 6: Verificar que o gatilho de energia dispara**

```bash
journalctl -f -u aucc-restore.service
```
Com o journal aberto, desconecte e reconecte o cabo de força. Cada evento deve
produzir um novo par "Starting/Finished" com a linha
`aucc --kb-restore: mode=...`.

Se **nenhum** evento aparecer, a regra udev não está casando. Diagnostique com:
```bash
udevadm monitor --property --subsystem-match=power_supply
```
e confira o `ACTION` e o `ATTR{type}` reais antes de ajustar a regra. Não
adivinhe a correção — use a saída do monitor.

- [ ] **Step 7: Commit**

```bash
git add aucc-rs/src/setup.rs install/70-avell-hid.rules install/install.sh
git commit -m "feat(restore): unificar unit e disparar restore na troca de energia"
```

---

### Task 7: Medir a corrida com o EC e mitigar se necessário

O spec deixou em aberto se o EC apaga o backlight **no** instante do evento de
energia ou alguns milissegundos **depois**. No segundo caso, o restore
disparado pelo udev é sobrescrito pelo EC e o bug persiste de forma
intermitente.

**Files:**
- Modify: `aucc-rs/src/setup.rs` e `install/install.sh` — somente se a medição indicar corrida.

**Interfaces:**
- Consumes: a unit da Task 6.
- Produces: nenhuma interface nova.

- [ ] **Step 1: Medir**

Aplique uma cor bem visível e cicle a energia dez vezes, anotando o resultado
de cada ciclo:

```bash
sudo aucc --color red --brightness 4
journalctl -f -u aucc-restore.service
```

Para cada ciclo (desconectar, esperar ~3s, reconectar, esperar ~3s), registre:
o teclado ficou aceso em vermelho, apagou e voltou, ou apagou e ficou apagado?

- [ ] **Step 2: Decidir com base no dado**

- **Acende e permanece nos 10 ciclos** → sem corrida. Marque esta task como
  concluída sem alterar código e siga para a Task 8.
- **Apaga e volta (piscada)** → comportamento aceitável, mas registre no
  README que uma piscada breve ocorre na troca de energia.
- **Fica apagado em qualquer ciclo** → há corrida. Aplique o Step 3.

- [ ] **Step 3: Mitigar apenas se o Step 2 indicou corrida**

Reaplicar após um atraso curto cobre o caso do EC apagar depois do nosso
comando. Adicione à seção `[Service]` da unit, em `setup.rs` e em
`install/install.sh`:

```
ExecStart=/usr/local/bin/aucc --restore
ExecStart=/bin/sleep 1
ExecStart=/usr/local/bin/aucc --restore
```

Repita a medição do Step 1. Se ainda falhar, **pare e reavalie** em vez de
aumentar o sleep às cegas: mais de duas tentativas falhas indicam que o gatilho
está errado, não o timing.

- [ ] **Step 4: Commit (só se houve mudança)**

```bash
git add aucc-rs/src/setup.rs install/install.sh
git commit -m "fix(restore): reaplicar estado apos atraso na troca de energia"
```

---

### Task 8: Verificação end-to-end e documentação

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: tudo das tasks anteriores.
- Produces: nada.

- [ ] **Step 1: Reinstalar a partir do binário final**

```bash
cd aucc-rs && cargo build --release && sudo ./target/release/aucc --install
```

- [ ] **Step 2: Executar o roteiro de verificação do spec**

Registre o resultado observado de cada item — não marque como concluído sem
ter olhado para o teclado:

1. `sudo aucc --color blue --brightness 4`, desconectar o cabo → mantém azul.
2. Reconectar o cabo → mantém azul.
3. `sudo reboot` com o cabo desconectado → acende azul após o boot.
4. `systemctl suspend` e retomar, na bateria e na AC → mantém azul.
5. `sudo aucc --off`, depois desconectar/reconectar o cabo e reiniciar → o
   teclado permanece apagado (a escolha "apagado" também é uma escolha).
6. `sudo aucc --style rainbow --speed 3`, reiniciar → o efeito rainbow volta.

- [ ] **Step 3: Documentar no README**

Na seção de persistência, substitua o texto que fala apenas da lightbar por:

```markdown
## Persistência

A configuração do teclado e da lightbar é salva em `/etc/aucc/keyboard.conf` e
`/etc/aucc/lightbar.conf` a cada comando aplicado, e restaurada automaticamente
por `aucc-restore.service` em três momentos:

- no boot, quando os dispositivos ITE aparecem;
- ao conectar ou desconectar o cabo de força;
- ao retomar de suspend/hibernate.

O terceiro caso existe porque o EC do notebook apaga a iluminação do teclado em
eventos de energia, ignorando o que está gravado na EEPROM do controlador. Sem
o serviço, o teclado ficaria apagado ao ligar na bateria.

`--save` continua gravando na EEPROM do teclado e é independente disso: ajuda a
iluminação a aparecer mais cedo no boot, antes do serviço rodar.

Para ativar: `sudo aucc --install`.
```

- [ ] **Step 4: Rodar a suíte completa e o clippy**

Run: `cd aucc-rs && cargo test 2>&1 | tail -20 && cargo clippy --all-targets 2>&1 | tail -20`
Expected: todos os testes passando; nenhum warning novo do clippy.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: documentar persistencia do teclado entre eventos de energia"
```
