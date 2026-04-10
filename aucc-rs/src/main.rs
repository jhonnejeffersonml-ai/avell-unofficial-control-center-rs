use aucc_rs::config::{self, LightbarConfig};
use aucc_rs::keyboard::{KeyboardDevice, colors::get_color, effects::{Effect, WaveDirection, effect_payload}};
use aucc_rs::lightbar;
use aucc_rs::power::{self, PowerProfile};
use aucc_rs::setup;
use clap::{ArgGroup, Parser, ValueEnum};
use colored::Colorize;

/// Wave direction for the wave effect.
#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum DirectionArg {
    Right,
    Left,
    Up,
    Down,
}

impl DirectionArg {
    fn to_wave_dir(self) -> WaveDirection {
        match self {
            DirectionArg::Right => WaveDirection::Right,
            DirectionArg::Left => WaveDirection::Left,
            DirectionArg::Up => WaveDirection::Up,
            DirectionArg::Down => WaveDirection::Down,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    disable_version_flag = true,
    arg_required_else_help = true,
    about = "AUCC — Avell Unofficial Control Center",
    long_about = "AUCC — Avell Unofficial Control Center\n\
\n\
Controla RGB do teclado (ITE 8291) e lightbar frontal (ITE 8233)\n\
em laptops Avell / TongFang (ex: Storm 470).\n\
\n\
TECLADO (requer root / pkexec):\n  \
  Cores e efeitos são enviados ao EEPROM do hardware com --save.\n  \
  Sem --save, a configuração é temporária e se perde ao desligar.\n\
\n\
LIGHTBAR (não requer root se usuário está no grupo plugdev):\n  \
  O controlador ITE 8233 NÃO possui EEPROM — qualquer configuração\n  \
  é perdida ao reiniciar. Use --lb-color / --lb-disable para aplicar\n  \
  E persistir via software: o estado é salvo em /etc/aucc/lightbar.conf\n  \
  e restaurado automaticamente a cada boot via regra udev.\n\
\n\
TUI INTERATIVO:\n  \
  Execute 'pkexec aucc-ui' para a interface interativa completa.",
    after_help = "EXEMPLOS:\n\
\n  \
  # Teclado — cor sólida temporária\n  \
  sudo aucc --color red\n\
\n  \
  # Teclado — cor azul, brilho máximo, salvo no EEPROM (persiste após reboot)\n  \
  sudo aucc --color blue --brightness 4 --save\n\
\n  \
  # Teclado — efeito rainbow com velocidade 3\n  \
  sudo aucc --style rainbow --speed 3 --save\n\
\n  \
  # Teclado — efeito breathing verde\n  \
  sudo aucc --style breathingg --save\n\
\n  \
  # Teclado — alternado horizontal vermelho/azul\n  \
  sudo aucc -H red blue --brightness 3 --save\n\
\n  \
  # Lightbar — definir cor e brilho (salvo automaticamente)\n  \
  aucc --lb-color white --lb-brightness 50\n\
\n  \
  # Lightbar — desligar e salvar estado (permanece desligada após reboot)\n  \
  aucc --lb-disable\n\
\n  \
  # Lightbar — restaurar manualmente o estado salvo\n  \
  aucc --lb-restore\n\
\n  \
  # Ativar restauração automática da lightbar no boot (instala regra udev)\n  \
  sudo aucc --install\n\
\n  \
  # Remover a regra udev de restauração automática\n  \
  sudo aucc --uninstall\n\
\n  \
  # Perfil de energia silencioso\n  \
  sudo aucc --profile silent\n\
\n  \
  # Telemetria do sistema (sem root)\n  \
  aucc --telemetry\n\
\nCORES DISPONÍVEIS:\n  \
  red, green, blue, teal, purple, pink, yellow, white,\n  \
  orange, olive, maroon, brown, gray, skyblue, navy,\n  \
  crimson, darkgreen, lightgreen, gold, violet\n\
\nEFEITOS DISPONÍVEIS:\n  \
  rainbow, wave, breathing, marquee, reactive, ripple,\n  \
  reactiveripple, aurora, reactiveaurora, fireworks, raindrop, random\n  \
  (breathing/marquee/reactive/ripple aceitam sufixo de cor: breathingr, breathingg …)\n\
\nPERSISTÊNCIA DA LIGHTBAR:\n  \
  Config salva em: /etc/aucc/lightbar.conf\n  \
  Restaurada automaticamente via udev quando o dispositivo é detectado.\n  \
  Para ativar: sudo aucc --install"
)]
// Group is kept (without required) to document mutual exclusivity.
// arg_required_else_help = true on the command handles the no-args case.
#[command(group(ArgGroup::new("action").args(["color","h_alt","v_alt","style","disable","profile","tdp","telemetry","lb_restore","lb_disable","lb_color","version","install","uninstall"])))]
struct Cli {
    /// Cor sólida do teclado: red, green, blue, teal, purple, pink, yellow, white, orange …
    #[arg(short = 'c', long, value_name = "COR")]
    color: Option<String>,

    /// Alternado horizontal: duas cores lado a lado (ex: -H red blue)
    #[arg(short = 'H', long, num_args = 2, value_names = ["COR_A", "COR_B"])]
    h_alt: Option<Vec<String>>,

    /// Alternado vertical: duas cores cima/baixo (ex: -V red blue)
    #[arg(short = 'V', long, num_args = 2, value_names = ["COR_A", "COR_B"])]
    v_alt: Option<Vec<String>>,

    /// Efeito de iluminação: rainbow, wave, breathing[r|g|b|…], marquee, reactive, ripple …
    #[arg(short = 's', long, value_name = "EFEITO")]
    style: Option<String>,

    /// Desligar iluminação do teclado
    #[arg(short = 'd', long)]
    disable: bool,

    /// Perfil de energia: silent, balanced, turbo
    #[arg(short = 'p', long, value_name = "PERFIL")]
    profile: Option<String>,

    /// Definir TDP PL1 manualmente em watts (ex: 45)
    #[arg(long, value_name = "WATTS")]
    tdp: Option<f32>,

    /// Exibir telemetria do sistema (CPU, GPU, RAM, NVMe, bateria) — sem root
    #[arg(long)]
    telemetry: bool,

    /// Brilho do teclado: 1 (mínimo) a 4 (máximo)
    #[arg(short = 'b', long, value_parser = clap::value_parser!(u8).range(1..=4), default_value = "4", value_name = "1-4")]
    brightness: u8,

    /// Velocidade do efeito: 1 (mais rápido) a 10 (mais lento)
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=10), default_value = "5", value_name = "1-10")]
    speed: u8,

    /// Direção da onda (para --style wave): right, left, up, down
    #[arg(long, default_value = "right", value_name = "DIREÇÃO")]
    direction: DirectionArg,

    /// Salvar configuração no EEPROM do teclado (persiste após reboot) — apenas para teclado
    #[arg(long)]
    save: bool,

    /// Mostrar versão
    #[arg(long)]
    version: bool,

    // ── Lightbar ──────────────────────────────────────────────────────────────

    /// [Lightbar] Restaurar estado salvo de /etc/aucc/lightbar.conf
    ///
    /// Executado automaticamente via udev a cada boot quando o dispositivo
    /// ITE 8233 é detectado. Pode ser chamado manualmente sem root
    /// (basta estar no grupo plugdev).
    #[arg(long)]
    lb_restore: bool,

    /// [Lightbar] Desligar a lightbar e salvar o estado (permanece desligada após reboot)
    #[arg(long)]
    lb_disable: bool,

    /// [Lightbar] Definir cor sólida e salvar estado (persiste após reboot via udev)
    ///
    /// A configuração é salva em /etc/aucc/lightbar.conf e restaurada
    /// automaticamente no próximo boot. Use --lb-brightness para ajustar o brilho.
    #[arg(long, value_name = "COR")]
    lb_color: Option<String>,

    /// [Lightbar] Brilho da lightbar: 0 (apagado) a 100 (máximo), padrão 50
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100), default_value = "50", value_name = "0-100")]
    lb_brightness: u8,

    // ── Instalação ────────────────────────────────────────────────────────────

    /// Instalar regra udev e copiar o binário para /usr/local/bin/aucc
    ///
    /// Escreve /etc/udev/rules.d/70-avell-hid.rules, cria /etc/aucc/,
    /// recarrega o udev e copia este binário para /usr/local/bin/aucc.
    /// Após isso, a lightbar será restaurada automaticamente sempre que o
    /// sistema iniciar. Requer root (sudo).
    #[arg(long)]
    install: bool,

    /// Remover a regra udev de restauração automática da lightbar
    ///
    /// Remove /etc/udev/rules.d/70-avell-hid.rules e recarrega o udev.
    /// Requer root (sudo).
    #[arg(long)]
    uninstall: bool,
}

fn require_root() {
    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Requer root. Use: ./teclado  ou  sudo aucc");
        std::process::exit(1);
    }
}

fn main() {
    let cli = Cli::parse();

    // --version: print and exit (manual handler because clap's built-in -V
    // conflicts with the -V short used by --v-alt)
    if cli.version {
        println!("aucc-rs {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Telemetry needs no root — run before root check
    if cli.telemetry {
        print_telemetry();
        return;
    }

    // Lightbar restore/disable/color: no root needed (plugdev group via udev rules)
    if cli.lb_restore || cli.lb_disable || cli.lb_color.is_some() {
        if let Err(e) = run_lightbar(&cli) {
            eprintln!("{} {e}", "Erro lightbar:".red().bold());
            std::process::exit(1);
        }
        return;
    }

    // Install/uninstall udev rules: requires root
    if cli.install || cli.uninstall {
        require_root();
        let current_exe = std::env::current_exe().unwrap_or_default();
        let result = if cli.install {
            setup::install(&current_exe, setup::INSTALL_BIN_PATH)
        } else {
            setup::uninstall(setup::INSTALL_BIN_PATH)
        };
        match result {
            Ok(msg) => println!("{}", msg.green()),
            Err(e)  => { eprintln!("{} {e}", "Erro:".red().bold()); std::process::exit(1); }
        }
        return;
    }

    require_root();

    // Commands that don't need the USB keyboard device
    if cli.profile.is_some() || cli.tdp.is_some() {
        if let Err(e) = run_no_dev(&cli) {
            eprintln!("{} {e}", "Erro:".red().bold());
            std::process::exit(1);
        }
        return;
    }

    let dev = match KeyboardDevice::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{} {e}", "Dispositivo não encontrado:".red().bold());
            std::process::exit(1);
        }
    };

    if let Err(e) = run(&dev, &cli) {
        eprintln!("{} {e}", "Erro USB:".red().bold());
        std::process::exit(1);
    }
}

// ── run_lightbar ──────────────────────────────────────────────────────────────

fn run_lightbar(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let path = lightbar::find_hidraw_path()
        .ok_or("Lightbar não encontrado (ITE 8233 / 048d:7001)")?;

    if cli.lb_disable {
        lightbar::disable(&path)?;
        config::save(&LightbarConfig { enabled: false, ..Default::default() })?;
        println!("{}", "Lightbar desligado.".dimmed());
        return Ok(());
    }

    if let Some(ref color_name) = cli.lb_color {
        let (r, g, b) = aucc_rs::keyboard::colors::get_color(color_name)
            .ok_or_else(|| format!("Cor desconhecida: '{color_name}'"))?;
        lightbar::apply_color(&path, r, g, b, cli.lb_brightness)?;
        config::save(&LightbarConfig { enabled: true, r, g, b, brightness: cli.lb_brightness, save_eeprom: false })?;
        println!("{}", format!("Lightbar: cor '{color_name}' brilho {}% aplicada.", cli.lb_brightness).green());
        return Ok(());
    }

    // --lb-restore: re-apply saved state silently
    let cfg = config::load();
    if !cfg.enabled {
        lightbar::disable(&path)?;
    } else {
        lightbar::apply_color(&path, cfg.r, cfg.g, cfg.b, cfg.brightness)?;
    }
    Ok(())
}

fn run_no_dev(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if cli.telemetry {
        print_telemetry();
        return Ok(());
    }
    if let Some(ref p) = cli.profile {
        let profile = PowerProfile::from_str(p)
            .ok_or_else(|| format!("Perfil desconhecido: '{p}'. Use: silent, balanced, turbo"))?;
        power::apply_profile(profile)?;
        let (pl1, pl2) = profile.limits_uw();
        println!("{}", format!("Perfil '{}' aplicado: PL1={}W PL2={}W",
            profile.name(), pl1 / 1_000_000, pl2 / 1_000_000).green());
        return Ok(());
    }
    if let Some(w) = cli.tdp {
        power::apply_tdp_w(w)?;
        println!("{}", format!("TDP PL1 = {w}W aplicado.").green());
        return Ok(());
    }
    Ok(())
}

fn run(dev: &KeyboardDevice, cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if cli.disable {
        dev.disable()?;
        println!("{}", "Teclado desligado.".dimmed());
        return Ok(());
    }
    if let Some(style) = &cli.style {
        let (name, letter, reactive) = split_style(style);
        let effect = Effect::from_str(name).ok_or_else(|| format!("Efeito desconhecido: '{name}'"))?;
        let dir = cli.direction.to_wave_dir();
        dev.apply_effect(&effect_payload(effect, cli.speed, cli.brightness, letter, dir, reactive, cli.save))?;
        println!("{}", format!("Efeito '{style}' aplicado.").green());
        return Ok(());
    }
    if let Some(c) = &cli.color {
        let (r, g, b) = get_color(c).ok_or_else(|| format!("Cor desconhecida: '{c}'"))?;
        dev.apply_mono_color(r, g, b, cli.brightness, cli.save)?;
        println!("{}", format!("Cor '{c}' aplicada.").green());
        return Ok(());
    }
    if let Some(cols) = &cli.h_alt {
        let (ra, ga, ba) = get_color(&cols[0]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[0]))?;
        let (rb, gb, bb) = get_color(&cols[1]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[1]))?;
        dev.apply_alt_color(ra, ga, ba, rb, gb, bb, cli.brightness, true, cli.save)?;
        println!("{}", format!("Alternado H: {} / {} aplicado.", cols[0], cols[1]).green());
        return Ok(());
    }
    if let Some(cols) = &cli.v_alt {
        let (ra, ga, ba) = get_color(&cols[0]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[0]))?;
        let (rb, gb, bb) = get_color(&cols[1]).ok_or_else(|| format!("Cor desconhecida: '{}'", cols[1]))?;
        dev.apply_alt_color(ra, ga, ba, rb, gb, bb, cli.brightness, false, cli.save)?;
        println!("{}", format!("Alternado V: {} / {} aplicado.", cols[0], cols[1]).green());
        return Ok(());
    }
    Ok(())
}

fn print_telemetry() {
    use aucc_rs::telemetry;
    let t = telemetry::collect();
    println!("{}", "═══ Telemetria do Sistema ═══".cyan().bold());
    println!("CPU  avg {:.0}°C  max {:.0}°C", t.cpu.temp_avg, t.cpu.temp_max);
    if let Some(g) = &t.gpu {
        println!("GPU  {}  {}°C  {}%  VRAM {}/{} MB",
            g.name, g.temp_c, g.utilization_pct, g.vram_used_mb, g.vram_total_mb);
    }
    println!("RAM  {}/{} MB ({:.0}%)", t.ram.used_mb, t.ram.total_mb, t.ram.used_pct);
    for n in &t.nvme {
        println!("NVMe ({})  {:.0}°C", n.hwmon, n.temp_c);
    }
    if let Some(b) = &t.battery {
        let dir = if b.current_now_ma > 0 { "carregando" }
            else if b.current_now_ma < 0 { "descarregando" } else { "AC" };
        println!("BAT  {}%  {}/{} mAh  {}  ciclos: {}",
            b.capacity_pct, b.charge_now_mah, b.charge_full_mah, dir, b.cycle_count);
    }
    if let Some(lim) = aucc_rs::power::read_limits() {
        let prof_name = aucc_rs::power::detect_profile().map(|p| p.name()).unwrap_or("?");
        let gov = aucc_rs::power::read_governor().unwrap_or_else(|| "?".into());
        let epp = aucc_rs::power::read_epp().unwrap_or_else(|| "?".into());
        println!("TDP  PL1={:.0}W  PL2={:.0}W  ({})  governor={}  epp={}",
            lim.pl1_w, lim.pl2_w, prof_name, gov, epp);
    }
}

fn split_style(style: &str) -> (&str, Option<char>, bool) {
    let reactive = Effect::is_reactive_alias(style);

    if let Some(last) = style.chars().last() {
        if "roygbtp".contains(last) && style.len() > 1 {
            let prefix = &style[..style.len() - last.len_utf8()];
            if Effect::from_str(prefix).is_some() {
                return (prefix, Some(last), reactive);
            }
        }
    }
    (style, None, reactive)
}
