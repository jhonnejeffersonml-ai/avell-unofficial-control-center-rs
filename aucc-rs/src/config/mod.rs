//! Persisted state for the lightbar and the keyboard.
//!
//! Both configs use the same trivial `key=value` line format so that they can
//! be inspected and edited by hand under /etc/aucc/.

pub mod lightbar;

pub use lightbar::{
    CONFIG_PATH, LightbarConfig, load, load_file, save, save_to,
};

use std::fs;

/// Parse a `key=value` file into pairs, skipping blanks and `#` comments.
///
/// Returns None only when the file cannot be read.
pub fn parse_kv(path: &str) -> Option<Vec<(String, String)>> {
    let content = fs::read_to_string(path).ok()?;
    Some(parse_kv_str(&content))
}

/// Parse `key=value` lines from an in-memory string.
pub fn parse_kv_str(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            out.push((key.trim().to_string(), val.trim().to_string()));
        }
    }
    out
}
