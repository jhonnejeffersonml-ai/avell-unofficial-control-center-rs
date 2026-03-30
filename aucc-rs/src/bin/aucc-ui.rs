#[cfg(unix)]
fn require_root() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Requer root. Use: ./teclado  ou  sudo aucc-ui");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn require_root() {}

fn main() {
    require_root();
    if let Err(e) = aucc_rs::ui::tui::run() {
        eprintln!("Erro na TUI: {e}");
        std::process::exit(1);
    }
}
