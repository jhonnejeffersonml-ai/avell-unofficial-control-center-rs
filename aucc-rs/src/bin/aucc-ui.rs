const INSTALL_BIN_PATH: &str = "/usr/local/bin/aucc-ui";

#[cfg(unix)]
fn require_root() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Requer root. Use: sudo aucc-ui");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn require_root() {}

fn install_self() {
    require_root();
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => { eprintln!("Erro ao obter caminho do binário: {e}"); std::process::exit(1); }
    };
    match std::fs::copy(&current_exe, INSTALL_BIN_PATH) {
        Ok(_) => println!("Binário instalado: {INSTALL_BIN_PATH}"),
        Err(e) => { eprintln!("Erro ao instalar binário: {e}"); std::process::exit(1); }
    }
}

fn uninstall_self() {
    require_root();
    match std::fs::remove_file(INSTALL_BIN_PATH) {
        Ok(_) => println!("Binário removido: {INSTALL_BIN_PATH}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound =>
            println!("Binário não encontrado em {INSTALL_BIN_PATH} (já removido?)."),
        Err(e) => { eprintln!("Erro ao remover binário: {e}"); std::process::exit(1); }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install") {
        install_self();
        return;
    }
    if args.iter().any(|a| a == "--uninstall") {
        uninstall_self();
        return;
    }

    require_root();
    if let Err(e) = aucc_rs::ui::tui::run() {
        eprintln!("Erro na TUI: {e}");
        std::process::exit(1);
    }
}
