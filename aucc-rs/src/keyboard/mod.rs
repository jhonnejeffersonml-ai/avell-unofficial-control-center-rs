pub mod colors;
pub mod effects;

use rusb::{DeviceHandle, GlobalContext};

pub const VENDOR_ID: u16  = 0x048d;
pub const PRODUCT_ID: u16 = 0x600b;
const INTERFACE: u8       = 1;

pub struct KeyboardDevice {
    handle: DeviceHandle<GlobalContext>,
}

impl KeyboardDevice {
    /// Find and open the ITE 8291 keyboard device.
    pub fn open() -> rusb::Result<Self> {
        let handle = rusb::open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID)
            .ok_or(rusb::Error::NoDevice)?;

        // Detach kernel driver on Linux so we can claim the interface.
        #[cfg(target_os = "linux")]
        if handle.kernel_driver_active(INTERFACE)? {
            handle.detach_kernel_driver(INTERFACE)?;
        }

        handle.claim_interface(INTERFACE)?;
        Ok(Self { handle })
    }

    /// Send an 8-byte HID feature report (control transfer, class request).
    pub fn ctrl_write(&self, payload: &[u8; 8]) -> rusb::Result<()> {
        // bmRequestType=0x21 (Host→Device, Class, Interface)
        // bRequest=0x09 (SET_REPORT)
        // wValue=0x0300 (Feature report, ID 0)
        // wIndex=1 (interface)
        self.handle.write_control(0x21, 0x09, 0x0300, 1, payload, std::time::Duration::from_secs(1))?;
        Ok(())
    }

    /// Send `times` repetitions of `payload` to the bulk-out endpoint.
    pub fn bulk_write(&self, times: u8, payload: &[u8]) -> rusb::Result<()> {
        let endpoint = self.find_out_endpoint()?;
        for _ in 0..times {
            self.handle.write_bulk(endpoint, payload, std::time::Duration::from_secs(1))?;
        }
        Ok(())
    }

    /// Disable all keyboard backlight.
    pub fn disable(&self) -> rusb::Result<()> {
        self.ctrl_write(&[0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
    }

    /// Set brightness without changing effect.
    pub fn set_brightness(&self, level: u8) -> rusb::Result<()> {
        let brt = effects::brightness_byte(level);
        self.ctrl_write(&[0x08, 0x02, 0x33, 0x00, brt, 0x00, 0x00, 0x00])
    }

    /// Apply a mono color to all keys.
    pub fn apply_mono_color(&self, r: u8, g: u8, b: u8, brightness: u8, save: bool) -> rusb::Result<()> {
        let brt = effects::brightness_byte(brightness);
        let save_byte = if save { 0x01 } else { 0x00 };
        self.ctrl_write(&[0x08, 0x02, 0x33, 0x00, brt, 0x00, 0x00, 0x00])?;
        self.ctrl_write(&[0x12, 0x00, 0x00, 0x08, save_byte, 0x00, 0x00, 0x00])?;
        let payload = colors::mono_payload(r, g, b);
        self.bulk_write(8, &payload)
    }

    /// Apply alternating horizontal or vertical colors.
    pub fn apply_alt_color(
        &self,
        ra: u8, ga: u8, ba: u8,
        rb: u8, gb: u8, bb: u8,
        brightness: u8,
        horizontal: bool,
        save: bool,
    ) -> rusb::Result<()> {
        let brt = effects::brightness_byte(brightness);
        let save_byte = if save { 0x01 } else { 0x00 };
        self.ctrl_write(&[0x08, 0x02, 0x33, 0x00, brt, 0x00, 0x00, 0x00])?;
        self.ctrl_write(&[0x12, 0x00, 0x00, 0x08, save_byte, 0x00, 0x00, 0x00])?;
        let payload = if horizontal {
            colors::h_alt_payload(ra, ga, ba, rb, gb, bb)
        } else {
            colors::v_alt_payload(ra, ga, ba, rb, gb, bb)
        };
        self.bulk_write(8, &payload)
    }

    /// Apply a named lighting effect.
    pub fn apply_effect(&self, payload: &[u8; 8]) -> rusb::Result<()> {
        self.ctrl_write(payload)
    }

    fn find_out_endpoint(&self) -> rusb::Result<u8> {
        let device = self.handle.device();
        let config = device.active_config_descriptor()?;
        for iface in config.interfaces() {
            if iface.number() != INTERFACE {
                continue;
            }
            for desc in iface.descriptors() {
                for ep in desc.endpoint_descriptors() {
                    if ep.direction() == rusb::Direction::Out {
                        return Ok(ep.address());
                    }
                }
            }
        }
        Err(rusb::Error::NotFound)
    }
}
