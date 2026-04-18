use crate::audio::effects::AudioEffect;
use crate::audio::{self, AudioCmd};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction as LayoutDir, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        block::BorderType, Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, List, ListItem,
        ListState, Paragraph,
    },
    Frame, Terminal,
};

use crate::config::{self, LightbarConfig};
use crate::keyboard::{
    colors::get_color,
    effects::{effect_payload, Effect, WaveDirection},
    KeyboardDevice,
};
use crate::lightbar;
use crate::power::{self, PowerProfile};
use crate::setup;
use crate::telemetry;

// ── Screen enum ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Main,
    Dashboard,
    PowerProfile,
    SolidColor,
    HAltA,
    HAltB,
    VAltA,
    VAltB,
    Effect,
    WaveDir,
    Reactive,
    EffColor,
    Brightness,
    Speed,
    LbColor,
    LbBrightness,
    AudioSync,
    AudioDevice,
}

// ── App state ─────────────────────────────────────────────────────────────────

/// Captured LED state before audio sync — restored on disable.
struct LedSnapshot {
    r: u8,
    g: u8,
    b: u8,
    brightness: u8,
}

pub struct AppState {
    pub screen: Screen,
    pub list_state: ListState,
    pub brightness: u8, // 1–4
    pub speed: u8,      // 1–10
    pub color_a: usize, // index into COLORS
    pub color_b: usize,
    pub effect: Effect,
    pub eff_variant: Option<char>,
    pub wave_dir: WaveDirection,
    pub reactive: bool,
    pub save_eeprom: bool,
    pub lb_brightness: u8, // 0x00–0x64
    pub _mode: Option<&'static str>,
    pub status: String,
    pub lb_path: Option<PathBuf>,
    pub telemetry: Option<telemetry::Telemetry>,
    pub power_profile: Option<PowerProfile>,
    // Channel to send USB commands from TUI thread
    usb_tx: mpsc::Sender<UsbCmd>,
    // Channel to receive error messages from USB worker thread
    error_rx: mpsc::Receiver<String>,
    // Telemetry history for sparkline charts
    pub telemetry_history: telemetry::TelemetryHistory,
    // Network counters
    prev_net_rx: u64,
    prev_net_tx: u64,
    // Audio sync state
    audio_tx: Option<mpsc::Sender<AudioCmd>>,
    audio_enabled: bool,
    audio_effect: AudioEffect,
    audio_devices: Vec<(String, String)>,
    audio_device_idx: Option<usize>,
    audio_snapshot: Option<LedSnapshot>,
}

// ── USB command channel ───────────────────────────────────────────────────────

pub enum UsbCmd {
    MonoColor {
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
        save: bool,
    },
    AltColor {
        ra: u8,
        ga: u8,
        ba: u8,
        rb: u8,
        gb: u8,
        bb: u8,
        brightness: u8,
        horizontal: bool,
        save: bool,
    },
    Effect([u8; 8]),
    Disable,
    LbColor {
        path: PathBuf,
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    },
    LbDisable {
        path: PathBuf,
    },
    /// Audio-reactive color + brightness (save: false always — never wear EEPROM at 30fps).
    AudioColor {
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    },
    /// Audio-reactive brightness only (fastest path — 1 USB transfer).
    AudioBrightness(u8),
}

const COLOR_NAMES: &[&str] = &[
    "red",
    "green",
    "blue",
    "teal",
    "purple",
    "pink",
    "yellow",
    "white",
    "orange",
    "olive",
    "maroon",
    "brown",
    "gray",
    "skyblue",
    "navy",
    "crimson",
    "darkgreen",
    "lightgreen",
    "gold",
    "violet",
];

const EFFECTS: &[(&str, Effect)] = &[
    ("rainbow", Effect::Rainbow),
    ("wave", Effect::Wave),
    ("breathing", Effect::Breathing),
    ("marquee", Effect::Marquee),
    ("random", Effect::Random),
    ("ripple", Effect::Ripple),
    ("reactiveripple", Effect::ReactiveRipple),
    ("aurora", Effect::Aurora),
    ("fireworks", Effect::Fireworks),
    ("raindrop", Effect::Raindrop),
];

const WAVE_DIRS: &[(&str, WaveDirection)] = &[
    ("Direita  →", WaveDirection::Right),
    ("Esquerda ←", WaveDirection::Left),
    ("Cima     ↑", WaveDirection::Up),
    ("Baixo    ↓", WaveDirection::Down),
];

const EFF_COLORS: &[(&str, Option<char>)] = &[
    ("Rainbow (padrão)", None),
    ("Red", Some('r')),
    ("Orange", Some('o')),
    ("Yellow", Some('y')),
    ("Green", Some('g')),
    ("Blue", Some('b')),
    ("Teal", Some('t')),
    ("Purple", Some('p')),
];

const LB_BRIGHTNESS: &[(u8, &str)] = &[
    (0x00, "0  — apagado"),
    (0x19, "25%"),
    (0x32, "50% (padrão)"),
    (0x4b, "75%"),
    (0x64, "100% — máximo"),
];

impl AppState {
    fn new(
        lb_path: Option<PathBuf>,
        usb_tx: mpsc::Sender<UsbCmd>,
        error_rx: mpsc::Receiver<String>,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        // Load persisted save_eeprom state from config
        let saved_config = config::load();
        Self {
            screen: Screen::Main,
            list_state,
            brightness: 3,
            speed: 5,
            color_a: 0,
            color_b: 2,
            effect: Effect::Rainbow,
            eff_variant: None,
            wave_dir: WaveDirection::Right,
            reactive: false,
            save_eeprom: saved_config.save_eeprom,
            lb_brightness: 0x32,
            _mode: None,
            status: String::new(),
            lb_path,
            telemetry: None,
            power_profile: power::detect_profile(),
            usb_tx,
            error_rx,
            telemetry_history: telemetry::TelemetryHistory::new(),
            prev_net_rx: 0,
            prev_net_tx: 0,
            audio_tx: None,
            audio_enabled: false,
            audio_effect: AudioEffect::Pulse,
            audio_devices: Vec::new(),
            audio_device_idx: None,
            audio_snapshot: None,
        }
    }

    fn selected(&self) -> usize {
        self.list_state.selected().unwrap_or(0)
    }

    fn set_selected(&mut self, i: usize) {
        self.list_state.select(Some(i));
    }

    fn item_count(&self) -> usize {
        match self.screen {
            Screen::Main => main_items_dynamic(self.save_eeprom).len(),
            Screen::Dashboard => 0,
            Screen::PowerProfile => PowerProfile::all().len(),
            Screen::SolidColor
            | Screen::HAltA
            | Screen::HAltB
            | Screen::VAltA
            | Screen::VAltB
            | Screen::LbColor => COLOR_NAMES.len(),
            Screen::Effect => EFFECTS.len(),
            Screen::WaveDir => WAVE_DIRS.len(),
            Screen::Reactive => 2, // Sim / Não
            Screen::EffColor => EFF_COLORS.len(),
            Screen::Brightness => 4,
            Screen::Speed => 10,
            Screen::LbBrightness => LB_BRIGHTNESS.len(),
            // 5 effects + sep + device btn + sep + toggle = 9
            Screen::AudioSync => 9,
            Screen::AudioDevice => self.audio_devices.len().max(1),
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        let n = self.item_count();
        if n == 0 {
            return;
        }
        let cur = self.selected() as i32;
        let mut next = ((cur + delta).rem_euclid(n as i32)) as usize;
        // Skip separator lines in main menu and AudioSync screen
        if self.screen == Screen::Main {
            let items = main_items_dynamic(self.save_eeprom);
            for _ in 0..n {
                if items[next].starts_with('─') {
                    next = ((next as i32 + delta).rem_euclid(n as i32)) as usize;
                } else {
                    break;
                }
            }
        } else if self.screen == Screen::AudioSync {
            // Separators are at indices 5 and 7
            for _ in 0..n {
                if next == 5 || next == 7 {
                    next = ((next as i32 + delta).rem_euclid(n as i32)) as usize;
                } else {
                    break;
                }
            }
        }
        self.set_selected(next);
        self.live_preview();
    }

    fn live_preview(&mut self) {
        let idx = self.selected();
        match self.screen {
            Screen::SolidColor | Screen::HAltA | Screen::VAltA => {
                let (r, g, b) = color_at(idx);
                let _ = self.usb_tx.send(UsbCmd::MonoColor {
                    r,
                    g,
                    b,
                    brightness: self.brightness,
                    save: false,
                });
            }
            Screen::HAltB => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(idx);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra,
                    ga,
                    ba,
                    rb,
                    gb,
                    bb,
                    brightness: self.brightness,
                    horizontal: true,
                    save: false,
                });
            }
            Screen::VAltB => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(idx);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra,
                    ga,
                    ba,
                    rb,
                    gb,
                    bb,
                    brightness: self.brightness,
                    horizontal: false,
                    save: false,
                });
            }
            Screen::LbColor => {
                if let Some(path) = &self.lb_path.clone() {
                    let (r, g, b) = color_at(idx);
                    let _ = self.usb_tx.send(UsbCmd::LbColor {
                        path: path.clone(),
                        r,
                        g,
                        b,
                        brightness: self.lb_brightness,
                    });
                }
            }
            _ => {}
        }
    }

    fn apply_final(&mut self) {
        let save = self.save_eeprom;
        match self._mode {
            Some("effect") => {
                let payload = effect_payload(
                    self.effect,
                    self.speed,
                    self.brightness,
                    self.eff_variant,
                    self.wave_dir,
                    self.reactive,
                    save,
                );
                let _ = self.usb_tx.send(UsbCmd::Effect(payload));
                let reactive_label = if self.reactive { " (reativo)" } else { "" };
                self.status = format!("Efeito '{}'{reactive_label} aplicado!", self.effect);
                self._mode = None;
            }
            Some("h_alt") => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(self.color_b);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra,
                    ga,
                    ba,
                    rb,
                    gb,
                    bb,
                    brightness: self.brightness,
                    horizontal: true,
                    save,
                });
                self.status = format!(
                    "Alternado H: {} / {} aplicado.",
                    COLOR_NAMES[self.color_a], COLOR_NAMES[self.color_b]
                );
                self._mode = None;
            }
            Some("v_alt") => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(self.color_b);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra,
                    ga,
                    ba,
                    rb,
                    gb,
                    bb,
                    brightness: self.brightness,
                    horizontal: false,
                    save,
                });
                self.status = format!(
                    "Alternado V: {} / {} aplicado.",
                    COLOR_NAMES[self.color_a], COLOR_NAMES[self.color_b]
                );
                self._mode = None;
            }
            _ => {
                let (r, g, b) = color_at(self.color_a);
                let _ = self.usb_tx.send(UsbCmd::MonoColor {
                    r,
                    g,
                    b,
                    brightness: self.brightness,
                    save,
                });
                self.status = format!("Cor '{}' aplicada!", COLOR_NAMES[self.color_a]);
            }
        }
    }

    /// Returns true if should exit.
    fn confirm(&mut self) -> bool {
        let idx = self.selected();
        match &self.screen {
            Screen::Main => {
                let items = main_items_dynamic(self.save_eeprom);
                let sel = items[idx];
                match sel {
                    "Dashboard (telemetria)" => {
                        let t = telemetry::collect_with_network(
                            self.prev_net_rx,
                            self.prev_net_tx,
                            1.0,
                        );
                        telemetry::update_history(&mut self.telemetry_history, &t);
                        if let Some(ref net) = t.network {
                            self.prev_net_rx = net.rx_bytes;
                            self.prev_net_tx = net.tx_bytes;
                        }
                        self.telemetry = Some(t);
                        self.go_to(Screen::Dashboard, 0);
                    }
                    "Perfis de Energia (TDP)" => {
                        let prof_idx = self
                            .power_profile
                            .and_then(|p| PowerProfile::all().iter().position(|x| *x == p))
                            .unwrap_or(1);
                        self.go_to(Screen::PowerProfile, prof_idx);
                    }
                    "Sair" => return true,
                    "Teclado — Desligar" => {
                        let _ = self.usb_tx.send(UsbCmd::Disable);
                        self.status = "Teclado desligado.".into();
                    }
                    "Lightbar — Desligar" => {
                        if let Some(path) = self.lb_path.clone() {
                            let _ = self.usb_tx.send(UsbCmd::LbDisable { path });
                            let _ = config::save(&LightbarConfig {
                                enabled: false,
                                ..Default::default()
                            });
                            self.status = "Lightbar desligado.".into();
                        } else {
                            self.status = "Lightbar não detectado.".into();
                        }
                    }
                    "Lightbar — Cor" => {
                        if self.lb_path.is_some() {
                            self.go_to(Screen::LbColor, 0);
                            self.live_preview();
                        } else {
                            self.status = "Lightbar não detectado.".into();
                        }
                    }
                    "Lightbar — Igual ao teclado" => {
                        if let Some(path) = self.lb_path.clone() {
                            let (r, g, b) = color_at(self.color_a);
                            let _ = self.usb_tx.send(UsbCmd::LbColor {
                                path,
                                r,
                                g,
                                b,
                                brightness: self.lb_brightness,
                            });
                            let _ = config::save(&LightbarConfig {
                                enabled: true,
                                r,
                                g,
                                b,
                                brightness: self.lb_brightness,
                                save_eeprom: self.save_eeprom,
                            });
                            self.status = format!(
                                "Lightbar: {} (igual ao teclado)",
                                COLOR_NAMES[self.color_a]
                            );
                        } else {
                            self.status = "Lightbar não detectado.".into();
                        }
                    }
                    s if s.starts_with("💾 Persistir") => {
                        self.save_eeprom = !self.save_eeprom;
                        // Persist the save_eeprom preference itself
                        if let Ok(cfg) = config::load_file() {
                            let _ = config::save(&config::LightbarConfig {
                                save_eeprom: self.save_eeprom,
                                ..cfg
                            });
                        }
                        let label = if self.save_eeprom {
                            "SIM ✔ — persistirá após reboot"
                        } else {
                            "NÃO — temporário"
                        };
                        self.status = format!("Persistir: {label}");
                    }
                    "⚙ Instalar udev + binários" => {
                        let exe = std::env::current_exe().unwrap_or_default();
                        match setup::install(&exe, setup::INSTALL_UI_BIN_PATH) {
                            Ok(msg) => self.status = format!("✔ {msg}"),
                            Err(e) => self.status = format!("✗ {e}"),
                        }
                    }
                    "⚙ Desinstalar" => match setup::uninstall(setup::INSTALL_UI_BIN_PATH) {
                        Ok(msg) => self.status = format!("✔ {msg}"),
                        Err(e) => self.status = format!("✗ {e}"),
                    },
                    "Teclado — Cor Sólida" => {
                        self._mode = None;
                        self.go_to(Screen::SolidColor, 0);
                        self.live_preview();
                    }
                    "Teclado — Alternado Horizontal" => {
                        self._mode = Some("h_alt");
                        self.go_to(Screen::HAltA, 0);
                        self.live_preview();
                    }
                    "Teclado — Alternado Vertical" => {
                        self._mode = Some("v_alt");
                        self.go_to(Screen::VAltA, 0);
                        self.live_preview();
                    }
                    "Teclado — Efeito" => {
                        self._mode = Some("effect");
                        self.go_to(Screen::Effect, 0);
                    }
                    "🎵 Audio Sync" => {
                        if self.audio_devices.is_empty() {
                            audio::capture::setup_audio_env_for_root();
                            self.audio_devices = audio::capture::with_stderr_suppressed(|| {
                                audio::capture::list_input_devices()
                            });
                        }
                        self.go_to(Screen::AudioSync, 0);
                    }
                    _ => {}
                }
            }
            Screen::PowerProfile => {
                let profile = PowerProfile::all()[idx];
                match power::apply_profile(profile) {
                    Ok(_) => {
                        self.power_profile = Some(profile);
                        self.status = format!("Perfil '{}' aplicado!", profile.name());
                    }
                    Err(e) => {
                        self.status = format!("Erro (requer root?): {e}");
                    }
                }
                self.go_to(Screen::Main, 0);
            }
            Screen::Dashboard => {
                // Refresh telemetry on Enter
                let t = telemetry::collect_with_network(self.prev_net_rx, self.prev_net_tx, 1.0);
                telemetry::update_history(&mut self.telemetry_history, &t);
                if let Some(ref net) = t.network {
                    self.prev_net_rx = net.rx_bytes;
                    self.prev_net_tx = net.tx_bytes;
                }
                self.telemetry = Some(t);
            }
            Screen::SolidColor => {
                self.color_a = idx;
                self.go_to(Screen::Brightness, self.brightness as usize - 1);
            }
            Screen::HAltA => {
                self.color_a = idx;
                self.go_to(Screen::HAltB, 0);
                self.live_preview();
            }
            Screen::HAltB => {
                self.color_b = idx;
                self.go_to(Screen::Brightness, self.brightness as usize - 1);
            }
            Screen::VAltA => {
                self.color_a = idx;
                self.go_to(Screen::VAltB, 0);
                self.live_preview();
            }
            Screen::VAltB => {
                self.color_b = idx;
                self.go_to(Screen::Brightness, self.brightness as usize - 1);
            }
            Screen::Effect => {
                self.effect = EFFECTS[idx].1;
                self.eff_variant = None;
                if self.effect == Effect::Wave {
                    let dir_idx = WAVE_DIRS
                        .iter()
                        .position(|(_, d)| *d == self.wave_dir)
                        .unwrap_or(0);
                    self.go_to(Screen::WaveDir, dir_idx);
                } else if self.effect.supports_color_variant() {
                    self.go_to(Screen::Reactive, 0);
                } else {
                    self.go_to(Screen::Brightness, self.brightness as usize - 1);
                }
            }
            Screen::WaveDir => {
                self.wave_dir = WAVE_DIRS[idx].1;
                self.go_to(Screen::Reactive, 0);
            }
            Screen::Reactive => {
                self.reactive = idx == 0; // 0 = Sim, 1 = Não
                if self.effect.supports_color_variant() {
                    self.go_to(Screen::EffColor, 0);
                } else {
                    self.go_to(Screen::Brightness, self.brightness as usize - 1);
                }
            }
            Screen::EffColor => {
                self.eff_variant = EFF_COLORS[idx].1;
                self.go_to(Screen::Brightness, self.brightness as usize - 1);
            }
            Screen::Brightness => {
                self.brightness = idx as u8 + 1;
                if self._mode == Some("effect") {
                    self.go_to(Screen::Speed, self.speed as usize - 1);
                } else {
                    self.apply_final();
                    self.go_to(Screen::Main, 0);
                }
            }
            Screen::Speed => {
                self.speed = idx as u8 + 1;
                self.apply_final();
                self.go_to(Screen::Main, 0);
            }
            Screen::LbColor => {
                self.color_a = idx;
                let lb_idx = LB_BRIGHTNESS
                    .iter()
                    .position(|(v, _)| *v == self.lb_brightness)
                    .unwrap_or(2);
                self.go_to(Screen::LbBrightness, lb_idx);
            }
            Screen::LbBrightness => {
                self.lb_brightness = LB_BRIGHTNESS[idx].0;
                if let Some(path) = self.lb_path.clone() {
                    let (r, g, b) = color_at(self.color_a);
                    let _ = self.usb_tx.send(UsbCmd::LbColor {
                        path,
                        r,
                        g,
                        b,
                        brightness: self.lb_brightness,
                    });
                    let _ = config::save(&LightbarConfig {
                        enabled: true,
                        r,
                        g,
                        b,
                        brightness: self.lb_brightness,
                        save_eeprom: self.save_eeprom,
                    });
                    self.status = format!("Lightbar: {} aplicado!", COLOR_NAMES[self.color_a]);
                }
                self.go_to(Screen::Main, 0);
            }
            Screen::AudioSync => {
                match idx {
                    0..=4 => {
                        const AUDIO_EFFECTS: [AudioEffect; 5] = [
                            AudioEffect::Pulse,
                            AudioEffect::ColorShift,
                            AudioEffect::Wave,
                            AudioEffect::Breathe,
                            AudioEffect::Random,
                        ];
                        self.audio_effect = AUDIO_EFFECTS[idx];
                        self.status = format!("Efeito: {}", self.audio_effect.label());
                        if self.audio_enabled {
                            if let Some(ref tx) = self.audio_tx {
                                let _ = tx.send(AudioCmd::SetEffect(self.audio_effect));
                            }
                        }
                    }
                    5 | 7 => {} // separators — no action
                    6 => {
                        self.audio_devices = audio::capture::with_stderr_suppressed(|| {
                            audio::capture::list_input_devices()
                        });
                        self.go_to(Screen::AudioDevice, 0);
                    }
                    8 => {
                        if self.audio_enabled {
                            self.disable_audio_sync();
                        } else {
                            self.enable_audio_sync();
                        }
                    }
                    _ => {}
                }
            }
            Screen::AudioDevice => {
                if !self.audio_devices.is_empty() {
                    let (_, desc) = &self.audio_devices[idx];
                    self.status = format!("Fonte: {desc}");
                    self.audio_device_idx = Some(idx);
                } else {
                    self.audio_device_idx = None;
                    self.status = "Usando dispositivo padrão.".into();
                }
                self.go_to(Screen::AudioSync, 0);
            }
        }
        false
    }

    fn go_back(&mut self) -> bool {
        let (dest, idx) = match self.screen {
            Screen::Dashboard
            | Screen::PowerProfile
            | Screen::SolidColor
            | Screen::HAltA
            | Screen::VAltA
            | Screen::Effect
            | Screen::LbColor => (Some(Screen::Main), 0),
            Screen::HAltB => (Some(Screen::HAltA), 0),
            Screen::VAltB => (Some(Screen::VAltA), 0),
            Screen::WaveDir => (Some(Screen::Effect), 0),
            Screen::Reactive => (Some(Screen::Effect), 0),
            Screen::EffColor => (Some(Screen::Reactive), 0),
            Screen::Brightness => (Some(Screen::Main), 0),
            Screen::Speed => (Some(Screen::Brightness), self.brightness as usize - 1),
            Screen::LbBrightness => (Some(Screen::LbColor), 0),
            Screen::AudioSync => (Some(Screen::Main), 0),
            Screen::AudioDevice => (Some(Screen::AudioSync), 0),
            Screen::Main => return true,
        };
        if let Some(s) = dest {
            self.go_to(s, idx);
        }
        false
    }

    fn go_to(&mut self, screen: Screen, idx: usize) {
        self.screen = screen;
        self.set_selected(idx);
    }

    fn enable_audio_sync(&mut self) {
        let (r, g, b) = color_at(self.color_a);
        self.audio_snapshot = Some(LedSnapshot {
            r,
            g,
            b,
            brightness: self.brightness,
        });

        let (audio_tx, audio_rx) = mpsc::channel::<AudioCmd>();
        let lb_path = self.lb_path.clone();
        audio::spawn_audio_engine(audio_rx, self.usb_tx.clone(), lb_path);

        let device_name = self
            .audio_device_idx
            .and_then(|i| self.audio_devices.get(i))
            .map(|(name, _)| name.clone());

        let _ = audio_tx.send(AudioCmd::Enable {
            device_name,
            effect: self.audio_effect,
        });

        self.audio_tx = Some(audio_tx);
        self.audio_enabled = true;
        self.status = "🎵 Audio sync ativado!".into();
    }

    fn disable_audio_sync(&mut self) {
        if let Some(ref tx) = self.audio_tx {
            let _ = tx.send(AudioCmd::Disable);
        }
        self.audio_tx = None;
        self.audio_enabled = false;

        if let Some(snap) = self.audio_snapshot.take() {
            let _ = self.usb_tx.send(UsbCmd::MonoColor {
                r: snap.r,
                g: snap.g,
                b: snap.b,
                brightness: snap.brightness,
                save: false,
            });
            if let Some(path) = self.lb_path.clone() {
                let _ = self.usb_tx.send(UsbCmd::LbColor {
                    path,
                    r: snap.r,
                    g: snap.g,
                    b: snap.b,
                    brightness: 0x32,
                });
            }
        }
        self.status = "Audio sync desativado.".into();
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn color_at(idx: usize) -> (u8, u8, u8) {
    let name = COLOR_NAMES.get(idx).copied().unwrap_or("red");
    get_color(name).unwrap_or((0xFF, 0, 0))
}

fn main_items_dynamic(save: bool) -> Vec<&'static str> {
    let save_label: &'static str = if save {
        "💾 Persistir: SIM ✔"
    } else {
        "💾 Persistir: NÃO"
    };
    vec![
        "Dashboard (telemetria)",
        "Perfis de Energia (TDP)",
        "──────────────────",
        "Teclado — Cor Sólida",
        "Teclado — Alternado Horizontal",
        "Teclado — Alternado Vertical",
        "Teclado — Efeito",
        "Teclado — Desligar",
        "──────────────────",
        "🎵 Audio Sync",
        "──────────────────",
        "Lightbar — Cor",
        "Lightbar — Igual ao teclado",
        "Lightbar — Desligar",
        "──────────────────",
        save_label,
        "──────────────────",
        "⚙ Instalar udev + binários",
        "⚙ Desinstalar",
        "──────────────────",
        "Sair",
    ]
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(f: &mut Frame, state: &mut AppState) {
    let area = f.area();

    // Dashboard uses a full-screen grid layout
    if state.screen == Screen::Dashboard {
        render_dashboard_grid(f, state);
    } else {
        // Traditional list-based layout for other screens
        let chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(area);

        // Title
        let title = screen_title(state);
        let title_par = Paragraph::new(title)
            .block(Block::default().borders(Borders::BOTTOM))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(title_par, chunks[0]);

        // Body
        let items = build_list_items(state);
        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, chunks[1], &mut state.list_state);

        // Status / help bar
        let help = if state.status.is_empty() {
            "↑↓ navegar  Enter confirmar  ESC voltar  q sair"
        } else {
            &state.status
        };
        let status_par = Paragraph::new(help)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::TOP));
        f.render_widget(status_par, chunks[2]);
    }
}

/// Render the dashboard as a grid of card widgets
fn render_dashboard_grid(f: &mut Frame, state: &mut AppState) {
    let area = f.area();

    // Main vertical split: header | content | footer
    let main_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(3), // header bar
            Constraint::Min(10),   // main content area
            Constraint::Length(2), // footer with aggregated stats
        ])
        .split(area);

    // ── Header bar ──
    render_header_bar(f, state, main_chunks[0]);

    // ── Content area: multi-row grid ──
    let content_chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(9), // Row 1: CPU | GPU
            Constraint::Length(8), // Row 2: RAM | Network
            Constraint::Length(9), // Row 3: Processes | NVMe+Battery
            Constraint::Min(10),   // Row 4: line charts
        ])
        .split(main_chunks[1]);

    // Row 1: CPU | GPU
    let row1 = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_chunks[0]);
    render_cpu_card(f, state, row1[0]);
    render_gpu_card(f, state, row1[1]);

    // Row 2: RAM | Network
    let row2 = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_chunks[1]);
    render_ram_card(f, state, row2[0]);
    render_network_card(f, state, row2[1]);

    // Row 3: Processes | NVMe+Battery
    let row3 = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_chunks[2]);
    render_processes_card(f, state, row3[0]);
    render_nvme_battery_card(f, state, row3[1]);

    // Row 4: CPU chart | Network chart
    let row4 = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_chunks[3]);
    render_cpu_chart(f, state, row4[0]);
    render_network_chart(f, state, row4[1]);

    // ── Footer bar with aggregated stats ──
    render_footer_bar(f, state, main_chunks[2]);
}

/// Helper to create a bordered block with rounded corners and colored border
fn card_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(format!(" {title} "))
}

fn screen_title(state: &AppState) -> String {
    let lb = if state.lb_path.is_some() {
        ""
    } else {
        "  (não detectado)"
    };
    match state.screen {
        Screen::Main => "AUCC — Menu Principal".into(),
        Screen::Dashboard => "📊 Dashboard — Telemetria  (Enter=atualizar  ESC=voltar)".into(),
        Screen::PowerProfile => {
            let cur = state.power_profile.map(|p| p.name()).unwrap_or("?");
            format!("⚡ Perfis de Energia  (atual: {cur})")
        }
        Screen::SolidColor => "🎨 Teclado — Cor Sólida".into(),
        Screen::HAltA => "↔ Teclado — Alternado H, Cor A".into(),
        Screen::HAltB => format!(
            "↔ Teclado — Alternado H, Cor B  (A={})",
            COLOR_NAMES[state.color_a]
        ),
        Screen::VAltA => "↕ Teclado — Alternado V, Cor A".into(),
        Screen::VAltB => format!(
            "↕ Teclado — Alternado V, Cor B  (A={})",
            COLOR_NAMES[state.color_a]
        ),
        Screen::Effect => "✨ Teclado — Efeito".into(),
        Screen::WaveDir => "🌊 Teclado — Direção do Wave".into(),
        Screen::Reactive => "💥 Teclado — Modo Reativo?".into(),
        Screen::EffColor => format!("✨ Teclado — Cor do Efeito ({})", state.effect),
        Screen::Brightness => "💡 Teclado — Brilho".into(),
        Screen::Speed => "⚡ Teclado — Velocidade".into(),
        Screen::LbColor => format!("🔆 Lightbar — Cor{lb}"),
        Screen::LbBrightness => "🔆 Lightbar — Intensidade".into(),
        Screen::AudioSync => "🎵 Audio Sync — LEDs reagem ao áudio".into(),
        Screen::AudioDevice => "🎵 Selecionar fonte de áudio".into(),
    }
}

fn build_list_items(state: &AppState) -> Vec<ListItem<'static>> {
    match state.screen {
        Screen::Main => main_items_dynamic(state.save_eeprom)
            .into_iter()
            .map(|s| ListItem::new(s))
            .collect(),
        Screen::Dashboard => vec![], // rendered separately as Paragraph
        Screen::PowerProfile => PowerProfile::all()
            .iter()
            .map(|p| {
                let marker = if state.power_profile == Some(*p) {
                    " ✔"
                } else {
                    ""
                };
                let (pl1, pl2) = p.limits_uw();
                let label = format!(
                    "{}{marker}  (PL1={} W / PL2={} W)",
                    p.name(),
                    pl1 / 1_000_000,
                    pl2 / 1_000_000
                );
                ListItem::new(label)
            })
            .collect(),
        Screen::SolidColor
        | Screen::HAltA
        | Screen::HAltB
        | Screen::VAltA
        | Screen::VAltB
        | Screen::LbColor => COLOR_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let (r, g, b) = color_at(i);
                let swatch = format!("  {name}");
                ListItem::new(Line::from(vec![
                    Span::styled("███ ", Style::default().fg(Color::Rgb(r, g, b))),
                    Span::raw(swatch),
                ]))
            })
            .collect(),
        Screen::Effect => EFFECTS
            .iter()
            .map(|(name, _)| ListItem::new(*name))
            .collect(),
        Screen::WaveDir => WAVE_DIRS
            .iter()
            .map(|(label, _)| ListItem::new(*label))
            .collect(),
        Screen::Reactive => vec![
            ListItem::new("Sim — reativo (responde a teclas)"),
            ListItem::new("Não — normal"),
        ],
        Screen::EffColor => EFF_COLORS
            .iter()
            .map(|(label, _)| ListItem::new(*label))
            .collect(),
        Screen::Brightness => (1u8..=4)
            .map(|b| {
                ListItem::new(match b {
                    1 => "1 — mínimo".to_string(),
                    4 => "4 — máximo".to_string(),
                    n => n.to_string(),
                })
            })
            .collect(),
        Screen::Speed => (1u8..=10)
            .map(|s| {
                ListItem::new(match s {
                    1 => "1 — mais rápido".to_string(),
                    10 => "10 — mais lento".to_string(),
                    n => n.to_string(),
                })
            })
            .collect(),
        Screen::LbBrightness => LB_BRIGHTNESS
            .iter()
            .map(|(_, label)| ListItem::new(*label))
            .collect(),
        Screen::AudioSync => {
            const EFFECTS: [AudioEffect; 5] = [
                AudioEffect::Pulse,
                AudioEffect::ColorShift,
                AudioEffect::Wave,
                AudioEffect::Breathe,
                AudioEffect::Random,
            ];
            let mut items: Vec<ListItem> = EFFECTS
                .iter()
                .map(|e| {
                    let marker = if *e == state.audio_effect { " ✔" } else { "" };
                    ListItem::new(format!("{}{marker}", e.label()))
                })
                .collect();
            items.push(ListItem::new("──────────────────"));
            items.push(ListItem::new("🔊 Selecionar fonte de áudio"));
            items.push(ListItem::new("──────────────────"));
            if state.audio_enabled {
                items.push(ListItem::new("⏹ Desativar Audio Sync"));
            } else {
                items.push(ListItem::new("▶ Ativar Audio Sync"));
            }
            items
        }
        Screen::AudioDevice => {
            if state.audio_devices.is_empty() {
                vec![ListItem::new("Nenhum dispositivo encontrado")]
            } else {
                state
                    .audio_devices
                    .iter()
                    .map(|(_name, desc)| ListItem::new(desc.clone()))
                    .collect()
            }
        }
    }
}

// ── Dashboard rendering ────────────────────────────────────────────────────────

/// Format uptime seconds into human-readable string.
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{d}d {h}h {m}m", d = days, h = hours, m = mins)
    } else if hours > 0 {
        format!("{h}h {m}m", h = hours, m = mins)
    } else {
        format!("{m}m", m = mins)
    }
}

/// Format KB/s into human-readable throughput.
fn format_throughput(kbs: f32) -> String {
    if kbs >= 1024.0 {
        format!("{:.1} MB/s", kbs / 1024.0)
    } else if kbs >= 1.0 {
        format!("{:.0} KB/s", kbs)
    } else {
        format!("{} B/s", (kbs * 1024.0) as u64)
    }
}

/// Get color based on percentage value
fn pct_color(pct: f32, warn: f32, crit: f32) -> Color {
    if pct >= crit {
        Color::Red
    } else if pct >= warn {
        Color::Yellow
    } else {
        Color::Green
    }
}

// ── Header Bar ──

fn render_header_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        let p = Paragraph::new("  Sem dados — pressione Enter para coletar.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(p, area);
        return;
    };

    let prof = state.power_profile.map(|p| p.name()).unwrap_or("?");
    let title = Span::styled(
        " ◈ AUCC DASHBOARD ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let sys_info = Span::styled(
        format!(
            " uptime: {} │ load: {:.2} │ perfil: {}",
            format_uptime(t.system.uptime_secs),
            t.system.load_avg_1,
            prof
        ),
        Style::default().fg(Color::DarkGray),
    );
    let refresh_info = Span::styled("auto-refresh: 1s", Style::default().fg(Color::DarkGray));

    let line = Line::from(vec![title, sys_info, Span::raw("   "), refresh_info]);
    let par = Paragraph::new(line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(par, area);
}

// ── Footer Bar with aggregated stats ──

fn render_footer_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        let help = if state.status.is_empty() {
            " Enter=atualizar  ESC=voltar  q=sair  │  auto-refresh: 1s"
        } else {
            &state.status
        };
        f.render_widget(
            Paragraph::new(help)
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
            area,
        );
        return;
    };

    // Build aggregated stats like picomon
    let mut parts = Vec::new();

    // CPU
    let cpu_util = if t.cpu.temp_avg > 40.0 {
        (t.cpu.temp_avg - 40.0) / 60.0 * 100.0
    } else {
        0.0
    };
    parts.push(Span::styled(
        format!("CPU {:.0}%", cpu_util),
        Style::default().fg(Color::Cyan),
    ));
    parts.push(Span::styled(
        format!("│{:.0}°C", t.cpu.temp_max),
        Style::default().fg(Color::Red),
    ));

    // GPU
    if let Some(ref g) = t.gpu {
        parts.push(Span::raw("  "));
        parts.push(Span::styled(
            format!("GPU {}%", g.utilization_pct),
            Style::default().fg(Color::Magenta),
        ));
        parts.push(Span::styled(
            format!("{:.0}°C", g.temp_c),
            Style::default().fg(Color::Yellow),
        ));
    }

    // RAM
    parts.push(Span::raw("  "));
    parts.push(Span::styled(
        format!("RAM {:.0}%", t.ram.used_pct),
        Style::default().fg(Color::Green),
    ));

    // Battery
    if let Some(ref b) = t.battery {
        parts.push(Span::raw("  "));
        let bat_icon = if b.current_now_ma > 0 {
            "⚡"
        } else if b.current_now_ma < 0 {
            "↓"
        } else {
            "="
        };
        parts.push(Span::styled(
            format!("BAT {}%{}", b.capacity_pct, bat_icon),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Uptime
    parts.push(Span::raw("  "));
    parts.push(Span::styled(
        format!("UP {}", format_uptime(t.system.uptime_secs)),
        Style::default().fg(Color::DarkGray),
    ));

    // Help text
    parts.push(Span::raw("  │  "));
    parts.push(Span::styled(
        "Enter=atualizar  ESC=voltar  q=sair",
        Style::default().fg(Color::DarkGray),
    ));

    let line = Line::from(parts);
    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

// ── CPU Card ──

fn render_cpu_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let cpu_util = if t.cpu.temp_avg > 40.0 {
        (t.cpu.temp_avg - 40.0) / 60.0 * 100.0
    } else {
        0.0
    };
    let temp_pct = (t.cpu.temp_avg / 110.0 * 100.0).min(100.0);
    let color = pct_color(temp_pct, 68.0, 82.0);

    let freq_str = if t.cpu.freq_mhz > 0 {
        format!("{:.2} GHz", t.cpu.freq_mhz as f32 / 1000.0)
    } else {
        "—".into()
    };

    let cores_text = if !t.cpu.temps.is_empty() {
        t.cpu
            .temps
            .iter()
            .enumerate()
            .map(|(i, temp)| format!("C{i}:{temp:.0}°"))
            .collect::<Vec<_>>()
            .join("  ")
    } else {
        "—".into()
    };

    // Sparkline for CPU temp history
    let sparkline_data: Vec<u64> = if !state.telemetry_history.cpu_temp.is_empty() {
        let max_temp = state
            .telemetry_history
            .cpu_temp
            .iter()
            .cloned()
            .fold(0f32, f32::max)
            .ceil()
            .max(1.0);
        state
            .telemetry_history
            .cpu_temp
            .iter()
            .map(|v| (*v / max_temp * 10.0) as u64)
            .collect()
    } else {
        Vec::new()
    };

    let block = card_block("CPU", Color::Cyan);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_spark = !sparkline_data.is_empty();
    let constraints = if has_spark {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints(constraints)
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" FREQ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                freq_str,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                format!("{} cores", t.cpu.core_count),
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        chunks[0],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" TEMP ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0}°/{:.0}°", t.cpu.temp_avg, t.cpu.temp_max),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[1],
    );

    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(color))
            .percent(temp_pct.min(100.0) as u16)
            .label(format!("{:.0}°C", t.cpu.temp_avg)),
        chunks[1],
    );

    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(color))
            .percent(cpu_util.min(100.0) as u16)
            .label(format!("{:.0}% util", cpu_util)),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" CORES", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("  {cores_text}")),
        ])),
        chunks[3],
    );

    if has_spark {
        let spark = ratatui::widgets::Sparkline::default()
            .data(&sparkline_data)
            .style(Style::default().fg(Color::Red))
            .direction(ratatui::widgets::RenderDirection::LeftToRight);
        f.render_widget(spark, chunks[4]);
    }
}

// ── GPU Card ──

fn render_gpu_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let block = card_block("GPU", Color::Magenta);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(ref g) = t.gpu {
        let temp_pct = (g.temp_c as f32 / 100.0 * 100.0).min(100.0);
        let util_pct = g.utilization_pct as f32;
        let vram_pct = g.vram_used_mb as f32 / g.vram_total_mb.max(1) as f32 * 100.0;

        let temp_color = pct_color(temp_pct, 70.0, 85.0);
        let util_color = pct_color(util_pct, 75.0, 90.0);
        let vram_color = pct_color(vram_pct, 75.0, 90.0);

        let short_name = if g.name.len() > 25 {
            format!("{}...", &g.name[..22])
        } else {
            g.name.clone()
        };

        let chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" GPU  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    short_name,
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[0],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" TEMP ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}°C", g.temp_c),
                    Style::default().fg(temp_color).add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[1],
        );
        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(temp_color))
                .percent(temp_pct as u16),
            chunks[1],
        );

        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(util_color))
                .percent(g.utilization_pct as u16)
                .label(format!("{}% util", g.utilization_pct)),
            chunks[2],
        );

        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(vram_color))
                .percent(vram_pct as u16)
                .label(format!("{}/{} MB", g.vram_used_mb, g.vram_total_mb)),
            chunks[3],
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  GPU indisponível",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
    }
}

// ── RAM Card ──

fn render_ram_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let ram_pct = t.ram.used_pct;
    let ram_color = pct_color(ram_pct, 75.0, 90.0);

    // Sparkline data
    let spark_data: Vec<u64> = if !state.telemetry_history.ram_usage_pct.is_empty() {
        state
            .telemetry_history
            .ram_usage_pct
            .iter()
            .map(|v| (*v / 10.0) as u64)
            .collect()
    } else {
        Vec::new()
    };
    let has_spark = !spark_data.is_empty();

    let block = card_block("RAM", Color::Green);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_swap = t.ram.swap_total_mb > 0;
    let n_constraints = if has_swap {
        if has_spark {
            5
        } else {
            4
        }
    } else {
        if has_spark {
            4
        } else {
            3
        }
    };
    let constraints = vec![Constraint::Length(1); n_constraints];
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" MEM  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{} ({:.0}%)", t.ram.used_mb, t.ram.total_mb, ram_pct),
                Style::default().fg(ram_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} MB disp.", t.ram.available_mb),
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        chunks[idx],
    );
    idx += 1;

    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(ram_color))
            .percent(ram_pct.min(100.0) as u16)
            .label(format!("{:.0}% usada", ram_pct)),
        chunks[idx],
    );
    idx += 1;

    if has_swap {
        let swap_pct = t.ram.swap_pct;
        let swap_color = pct_color(swap_pct, 50.0, 80.0);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" SWAP ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(
                        "{}/{} MB ({:.0}%)",
                        t.ram.swap_used_mb, t.ram.swap_total_mb, swap_pct
                    ),
                    Style::default().fg(swap_color),
                ),
            ])),
            chunks[idx],
        );
        idx += 1;
        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(swap_color))
                .percent(swap_pct.min(100.0) as u16),
            chunks[idx],
        );
        idx += 1;
    }

    if has_spark {
        let spark = ratatui::widgets::Sparkline::default()
            .data(&spark_data)
            .style(Style::default().fg(Color::Green))
            .direction(ratatui::widgets::RenderDirection::LeftToRight);
        f.render_widget(spark, chunks[idx]);
    }
}

// ── Network Card ──

fn render_network_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let block = card_block("REDE", Color::Blue);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(ref n) = t.network {
        let rx_str = format_throughput(n.rx_kbs);
        let tx_str = format_throughput(n.tx_kbs);
        let total_rx_mb = n.rx_bytes as f32 / 1024.0 / 1024.0;
        let total_tx_mb = n.tx_bytes as f32 / 1024.0 / 1024.0;

        let chunks = Layout::default()
            .direction(LayoutDir::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" IF   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &n.interface,
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[0],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ↓ RX ", Style::default().fg(Color::Green)),
                Span::styled(
                    format!("{:>10}", rx_str),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled("↑ TX ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{:>10}", tx_str),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[1],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" TOT  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("↓{:.0}MB", total_rx_mb),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("    "),
                Span::styled(
                    format!("↑{:.0}MB", total_tx_mb),
                    Style::default().fg(Color::Cyan),
                ),
            ])),
            chunks[2],
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Sem dados de rede",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
    }
}

// ── NVMe Card ──

// ── NVMe + Battery Combined Card ──

fn render_nvme_battery_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let block = card_block("ARMAZENAMENTO + BATERIA", Color::Rgb(255, 165, 0));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let has_nvme = !t.nvme.is_empty();
    let has_bat = t.battery.is_some();

    // Calculate layout based on available content
    let nvme_lines = if has_nvme { t.nvme.len() * 2 } else { 0 };
    let bat_lines = if has_bat { 4 } else { 0 };
    let total_lines = nvme_lines + bat_lines;
    let constraints = vec![Constraint::Length(1); total_lines.max(1)];
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0;

    // NVMe section
    if has_nvme {
        for nvme in &t.nvme {
            let temp_pct = (nvme.temp_c / 70.0 * 100.0).min(100.0);
            let color = pct_color(temp_pct, 55.0, 70.0);
            let label = if nvme.critical_temp_c > 0.0 {
                format!("{:.0}°C (crít:{:.0}°C)", nvme.temp_c, nvme.critical_temp_c)
            } else {
                format!("{:.0}°C", nvme.temp_c)
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(" {}  ", nvme.hwmon),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        label,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ])),
                chunks[idx],
            );
            idx += 1;
            f.render_widget(
                Gauge::default()
                    .gauge_style(Style::default().fg(color))
                    .percent(temp_pct as u16),
                chunks[idx],
            );
            idx += 1;
        }
    } else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  NVMe não detectado",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[idx],
        );
        idx += 1;
    }

    // Battery section
    if let Some(ref b) = t.battery {
        let cap_pct = b.capacity_pct as f32;
        let color = pct_color(cap_pct, 20.0, 40.0);
        let direction = if b.current_now_ma > 0 {
            "⚡ Carregando"
        } else if b.current_now_ma < 0 {
            "↓ Descarregando"
        } else {
            "= Na tomada"
        };
        let time_str = b
            .time_remaining_min
            .map(|m| {
                if m >= 60 {
                    format!("{h}h {m}min", h = m / 60, m = m % 60)
                } else {
                    format!("{m} min", m = m)
                }
            })
            .unwrap_or_else(|| "—".into());
        let power_str = b
            .power_w
            .map(|w| format!("{w:.1}W"))
            .unwrap_or_else(|| "—".into());

        // Separator line
        f.render_widget(
            Paragraph::new(Span::styled(
                " ─────────────────────────",
                Style::default().fg(Color::DarkGray),
            )),
            chunks[idx],
        );
        idx += 1;

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" BAT  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}%  {}", b.capacity_pct, direction),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ])),
            chunks[idx],
        );
        idx += 1;

        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(color))
                .percent(cap_pct as u16)
                .label(format!("{}%  {}  {}", b.capacity_pct, time_str, power_str)),
            chunks[idx],
        );
        idx += 1;

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" CARGA", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}/{} mAh", b.charge_now_mah, b.charge_full_mah),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("ciclos: {}", b.cycle_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            chunks[idx],
        );
        idx += 1;
    } else {
        if has_nvme {
            f.render_widget(
                Paragraph::new(Span::styled(
                    " ─────────────────────────",
                    Style::default().fg(Color::DarkGray),
                )),
                chunks[idx],
            );
            idx += 1;
        }
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Bateria não detectada",
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[idx],
        );
    }
}

// ── Top Processes Card ──

fn render_processes_card(f: &mut Frame, state: &AppState, area: Rect) {
    let Some(ref t) = state.telemetry else {
        return;
    };

    let block = card_block("PROCESSOS", Color::White);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let procs = &t.system.top_processes;
    if procs.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Nenhum processo",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
        return;
    }

    // Header
    let header = Line::from(vec![
        Span::styled(
            " PID    ",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " NOME           ",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " CPU%  ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " MEM%",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut items = vec![header];
    for p in procs {
        let cpu_color = if p.cpu_pct >= 80.0 {
            Color::Red
        } else if p.cpu_pct >= 40.0 {
            Color::Yellow
        } else {
            Color::White
        };
        let name = if p.name.len() > 14 {
            format!("{}...", &p.name[..11])
        } else {
            p.name.clone()
        };
        items.push(Line::from(vec![
            Span::styled(
                format!(" {:<6}", p.pid),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!(" {:<14}", name), Style::default().fg(Color::White)),
            Span::styled(
                format!(" {:>5.1}", p.cpu_pct),
                Style::default().fg(cpu_color),
            ),
            Span::styled(
                format!(" {:>4.1}", p.mem_pct),
                Style::default().fg(Color::Green),
            ),
        ]));
    }

    let list = Paragraph::new(items);
    f.render_widget(list, inner);
}

// ── Network Line Chart ──

fn render_network_chart(f: &mut Frame, state: &AppState, area: Rect) {
    let h = &state.telemetry_history;

    let block = card_block("HISTÓRICO REDE", Color::Blue);
    let inner = block.inner(area);
    f.render_widget(block.clone(), area);

    if h.net_rx_kbs.is_empty() || h.net_rx_kbs.len() < 2 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Aguardando dados...",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
        return;
    }

    let rx_data: Vec<(f64, f64)> = h
        .net_rx_kbs
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect();
    let tx_data: Vec<(f64, f64)> = h
        .net_tx_kbs
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect();

    let max_rx = h
        .net_rx_kbs
        .iter()
        .cloned()
        .fold(0f32, f32::max)
        .ceil()
        .max(1.0);
    let max_tx = h
        .net_tx_kbs
        .iter()
        .cloned()
        .fold(0f32, f32::max)
        .ceil()
        .max(1.0);
    let y_max = max_rx.max(max_tx) * 1.1;

    let x_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, h.net_rx_kbs.len().max(1) as f64]);

    let y_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, y_max as f64]);

    let rx_dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(Color::Green))
        .graph_type(GraphType::Line)
        .data(&rx_data);

    let tx_dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(Color::Cyan))
        .graph_type(GraphType::Line)
        .data(&tx_data);

    let chart = Chart::new(vec![rx_dataset, tx_dataset])
        .block(block)
        .x_axis(x_axis)
        .y_axis(y_axis);
    f.render_widget(chart, area);
}

// ── CPU Line Chart ──

fn render_cpu_chart(f: &mut Frame, state: &AppState, area: Rect) {
    let h = &state.telemetry_history;

    let block = card_block("HISTÓRICO CPU", Color::Cyan);

    if h.cpu_temp.is_empty() || h.cpu_temp.len() < 2 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Aguardando dados...",
                Style::default().fg(Color::DarkGray),
            )))
            .alignment(ratatui::layout::Alignment::Center),
            area,
        );
        return;
    }

    let temp_data: Vec<(f64, f64)> = h
        .cpu_temp
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect();
    let util_data: Vec<(f64, f64)> = h
        .cpu_utilization
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect();

    let max_temp = h
        .cpu_temp
        .iter()
        .cloned()
        .fold(0f32, f32::max)
        .ceil()
        .max(1.0);

    let x_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, h.cpu_temp.len().max(1) as f64]);
    let y_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, max_temp as f64 * 1.1]);

    let temp_dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(Color::Red))
        .graph_type(GraphType::Line)
        .data(&temp_data);
    let util_dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(Color::Yellow))
        .graph_type(GraphType::Line)
        .data(&util_data);

    let chart = Chart::new(vec![temp_dataset, util_dataset])
        .block(block)
        .x_axis(x_axis)
        .y_axis(y_axis);
    f.render_widget(chart, area);
}

// ── RAM Line Chart ──

fn render_ram_chart(f: &mut Frame, state: &AppState, area: Rect) {
    let h = &state.telemetry_history;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(" ◈ HISTÓRICO RAM ");

    if h.ram_usage_pct.is_empty() || h.ram_usage_pct.len() < 2 {
        let text = vec![Line::from(vec![Span::styled(
            "  Aguardando dados...",
            Style::default().fg(Color::DarkGray),
        )])];
        let par = Paragraph::new(text).block(block);
        f.render_widget(par, area);
        return;
    }

    let ram_data: Vec<(f64, f64)> = h
        .ram_usage_pct
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect();

    let max_ram = h
        .ram_usage_pct
        .iter()
        .cloned()
        .fold(0f32, f32::max)
        .ceil()
        .max(1.0);

    let x_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, h.ram_usage_pct.len().max(1) as f64]);

    let y_axis = Axis::default()
        .style(Style::default().fg(Color::DarkGray))
        .bounds([0.0, max_ram as f64 * 1.1]);

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .style(Style::default().fg(Color::Green))
        .graph_type(GraphType::Line)
        .data(&ram_data);

    let chart = Chart::new(vec![dataset])
        .block(block)
        .x_axis(x_axis)
        .y_axis(y_axis);
    f.render_widget(chart, area);
}

// ── USB worker thread ─────────────────────────────────────────────────────────

fn spawn_usb_worker(rx: mpsc::Receiver<UsbCmd>, error_tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        let dev = KeyboardDevice::open().ok();
        for cmd in rx {
            if let Some(ref d) = dev {
                let result = match cmd {
                    UsbCmd::MonoColor {
                        r,
                        g,
                        b,
                        brightness,
                        save,
                    } => d.apply_mono_color(r, g, b, brightness, save),
                    UsbCmd::AltColor {
                        ra,
                        ga,
                        ba,
                        rb,
                        gb,
                        bb,
                        brightness,
                        horizontal,
                        save,
                    } => d.apply_alt_color(ra, ga, ba, rb, gb, bb, brightness, horizontal, save),
                    UsbCmd::Effect(payload) => d.apply_effect(&payload),
                    UsbCmd::Disable => d.disable(),
                    UsbCmd::LbColor {
                        path,
                        r,
                        g,
                        b,
                        brightness,
                    } => lightbar::apply_color(&path, r, g, b, brightness)
                        .map_err(|_| rusb::Error::Io),
                    UsbCmd::LbDisable { path } => {
                        lightbar::disable(&path).map_err(|_| rusb::Error::Io)
                    }
                    UsbCmd::AudioColor {
                        r,
                        g,
                        b,
                        brightness,
                    } => {
                        // save: false — never persist audio-reactive state to EEPROM
                        d.apply_mono_color(r, g, b, brightness, false)
                    }
                    UsbCmd::AudioBrightness(level) => d.set_brightness(level),
                };
                if let Err(e) = result {
                    let _ = error_tx.send(format!("USB error: {e}"));
                }
            } else {
                let _ = error_tx.send("USB device not available".to_string());
            }
        }
    });
}

// ── Main run loop ─────────────────────────────────────────────────────────────

pub fn run() -> io::Result<()> {
    // Detect lightbar
    let lb_path = lightbar::find_hidraw_path().or_else(|| {
        let _ = lightbar::ensure_bound();
        thread::sleep(Duration::from_millis(300));
        lightbar::find_hidraw_path()
    });

    let lb_status = lb_path
        .as_ref()
        .map(|p| format!("\x1b[32m{}\x1b[0m", p.display()))
        .unwrap_or_else(|| "\x1b[33mnão detectado\x1b[0m".into());
    println!("Teclado: \x1b[32mOK\x1b[0m  |  Lightbar: {lb_status}");

    // USB worker channel
    let (tx, rx) = mpsc::channel::<UsbCmd>();
    let (error_tx, error_rx) = mpsc::channel::<String>();
    spawn_usb_worker(rx, error_tx);

    let mut state = AppState::new(lb_path, tx, error_rx);

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut last_dashboard_tick = std::time::Instant::now();

    loop {
        // Auto-refresh telemetry on Dashboard every 1s
        if state.screen == Screen::Dashboard
            && last_dashboard_tick.elapsed() >= Duration::from_secs(1)
        {
            let t = telemetry::collect_with_network(state.prev_net_rx, state.prev_net_tx, 1.0);
            telemetry::update_history(&mut state.telemetry_history, &t);
            if let Some(ref net) = t.network {
                state.prev_net_rx = net.rx_bytes;
                state.prev_net_tx = net.tx_bytes;
            }
            state.telemetry = Some(t);
            last_dashboard_tick = std::time::Instant::now();
        }

        terminal.draw(|f| render(f, &mut state))?;

        // Drain any pending USB errors
        while let Ok(err) = state.error_rx.try_recv() {
            state.status = err;
        }

        let poll_ms = if state.screen == Screen::Dashboard {
            500
        } else {
            50
        };
        if event::poll(Duration::from_millis(poll_ms))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                state.status.clear();
                match key.code {
                    KeyCode::Up => state.move_cursor(-1),
                    KeyCode::Down => state.move_cursor(1),
                    KeyCode::Enter => {
                        if state.confirm() {
                            break;
                        }
                    }
                    KeyCode::Esc | KeyCode::Backspace => {
                        if state.go_back() {
                            break;
                        }
                    }
                    KeyCode::Char('q') => break,
                    _ => {}
                }
            }
        }
    }

    // Stop audio sync before restoring terminal
    if state.audio_enabled {
        state.disable_audio_sync();
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    println!("\x1b[0mAté logo! 👋\x1b[0m");
    Ok(())
}
