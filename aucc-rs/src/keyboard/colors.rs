/// Named RGB colors available for keyboard backlight.
pub const COLORS: &[(&str, (u8, u8, u8))] = &[
    ("red",        (0xFF, 0x00, 0x00)),
    ("green",      (0x00, 0xFF, 0x00)),
    ("blue",       (0x00, 0x00, 0xFF)),
    ("teal",       (0x00, 0xFF, 0xFF)),
    ("purple",     (0xFF, 0x00, 0xFF)),
    ("pink",       (0xFF, 0x00, 0x77)),
    ("yellow",     (0xFF, 0x77, 0x00)),
    ("white",      (0xFF, 0xFF, 0xFF)),
    ("orange",     (0xFF, 0x1C, 0x00)),
    ("olive",      (0x80, 0x80, 0x00)),
    ("maroon",     (0x80, 0x00, 0x00)),
    ("brown",      (0xA5, 0x2A, 0x2A)),
    ("gray",       (0x80, 0x80, 0x80)),
    ("skyblue",    (0x87, 0xCE, 0xEB)),
    ("navy",       (0x00, 0x00, 0x80)),
    ("crimson",    (0xDC, 0x14, 0x3C)),
    ("darkgreen",  (0x00, 0x64, 0x00)),
    ("lightgreen", (0x90, 0xEE, 0x90)),
    ("gold",       (0xFF, 0xD7, 0x00)),
    ("violet",     (0xEE, 0x82, 0xEE)),
];

/// Resolve a color name to its RGB tuple.
pub fn get_color(name: &str) -> Option<(u8, u8, u8)> {
    COLORS.iter().find(|(n, _)| *n == name).map(|(_, rgb)| *rgb)
}

/// 64-byte payload: same color repeated for all 16 key slots.
/// Format per slot: [0x00, R, G, B]
pub fn mono_payload(r: u8, g: u8, b: u8) -> Vec<u8> {
    let slot = [0x00, r, g, b];
    slot.repeat(16)
}

/// 64-byte payload alternating color_a and color_b horizontally (8+8 slots).
pub fn h_alt_payload(ra: u8, ga: u8, ba: u8, rb: u8, gb: u8, bb: u8) -> Vec<u8> {
    let ca = [0x00, ra, ga, ba];
    let cb = [0x00, rb, gb, bb];
    ca.repeat(8).into_iter().chain(cb.repeat(8)).collect()
}

/// 64-byte payload alternating color_a and color_b vertically (interleaved slots).
pub fn v_alt_payload(ra: u8, ga: u8, ba: u8, rb: u8, gb: u8, bb: u8) -> Vec<u8> {
    let ca = [0x00, ra, ga, ba];
    let cb = [0x00, rb, gb, bb];
    (0..16)
        .flat_map(|i| if i % 2 == 0 { ca } else { cb })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── get_color() ─────────────────────────────────────────────────

    #[test]
    fn get_color_primary() {
        assert_eq!(get_color("red"), Some((0xFF, 0x00, 0x00)));
        assert_eq!(get_color("green"), Some((0x00, 0xFF, 0x00)));
        assert_eq!(get_color("blue"), Some((0x00, 0x00, 0xFF)));
        assert_eq!(get_color("white"), Some((0xFF, 0xFF, 0xFF)));
        assert_eq!(get_color("black"), None);
    }

    #[test]
    fn get_color_all_names() {
        for (name, rgb) in COLORS {
            assert_eq!(get_color(name), Some(*rgb));
        }
    }

    #[test]
    fn get_color_invalid() {
        assert_eq!(get_color("notacolor"), None);
        assert_eq!(get_color(""), None);
        assert_eq!(get_color("RED"), None); // case-sensitive
    }

    #[test]
    fn color_count() {
        assert_eq!(COLORS.len(), 20);
    }

    // ── mono_payload() ──────────────────────────────────────────────

    #[test]
    fn mono_payload_size() {
        let p = mono_payload(0xFF, 0x00, 0x00);
        assert_eq!(p.len(), 64); // 16 slots × 4 bytes
    }

    #[test]
    fn mono_payload_content() {
        let p = mono_payload(0x11, 0x22, 0x33);
        for i in 0..16 {
            let offset = i * 4;
            assert_eq!(&p[offset..offset + 4], &[0x00, 0x11, 0x22, 0x33]);
        }
    }

    // ── h_alt_payload() ─────────────────────────────────────────────

    #[test]
    fn h_alt_payload_size() {
        let p = h_alt_payload(0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF);
        assert_eq!(p.len(), 64);
    }

    #[test]
    fn h_alt_payload_content() {
        let p = h_alt_payload(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF);
        // First 8 slots = color A
        for i in 0..8 {
            let offset = i * 4;
            assert_eq!(&p[offset..offset + 4], &[0x00, 0xAA, 0xBB, 0xCC]);
        }
        // Last 8 slots = color B
        for i in 8..16 {
            let offset = i * 4;
            assert_eq!(&p[offset..offset + 4], &[0x00, 0xDD, 0xEE, 0xFF]);
        }
    }

    // ── v_alt_payload() ─────────────────────────────────────────────

    #[test]
    fn v_alt_payload_size() {
        let p = v_alt_payload(0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF);
        assert_eq!(p.len(), 64);
    }

    #[test]
    fn v_alt_payload_interleaved() {
        let p = v_alt_payload(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF);
        // Even slots = color A, odd slots = color B
        for i in 0..16 {
            let offset = i * 4;
            let expected = if i % 2 == 0 {
                [0x00, 0xAA, 0xBB, 0xCC]
            } else {
                [0x00, 0xDD, 0xEE, 0xFF]
            };
            assert_eq!(&p[offset..offset + 4], &expected, "slot {i} mismatch");
        }
    }
}
