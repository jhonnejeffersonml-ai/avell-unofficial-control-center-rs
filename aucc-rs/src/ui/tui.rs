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
    layout::{Constraint, Direction as LayoutDir, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::config::{self, LightbarConfig};
use crate::keyboard::{
    KeyboardDevice,
    colors::get_color,
    effects::{Effect, WaveDirection, effect_payload},
};
use crate::lightbar;
use crate::telemetry;
use crate::power::{self, PowerProfile};

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
    EffColor,
    Brightness,
    Speed,
    LbColor,
    LbBrightness,
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct AppState {
    pub screen: Screen,
    pub list_state: ListState,
    pub brightness: u8,        // 1–4
    pub speed: u8,             // 1–10
    pub color_a: usize,        // index into COLORS
    pub color_b: usize,
    pub effect: Effect,
    pub eff_variant: Option<char>,
    pub wave_dir: WaveDirection,
    pub save_eeprom: bool,
    pub lb_brightness: u8,     // 0x00–0x64
    pub _mode: Option<&'static str>,
    pub status: String,
    pub lb_path: Option<PathBuf>,
    pub telemetry: Option<telemetry::Telemetry>,
    pub power_profile: Option<PowerProfile>,
    // Channel to send USB commands from TUI thread
    usb_tx: mpsc::Sender<UsbCmd>,
}

// ── USB command channel ───────────────────────────────────────────────────────

enum UsbCmd {
    MonoColor { r: u8, g: u8, b: u8, brightness: u8, save: bool },
    AltColor { ra: u8, ga: u8, ba: u8, rb: u8, gb: u8, bb: u8, brightness: u8, horizontal: bool, save: bool },
    Effect([u8; 8]),
    Disable,
    LbColor { path: PathBuf, r: u8, g: u8, b: u8, brightness: u8 },
    LbDisable { path: PathBuf },
}

const COLOR_NAMES: &[&str] = &[
    "red","green","blue","teal","purple","pink","yellow","white",
    "orange","olive","maroon","brown","gray","skyblue","navy",
    "crimson","darkgreen","lightgreen","gold","violet",
];

const EFFECTS: &[(&str, Effect)] = &[
    ("rainbow",        Effect::Rainbow),
    ("wave",           Effect::Wave),
    ("breathing",      Effect::Breathing),
    ("marquee",        Effect::Marquee),
    ("reactive",       Effect::Reactive),
    ("ripple",         Effect::Ripple),
    ("reactiveripple", Effect::ReactiveRipple),
    ("aurora",         Effect::Aurora),
    ("reactiveaurora", Effect::ReactiveAurora),
    ("fireworks",      Effect::Fireworks),
    ("raindrop",       Effect::Raindrop),
    ("random",         Effect::Random),
];

const WAVE_DIRS: &[(&str, WaveDirection)] = &[
    ("Direita  →", WaveDirection::Right),
    ("Esquerda ←", WaveDirection::Left),
    ("Cima     ↑", WaveDirection::Up),
    ("Baixo    ↓", WaveDirection::Down),
];

const EFF_COLORS: &[(&str, Option<char>)] = &[
    ("Rainbow (padrão)", None),
    ("Red",    Some('r')),
    ("Orange", Some('o')),
    ("Yellow", Some('y')),
    ("Green",  Some('g')),
    ("Blue",   Some('b')),
    ("Teal",   Some('t')),
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
    fn new(lb_path: Option<PathBuf>, usb_tx: mpsc::Sender<UsbCmd>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
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
            save_eeprom: false,
            lb_brightness: 0x32,
            _mode: None,
            status: String::new(),
            lb_path,
            telemetry: None,
            power_profile: power::detect_profile(),
            usb_tx,
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
            Screen::Main         => main_items_dynamic(self.save_eeprom).len(),
            Screen::Dashboard    => 0,
            Screen::PowerProfile => PowerProfile::all().len(),
            Screen::SolidColor
            | Screen::HAltA | Screen::HAltB
            | Screen::VAltA | Screen::VAltB
            | Screen::LbColor    => COLOR_NAMES.len(),
            Screen::Effect       => EFFECTS.len(),
            Screen::WaveDir      => WAVE_DIRS.len(),
            Screen::EffColor     => EFF_COLORS.len(),
            Screen::Brightness   => 4,
            Screen::Speed        => 10,
            Screen::LbBrightness => LB_BRIGHTNESS.len(),
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        let n = self.item_count();
        if n == 0 { return; }
        let cur = self.selected() as i32;
        let mut next = ((cur + delta).rem_euclid(n as i32)) as usize;
        // Skip separator lines in main menu
        if self.screen == Screen::Main {
            let items = main_items_dynamic(self.save_eeprom);
            for _ in 0..n {
                if items[next].starts_with('─') {
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
                    r, g, b, brightness: self.brightness, save: false,
                });
            }
            Screen::HAltB => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(idx);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra, ga, ba, rb, gb, bb,
                    brightness: self.brightness, horizontal: true, save: false,
                });
            }
            Screen::VAltB => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(idx);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra, ga, ba, rb, gb, bb,
                    brightness: self.brightness, horizontal: false, save: false,
                });
            }
            Screen::LbColor => {
                if let Some(path) = &self.lb_path.clone() {
                    let (r, g, b) = color_at(idx);
                    let _ = self.usb_tx.send(UsbCmd::LbColor {
                        path: path.clone(), r, g, b, brightness: self.lb_brightness,
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
                    self.effect, self.speed, self.brightness,
                    self.eff_variant, self.wave_dir, save,
                );
                let _ = self.usb_tx.send(UsbCmd::Effect(payload));
                self.status = format!("Efeito '{}' aplicado!", self.effect);
                self._mode = None;
            }
            Some("h_alt") => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(self.color_b);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra, ga, ba, rb, gb, bb,
                    brightness: self.brightness, horizontal: true, save,
                });
                self.status = format!("Alternado H: {} / {} aplicado.", COLOR_NAMES[self.color_a], COLOR_NAMES[self.color_b]);
                self._mode = None;
            }
            Some("v_alt") => {
                let (ra, ga, ba) = color_at(self.color_a);
                let (rb, gb, bb) = color_at(self.color_b);
                let _ = self.usb_tx.send(UsbCmd::AltColor {
                    ra, ga, ba, rb, gb, bb,
                    brightness: self.brightness, horizontal: false, save,
                });
                self.status = format!("Alternado V: {} / {} aplicado.", COLOR_NAMES[self.color_a], COLOR_NAMES[self.color_b]);
                self._mode = None;
            }
            _ => {
                let (r, g, b) = color_at(self.color_a);
                let _ = self.usb_tx.send(UsbCmd::MonoColor { r, g, b, brightness: self.brightness, save });
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
                        self.telemetry = Some(telemetry::collect());
                        self.go_to(Screen::Dashboard, 0);
                    }
                    "Perfis de Energia (TDP)" => {
                        let prof_idx = self.power_profile
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
                            let _ = config::save(&LightbarConfig { enabled: false, ..Default::default() });
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
                                path, r, g, b, brightness: self.lb_brightness,
                            });
                            let _ = config::save(&LightbarConfig { enabled: true, r, g, b, brightness: self.lb_brightness });
                            self.status = format!("Lightbar: {} (igual ao teclado)", COLOR_NAMES[self.color_a]);
                        } else {
                            self.status = "Lightbar não detectado.".into();
                        }
                    }
                    s if s.starts_with("💾 Persistir") => {
                        self.save_eeprom = !self.save_eeprom;
                        let label = if self.save_eeprom { "SIM ✔ — persistirá após reboot" } else { "NÃO — temporário" };
                        self.status = format!("Persistir: {label}");
                    }
                    "Teclado — Cor Sólida"          => { self._mode = None;        self.go_to(Screen::SolidColor, 0); self.live_preview(); }
                    "Teclado — Alternado Horizontal" => { self._mode = Some("h_alt"); self.go_to(Screen::HAltA, 0); self.live_preview(); }
                    "Teclado — Alternado Vertical"   => { self._mode = Some("v_alt"); self.go_to(Screen::VAltA, 0); self.live_preview(); }
                    "Teclado — Efeito"               => { self._mode = Some("effect"); self.go_to(Screen::Effect, 0); }
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
                self.telemetry = Some(telemetry::collect());
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
                    let dir_idx = WAVE_DIRS.iter().position(|(_, d)| *d == self.wave_dir).unwrap_or(0);
                    self.go_to(Screen::WaveDir, dir_idx);
                } else if self.effect.supports_color_variant() {
                    self.go_to(Screen::EffColor, 0);
                } else {
                    self.go_to(Screen::Brightness, self.brightness as usize - 1);
                }
            }
            Screen::WaveDir => {
                self.wave_dir = WAVE_DIRS[idx].1;
                self.go_to(Screen::Brightness, self.brightness as usize - 1);
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
                let lb_idx = LB_BRIGHTNESS.iter().position(|(v, _)| *v == self.lb_brightness).unwrap_or(2);
                self.go_to(Screen::LbBrightness, lb_idx);
            }
            Screen::LbBrightness => {
                self.lb_brightness = LB_BRIGHTNESS[idx].0;
                if let Some(path) = self.lb_path.clone() {
                    let (r, g, b) = color_at(self.color_a);
                    let _ = self.usb_tx.send(UsbCmd::LbColor {
                        path, r, g, b, brightness: self.lb_brightness,
                    });
                    let _ = config::save(&LightbarConfig { enabled: true, r, g, b, brightness: self.lb_brightness });
                    self.status = format!("Lightbar: {} aplicado!", COLOR_NAMES[self.color_a]);
                }
                self.go_to(Screen::Main, 0);
            }
        }
        false
    }

    fn go_back(&mut self) -> bool {
        let (dest, idx) = match self.screen {
            Screen::Dashboard | Screen::PowerProfile
            | Screen::SolidColor | Screen::HAltA | Screen::VAltA
            | Screen::Effect | Screen::LbColor => (Some(Screen::Main), 0),
            Screen::HAltB    => (Some(Screen::HAltA), 0),
            Screen::VAltB    => (Some(Screen::VAltA), 0),
            Screen::WaveDir  => (Some(Screen::Effect), 0),
            Screen::EffColor => (Some(Screen::Effect), 0),
            Screen::Brightness => (Some(Screen::Main), 0),
            Screen::Speed    => (Some(Screen::Brightness), self.brightness as usize - 1),
            Screen::LbBrightness => (Some(Screen::LbColor), 0),
            Screen::Main     => return true,
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
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn color_at(idx: usize) -> (u8, u8, u8) {
    let name = COLOR_NAMES.get(idx).copied().unwrap_or("red");
    get_color(name).unwrap_or((0xFF, 0, 0))
}

fn main_items_dynamic(save: bool) -> Vec<&'static str> {
    let save_label: &'static str = if save { "💾 Persistir: SIM ✔" } else { "💾 Persistir: NÃO" };
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
        "Lightbar — Cor",
        "Lightbar — Igual ao teclado",
        "Lightbar — Desligar",
        "──────────────────",
        save_label,
        "Sair",
    ]
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render(f: &mut Frame, state: &mut AppState) {
    let area = f.area();

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
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(title_par, chunks[0]);

    // Body — Dashboard is a Paragraph, everything else is a List
    if state.screen == Screen::Dashboard {
        let text = build_dashboard_text(state);
        let dash = Paragraph::new(text)
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(dash, chunks[1]);
    } else {
        let items = build_list_items(state);
        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, chunks[1], &mut state.list_state);
    }

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

fn screen_title(state: &AppState) -> String {
    let lb = if state.lb_path.is_some() { "" } else { "  (não detectado)" };
    match state.screen {
        Screen::Main         => "AUCC — Menu Principal".into(),
        Screen::Dashboard    => "📊 Dashboard — Telemetria  (Enter=atualizar  ESC=voltar)".into(),
        Screen::PowerProfile => {
            let cur = state.power_profile.map(|p| p.name()).unwrap_or("?");
            format!("⚡ Perfis de Energia  (atual: {cur})")
        }
        Screen::SolidColor   => "🎨 Teclado — Cor Sólida".into(),
        Screen::HAltA        => "↔ Teclado — Alternado H, Cor A".into(),
        Screen::HAltB        => format!("↔ Teclado — Alternado H, Cor B  (A={})", COLOR_NAMES[state.color_a]),
        Screen::VAltA        => "↕ Teclado — Alternado V, Cor A".into(),
        Screen::VAltB        => format!("↕ Teclado — Alternado V, Cor B  (A={})", COLOR_NAMES[state.color_a]),
        Screen::Effect       => "✨ Teclado — Efeito".into(),
        Screen::WaveDir      => "🌊 Teclado — Direção do Wave".into(),
        Screen::EffColor     => format!("✨ Teclado — Cor do Efeito ({})", state.effect),
        Screen::Brightness   => "💡 Teclado — Brilho".into(),
        Screen::Speed        => "⚡ Teclado — Velocidade".into(),
        Screen::LbColor      => format!("🔆 Lightbar — Cor{lb}"),
        Screen::LbBrightness => "🔆 Lightbar — Intensidade".into(),
    }
}

fn build_list_items(state: &AppState) -> Vec<ListItem<'static>> {
    match state.screen {
        Screen::Main => main_items_dynamic(state.save_eeprom)
            .into_iter()
            .map(|s| ListItem::new(s))
            .collect(),
        Screen::Dashboard => vec![],  // rendered separately as Paragraph
        Screen::PowerProfile => PowerProfile::all().iter().map(|p| {
            let marker = if state.power_profile == Some(*p) { " ✔" } else { "" };
            let (pl1, pl2) = p.limits_uw();
            let label = format!("{}{marker}  (PL1={} W / PL2={} W)",
                p.name(), pl1 / 1_000_000, pl2 / 1_000_000);
            ListItem::new(label)
        }).collect(),
        Screen::SolidColor | Screen::HAltA | Screen::HAltB
        | Screen::VAltA | Screen::VAltB | Screen::LbColor => {
            COLOR_NAMES.iter().enumerate().map(|(i, name)| {
                let (r, g, b) = color_at(i);
                let swatch = format!("  {name}");
                ListItem::new(Line::from(vec![
                    Span::styled("███ ", Style::default().fg(Color::Rgb(r, g, b))),
                    Span::raw(swatch),
                ]))
            }).collect()
        }
        Screen::Effect => EFFECTS.iter()
            .map(|(name, _)| ListItem::new(*name))
            .collect(),
        Screen::WaveDir => WAVE_DIRS.iter()
            .map(|(label, _)| ListItem::new(*label))
            .collect(),
        Screen::EffColor => EFF_COLORS.iter()
            .map(|(label, _)| ListItem::new(*label))
            .collect(),
        Screen::Brightness => (1u8..=4)
            .map(|b| ListItem::new(match b {
                1 => "1 — mínimo".to_string(),
                4 => "4 — máximo".to_string(),
                n => n.to_string(),
            }))
            .collect(),
        Screen::Speed => (1u8..=10)
            .map(|s| ListItem::new(match s {
                1  => "1 — mais rápido".to_string(),
                10 => "10 — mais lento".to_string(),
                n  => n.to_string(),
            }))
            .collect(),
        Screen::LbBrightness => LB_BRIGHTNESS.iter()
            .map(|(_, label)| ListItem::new(*label))
            .collect(),
    }
}

// ── Dashboard rendering ────────────────────────────────────────────────────────

fn bar(pct: f32, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f32).round() as usize;
    let filled = filled.min(width);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

fn build_dashboard_text(state: &AppState) -> Vec<Line<'static>> {
    let Some(ref t) = state.telemetry else {
        return vec![Line::from("  Sem dados — pressione Enter para coletar.")];
    };

    let mut lines: Vec<Line<'static>> = Vec::new();

    // CPU
    let cpu_color = if t.cpu.temp_max >= 90.0 { Color::Red }
        else if t.cpu.temp_max >= 75.0 { Color::Yellow }
        else { Color::Green };
    lines.push(Line::from(vec![
        Span::styled("  CPU  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("Avg {:.0}°C  Max {:.0}°C", t.cpu.temp_avg, t.cpu.temp_max),
            Style::default().fg(cpu_color),
        ),
    ]));

    // GPU
    if let Some(ref g) = t.gpu {
        let gpu_bar = bar(g.utilization_pct as f32, 20);
        let gpu_color = if g.temp_c >= 85 { Color::Red } else if g.temp_c >= 70 { Color::Yellow } else { Color::Green };
        let vram_pct = g.vram_used_mb as f32 / g.vram_total_mb as f32 * 100.0;
        lines.push(Line::from(vec![
            Span::styled("  GPU  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} {}% {}  {:.0}°C  VRAM {}/{}MB ({:.0}%)",
                    g.name, g.utilization_pct, gpu_bar, g.temp_c,
                    g.vram_used_mb, g.vram_total_mb, vram_pct),
                Style::default().fg(gpu_color),
            ),
        ]));
    } else {
        lines.push(Line::from("  GPU  — nvidia-smi não disponível".to_string()));
    }

    // RAM
    let ram_bar = bar(t.ram.used_pct, 20);
    let ram_color = if t.ram.used_pct >= 90.0 { Color::Red }
        else if t.ram.used_pct >= 75.0 { Color::Yellow }
        else { Color::Green };
    lines.push(Line::from(vec![
        Span::styled("  RAM  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{}/{} MB ({:.0}%) {}", t.ram.used_mb, t.ram.total_mb, t.ram.used_pct, ram_bar),
            Style::default().fg(ram_color),
        ),
    ]));

    // NVMe
    for nvme in &t.nvme {
        let nvme_color = if nvme.temp_c >= 70.0 { Color::Red }
            else if nvme.temp_c >= 55.0 { Color::Yellow }
            else { Color::Green };
        lines.push(Line::from(vec![
            Span::styled(format!("  NVMe ({})  ", nvme.hwmon),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:.0}°C", nvme.temp_c), Style::default().fg(nvme_color)),
        ]));
    }

    // Battery
    if let Some(ref b) = t.battery {
        lines.push(Line::raw(""));
        let bat_color = if b.capacity_pct < 20 { Color::Red }
            else if b.capacity_pct < 40 { Color::Yellow }
            else { Color::Green };
        let direction = if b.current_now_ma > 0 { "↑ carregando" }
            else if b.current_now_ma < 0 { "↓ descarregando" }
            else { "= AC" };
        lines.push(Line::from(vec![
            Span::styled("  Bateria  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}% {}  {}/{} mAh  {}  ciclos: {}",
                    b.capacity_pct, bar(b.capacity_pct as f32, 20),
                    b.charge_now_mah, b.charge_full_mah, direction, b.cycle_count),
                Style::default().fg(bat_color),
            ),
        ]));
    }

    // Power limits
    lines.push(Line::raw(""));
    if let Some(lim) = power::read_limits() {
        let prof_name = state.power_profile.map(|p| p.name()).unwrap_or("?");
        let gov = power::read_governor().unwrap_or_else(|| "?".into());
        let epp = power::read_epp().unwrap_or_else(|| "?".into());
        lines.push(Line::from(vec![
            Span::styled("  TDP    ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("PL1 {:.0} W / PL2 {:.0} W  ({})  gov={}  epp={}",
                    lim.pl1_w, lim.pl2_w, prof_name, gov, epp),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    lines
}

// ── USB worker thread ─────────────────────────────────────────────────────────

fn spawn_usb_worker(rx: mpsc::Receiver<UsbCmd>) {
    thread::spawn(move || {
        let dev = KeyboardDevice::open().ok();
        for cmd in rx {
            if let Some(ref d) = dev {
                match cmd {
                    UsbCmd::MonoColor { r, g, b, brightness, save } => {
                        let _ = d.apply_mono_color(r, g, b, brightness, save);
                    }
                    UsbCmd::AltColor { ra, ga, ba, rb, gb, bb, brightness, horizontal, save } => {
                        let _ = d.apply_alt_color(ra, ga, ba, rb, gb, bb, brightness, horizontal, save);
                    }
                    UsbCmd::Effect(payload) => {
                        let _ = d.apply_effect(&payload);
                    }
                    UsbCmd::Disable => {
                        let _ = d.disable();
                    }
                    UsbCmd::LbColor { path, r, g, b, brightness } => {
                        let _ = lightbar::apply_color(&path, r, g, b, brightness);
                    }
                    UsbCmd::LbDisable { path } => {
                        let _ = lightbar::disable(&path);
                    }
                }
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

    let lb_status = lb_path.as_ref()
        .map(|p| format!("\x1b[32m{}\x1b[0m", p.display()))
        .unwrap_or_else(|| "\x1b[33mnão detectado\x1b[0m".into());
    println!("Teclado: \x1b[32mOK\x1b[0m  |  Lightbar: {lb_status}");

    // USB worker channel
    let (tx, rx) = mpsc::channel::<UsbCmd>();
    spawn_usb_worker(rx);

    let mut state = AppState::new(lb_path, tx);

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
            state.telemetry = Some(telemetry::collect());
            last_dashboard_tick = std::time::Instant::now();
        }

        terminal.draw(|f| render(f, &mut state))?;
        state.status.clear();

        let poll_ms = if state.screen == Screen::Dashboard { 500 } else { 50 };
        if event::poll(Duration::from_millis(poll_ms))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up    => state.move_cursor(-1),
                    KeyCode::Down  => state.move_cursor(1),
                    KeyCode::Enter => {
                        if state.confirm() { break; }
                    }
                    KeyCode::Esc | KeyCode::Backspace => {
                        if state.go_back() { break; }
                    }
                    KeyCode::Char('q') => break,
                    _ => {}
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    println!("\x1b[0mAté logo! 👋\x1b[0m");
    Ok(())
}
