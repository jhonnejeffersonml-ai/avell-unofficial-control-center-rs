#!/usr/bin/env bash
# install.sh — Avell Custom Control Center production installer
#
# Installs aucc-rs binaries, udev rules and polkit policy on an
# Avell Storm 470 (or compatible TongFang chassis) running Ubuntu/Debian.
#
# Usage:
#   ./install/install.sh          # build from source and install
#   ./install/install.sh --skip-build  # install pre-built binaries only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_DIR="$PROJECT_DIR/aucc-rs"
BIN_DIR="/usr/local/bin"
UDEV_DIR="/etc/udev/rules.d"
SYSTEMD_DIR="/etc/systemd/system"
SLEEP_HOOK_DIR="/lib/systemd/system-sleep"
POLKIT_DIR="/usr/share/polkit-1/actions"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

info()    { echo -e "${GREEN}[+]${NC} $*"; }
warn()    { echo -e "${YELLOW}[!]${NC} $*"; }
error()   { echo -e "${RED}[✗]${NC} $*" >&2; exit 1; }

require_root() {
    if [ "$EUID" -ne 0 ]; then
        error "Este script precisa de root. Execute: sudo ./install/install.sh"
    fi
}

check_deps() {
    info "Verificando dependências do sistema..."
    local missing=()
    for pkg in libusb-1.0-0 libgcc-s1; do
        dpkg -s "$pkg" &>/dev/null || missing+=("$pkg")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        warn "Instalando pacotes: ${missing[*]}"
        apt-get install -y "${missing[@]}"
    fi
}

build_rust() {
    info "Compilando aucc-rs (Rust)..."

    # When running under sudo, $HOME is /root but cargo lives in the invoking
    # user's home. Resolve the real home via getent to avoid false-negatives.
    local REAL_USER REAL_HOME
    REAL_USER="${SUDO_USER:-$USER}"
    REAL_HOME="$(getent passwd "$REAL_USER" | cut -d: -f6)"

    if ! command -v cargo &>/dev/null; then
        if [ -x "$REAL_HOME/.cargo/bin/cargo" ]; then
            export PATH="$REAL_HOME/.cargo/bin:$PATH"
        else
            warn "Cargo não encontrado. Instalando Rust toolchain via rustup..."
            # Run as the invoking user, not root
            sudo -u "$REAL_USER" bash -c \
                'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path'
            export PATH="$REAL_HOME/.cargo/bin:$PATH"
        fi
    fi

    # Build-time deps (pkg-config + libusb-dev may be needed by rusb)
    for pkg in pkg-config libusb-1.0-0-dev; do
        dpkg -s "$pkg" &>/dev/null || apt-get install -y "$pkg"
    done

    pushd "$RUST_DIR" > /dev/null
    cargo build --release
    popd > /dev/null
    info "Build concluído."
}

install_binaries() {
    info "Instalando binários em $BIN_DIR ..."
    local ui_bin="$RUST_DIR/target/release/aucc-ui"
    local cli_bin="$RUST_DIR/target/release/aucc"

    [ -x "$ui_bin" ] || error "Binário não encontrado: $ui_bin — rode sem --skip-build"
    [ -x "$cli_bin" ] || error "Binário não encontrado: $cli_bin — rode sem --skip-build"

    install -m 755 "$ui_bin" "$BIN_DIR/aucc-ui"
    install -m 755 "$cli_bin" "$BIN_DIR/aucc"
    info "Binários instalados: $BIN_DIR/aucc-ui  $BIN_DIR/aucc"

    # Ensure config directory exists for lightbar persistence.
    # CLI runs as plugdev user (no root) but must write here, so grant
    # group rwx to plugdev. Otherwise lb-color/lb-disable saves silently
    # fail with EACCES and persistence breaks.
    mkdir -p /etc/aucc
    chgrp -R plugdev /etc/aucc
    chmod -R g+w /etc/aucc
    info "Diretório de config criado: /etc/aucc (gravável por plugdev)"
}

install_udev() {
    info "Instalando regra udev ($UDEV_DIR/70-avell-hid.rules)..."
    install -m 644 "$SCRIPT_DIR/70-avell-hid.rules" "$UDEV_DIR/70-avell-hid.rules"
    udevadm control --reload-rules
    udevadm trigger --subsystem-match=usb --subsystem-match=hidraw --subsystem-match=power_supply
    info "udev recarregado."
}

install_systemd() {
    info "Instalando systemd units para restauração do teclado e da lightbar..."

    # Boot-restore service — runs after udev settled, triggered also via SYSTEMD_WANTS.
    cat > "$SYSTEMD_DIR/aucc-restore.service" <<'EOF'
[Unit]
Description=Restore Avell keyboard and lightbar state
After=systemd-udev-settle.service

[Service]
Type=oneshot
# The EC blanks the keyboard backlight once, shortly after a power event
# (AC plug/unplug), overriding whatever state we just applied. We reapply
# the state again after a short delay to win that race. The delay value
# (1s) comes from a measurement on the target machine — do not tune it
# blindly. The first ExecStart is prefixed with '-' so a transient failure
# (e.g. the keyboard USB device not yet settled at boot) does not abort the
# retry that follows; the final ExecStart is left unprefixed so a genuine
# persistent failure still surfaces in `systemctl status`.
ExecStart=-/usr/local/bin/aucc --restore
ExecStart=/bin/sleep 1
ExecStart=/usr/local/bin/aucc --restore
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    # Post-resume hook — called by systemd-sleep with pre|post arg.
    mkdir -p "$SLEEP_HOOK_DIR"
    cat > "$SLEEP_HOOK_DIR/aucc-lightbar" <<'EOF'
#!/bin/sh
# Restore Avell keyboard and lightbar after resume — managed by aucc
[ "$1" = "post" ] && /usr/local/bin/aucc --restore
EOF
    chmod +x "$SLEEP_HOOK_DIR/aucc-lightbar"

    systemctl daemon-reload
    systemctl enable --now aucc-restore.service
    info "systemd service habilitado: aucc-restore.service"
    info "sleep hook instalado: $SLEEP_HOOK_DIR/aucc-lightbar"

    # Migration: remove the pre-0.2 unit now that the new one is up and
    # running (errors ignored — a missing old unit is the normal case on
    # fresh installs).
    systemctl disable --now aucc-lightbar-restore.service 2>/dev/null || true
    rm -f "$SYSTEMD_DIR/aucc-lightbar-restore.service"
}

install_polkit() {
    info "Instalando polkit policy ($POLKIT_DIR/org.avell.aucc.policy)..."
    install -m 644 "$SCRIPT_DIR/org.avell.aucc.policy" "$POLKIT_DIR/org.avell.aucc.policy"
    info "Polkit policy instalada."
}

ensure_plugdev() {
    local target_user="${SUDO_USER:-$USER}"
    if ! groups "$target_user" | grep -q plugdev; then
        warn "Adicionando $target_user ao grupo 'plugdev'..."
        usermod -aG plugdev "$target_user"
        warn "Faça logout e login novamente para o grupo ter efeito."
    else
        info "Usuário $target_user já está no grupo plugdev. ✓"
    fi
}

print_summary() {
    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════${NC}"
    echo -e "${GREEN}  Avell Custom Control Center — instalado!  ${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════${NC}"
    echo ""
    echo "  TUI interativo:     pkexec aucc-ui"
    echo "  Perfil Silencioso:  pkexec aucc --profile silent"
    echo "  Perfil Equilibrado: pkexec aucc --profile balanced"
    echo "  Perfil Turbo:       pkexec aucc --profile turbo"
    echo "  Telemetria:         aucc --telemetry   (sem root)"
    echo ""
    echo "  Lightbar — cor:     aucc --lb-color red --lb-brightness 50"
    echo "  Lightbar — deslig.: aucc --lb-disable"
    echo "  Restaurar tudo:     aucc --restore  (automático no boot, na troca AC/bateria e após suspend)"
    echo ""
    echo "  Atalho rápido:      $PROJECT_DIR/teclado"
    echo ""
}

# ── main ──────────────────────────────────────────────────────────────────────
require_root
check_deps

SKIP_BUILD=false
for arg in "$@"; do
    [ "$arg" = "--skip-build" ] && SKIP_BUILD=true
done

if [ "$SKIP_BUILD" = false ]; then
    build_rust
fi

install_binaries
install_udev
install_systemd
install_polkit
ensure_plugdev
print_summary
