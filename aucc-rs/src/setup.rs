/// Install/uninstall logic shared between CLI and TUI.
///
/// Returns a human-readable result message (Ok) or error string (Err).
use std::fs;
use std::process::Command;

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

type Result = std::result::Result<String, String>;

pub fn install(current_exe: &std::path::Path, bin_dest: &str) -> Result {
    // 1. Config directory — plugdev-writable so CLI (non-root) can save state.
    fs::create_dir_all("/etc/aucc")
        .map_err(|e| format!("Erro ao criar /etc/aucc: {e}"))?;
    let _ = Command::new("chgrp").args(["-R", "plugdev", "/etc/aucc"]).status();
    let _ = Command::new("chmod").args(["-R", "g+w", "/etc/aucc"]).status();

    // 2. Binary copy — must happen before systemctl enable --now starts the service.
    let src_canonical  = fs::canonicalize(current_exe).unwrap_or_else(|_| current_exe.to_path_buf());
    let dest_canonical = fs::canonicalize(bin_dest).unwrap_or_else(|_| std::path::PathBuf::from(bin_dest));

    let bin_msg = if src_canonical != dest_canonical {
        fs::copy(current_exe, bin_dest)
            .map_err(|e| format!("Erro ao copiar binário para {bin_dest}: {e}"))?;
        format!("binário instalado em {bin_dest}")
    } else {
        format!("binário NÃO atualizado (este já é o instalado — rode o novo executável e instale novamente)")
    };

    // 3. udev rules.
    fs::write(UDEV_RULE_PATH, UDEV_RULES)
        .map_err(|e| format!("Erro ao escrever regra udev: {e}"))?;

    // 3b. Migration: drop the pre-0.2 unit so two services do not race to
    // restore the same devices.
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-lightbar-restore.service"])
        .status();
    let _ = fs::remove_file(OLD_RESTORE_SERVICE_PATH);

    // 4. systemd service (boot restore).
    fs::create_dir_all("/etc/systemd/system")
        .map_err(|e| format!("Erro ao criar /etc/systemd/system: {e}"))?;
    fs::write(RESTORE_SERVICE_PATH, RESTORE_SERVICE)
        .map_err(|e| format!("Erro ao escrever {RESTORE_SERVICE_PATH}: {e}"))?;

    // 5. system-sleep hook (post-resume restore).
    fs::create_dir_all("/lib/systemd/system-sleep")
        .map_err(|e| format!("Erro ao criar /lib/systemd/system-sleep: {e}"))?;
    fs::write(SLEEP_HOOK_PATH, RESTORE_SLEEP_HOOK)
        .map_err(|e| format!("Erro ao escrever {SLEEP_HOOK_PATH}: {e}"))?;
    Command::new("chmod").args(["+x", SLEEP_HOOK_PATH]).status()
        .map_err(|e| format!("chmod +x {SLEEP_HOOK_PATH} falhou: {e}"))?;

    // 6. Activate — daemon-reload first so systemd sees the new unit file.
    Command::new("systemctl").args(["daemon-reload"]).status()
        .map_err(|e| format!("systemctl daemon-reload falhou: {e}"))?;

    Command::new("systemctl")
        .args(["enable", "--now", "aucc-restore.service"])
        .status()
        .map_err(|e| format!("systemctl enable falhou: {e}"))?
        .success()
        .then_some(())
        .ok_or_else(|| "systemctl enable aucc-restore.service retornou erro".to_string())?;

    // 7. udev reload so SYSTEMD_WANTS takes effect on next device event.
    Command::new("udevadm").args(["control", "--reload-rules"]).status()
        .map_err(|e| format!("udevadm control falhou: {e}"))?
        .success()
        .then_some(())
        .ok_or_else(|| "udevadm control --reload-rules retornou erro".to_string())?;

    Command::new("udevadm")
        .args(["trigger", "--subsystem-match=usb", "--subsystem-match=hidraw"])
        .status()
        .map_err(|e| format!("udevadm trigger falhou: {e}"))?
        .success()
        .then_some(())
        .ok_or_else(|| "udevadm trigger retornou erro".to_string())?;

    Ok(format!("systemd service habilitado ✔  |  udev recarregado ✔  |  {bin_msg}"))
}

pub fn uninstall(bin_dest: &str) -> Result {
    let mut msgs = Vec::new();

    // Disable and stop the service before removing the files.
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-restore.service"])
        .status();
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "aucc-lightbar-restore.service"])
        .status();

    for path in [RESTORE_SERVICE_PATH, OLD_RESTORE_SERVICE_PATH, SLEEP_HOOK_PATH] {
        match fs::remove_file(path) {
            Ok(_) => msgs.push(format!("{path} removido")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Erro ao remover {path}: {e}")),
        }
    }

    let _ = Command::new("systemctl").args(["daemon-reload"]).status();

    match fs::remove_file(UDEV_RULE_PATH) {
        Ok(_) => msgs.push("Regra udev removida".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            msgs.push("Regra udev já removida".to_string())
        }
        Err(e) => return Err(format!("Erro ao remover regra udev: {e}")),
    }

    Command::new("udevadm").args(["control", "--reload-rules"]).status()
        .map_err(|e| format!("udevadm falhou: {e}"))?
        .success()
        .then_some(())
        .ok_or_else(|| "udevadm retornou erro".to_string())?;

    match fs::remove_file(bin_dest) {
        Ok(_) => msgs.push(format!("Binário {bin_dest} removido")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            msgs.push(format!("Binário {bin_dest} já removido"))
        }
        Err(e) => return Err(format!("Erro ao remover {bin_dest}: {e}")),
    }

    Ok(msgs.join("  |  "))
}
