/// Install/uninstall logic shared between CLI and TUI.
///
/// Returns a human-readable result message (Ok) or error string (Err).
use std::fs;
use std::process::Command;

// Udev rules content embedded in the binary so install works standalone.
const UDEV_RULES: &str = "\
# udev rules for Avell Storm 470 (TongFang chassis) HID devices — managed by aucc
#
# Grants read/write access to members of the 'plugdev' group so that
# keyboard RGB and lightbar control work WITHOUT root privileges.
# The RUN+= entry restores the saved lightbar state on every boot.

# ITE Device 8291 — RGB Keyboard (048d:600b)
SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"600b\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\"

# ITE Device 8233 — Front LED Lightbar (048d:7001)
SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"7001\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\", \\
    RUN+=\"/usr/local/bin/aucc --lb-restore\"

SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"048d\", ATTRS{idProduct}==\"7001\", \\
    GROUP=\"plugdev\", MODE=\"0660\", TAG+=\"uaccess\"
";

pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/70-avell-hid.rules";
pub const INSTALL_BIN_PATH: &str = "/usr/local/bin/aucc";
pub const INSTALL_UI_BIN_PATH: &str = "/usr/local/bin/aucc-ui";

type Result = std::result::Result<String, String>;

pub fn install(current_exe: &std::path::Path, bin_dest: &str) -> Result {
    fs::create_dir_all("/etc/aucc")
        .map_err(|e| format!("Erro ao criar /etc/aucc: {e}"))?;

    fs::write(UDEV_RULE_PATH, UDEV_RULES)
        .map_err(|e| format!("Erro ao escrever regra udev: {e}"))?;

    Command::new("udevadm")
        .args(["control", "--reload-rules"])
        .status()
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

    fs::copy(current_exe, bin_dest)
        .map_err(|e| format!("Erro ao copiar binário para {bin_dest}: {e}"))?;

    Ok(format!("Instalado em {bin_dest}  |  udev recarregado ✔"))
}

pub fn uninstall(bin_dest: &str) -> Result {
    let mut msgs = Vec::new();

    match fs::remove_file(UDEV_RULE_PATH) {
        Ok(_) => msgs.push(format!("Regra udev removida")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            msgs.push("Regra udev já removida".to_string())
        }
        Err(e) => return Err(format!("Erro ao remover regra udev: {e}")),
    }

    Command::new("udevadm")
        .args(["control", "--reload-rules"])
        .status()
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
