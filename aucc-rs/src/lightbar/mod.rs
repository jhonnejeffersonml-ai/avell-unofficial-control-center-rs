use std::fs;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

pub const LB_VENDOR_ID: u16  = 0x048d;
pub const LB_PRODUCT_ID: u16 = 0x7001;

// HID ioctl: HIDIOCSFEATURE(n) = _IOWR('H', 0x06, char[n])
// _IOC(dir, type, nr, size) = (dir<<30)|(type<<8)|nr|(size<<16)
// _IOWR = IOC_READ|IOC_WRITE = 3, so: (3<<30) | ('H'<<8) | 0x06 | (n<<16)
fn hidiocsfeature(n: usize) -> libc::c_ulong {
    ((3u64 << 30) | (('H' as u64) << 8) | 0x06 | ((n as u64) << 16)) as libc::c_ulong
}

/// Scan `/sys/class/hidraw/*/device/uevent` for the lightbar HID device.
pub fn find_hidraw_path() -> Option<PathBuf> {
    let target = format!(
        "HID_ID=0003:0000{:04X}:0000{:04X}",
        LB_VENDOR_ID, LB_PRODUCT_ID
    );
    let entries = fs::read_dir("/sys/class/hidraw").ok()?;
    for entry in entries.flatten() {
        let uevent_path = entry.path().join("device/uevent");
        if let Ok(contents) = fs::read_to_string(&uevent_path) {
            if contents.lines().any(|l| l == target) {
                let node_name = entry.file_name();
                return Some(PathBuf::from("/dev").join(node_name));
            }
        }
    }
    None
}

/// Attempt to rebind `usbhid` to the lightbar's USB interface.
pub fn ensure_bound() -> std::io::Result<()> {
    let bind_path = "/sys/bus/usb/drivers/usbhid/bind";
    let iface = "1-10:1.1";
    let mut f = fs::OpenOptions::new().write(true).open(bind_path)?;
    f.write_all(iface.as_bytes())?;
    Ok(())
}

/// Apply a solid color to the lightbar via hidraw HIDIOCSFEATURE ioctl.
///
/// `brightness`: 0x00–0x64 (0–100%).
pub fn apply_color(path: &Path, r: u8, g: u8, b: u8, brightness: u8) -> std::io::Result<()> {
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    let fd = file.as_raw_fd();

    // Report 1: set color [report_id, 0x14, 0x00, 0x01, R, G, B, 0x00, 0x00]
    let color_report: [u8; 9] = [0x00, 0x14, 0x00, 0x01, r, g, b, 0x00, 0x00];
    unsafe {
        if libc::ioctl(fd, hidiocsfeature(color_report.len()), color_report.as_ptr()) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    // Report 2: set brightness [report_id, 0x08, 0x22, 0x01, 0x01, brightness, 0x01, 0x00, 0x00]
    let brt_report: [u8; 9] = [0x00, 0x08, 0x22, 0x01, 0x01, brightness, 0x01, 0x00, 0x00];
    unsafe {
        if libc::ioctl(fd, hidiocsfeature(brt_report.len()), brt_report.as_ptr()) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Turn the lightbar off.
pub fn disable(path: &Path) -> std::io::Result<()> {
    apply_color(path, 0, 0, 0, 0x00)
}
