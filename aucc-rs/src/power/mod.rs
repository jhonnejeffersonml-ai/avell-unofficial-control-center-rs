//! CPU power limit (TDP) control via Intel RAPL sysfs.
//!
//! Requires root — the powercap files are root-writable only.

use std::fs;
use std::path::Path;

const RAPL_ROOT: &str = "/sys/class/powercap/intel-rapl:0";

/// Predefined power profiles mapped to PL1 / PL2 in microwatts.
///
/// Values per spec v2.0 §4.2 — initial estimates, calibrate with stress-ng + turbostat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerProfile {
    /// Silent: 25 W PL1 / 35 W PL2 — quiet, cool, powersave governor.
    Silent,
    /// Balanced: 45 W PL1 / 65 W PL2 — good mix of performance and noise.
    Balanced,
    /// Turbo: 80 W PL1 / 120 W PL2 — maximum performance.
    Turbo,
}

impl PowerProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Silent => "Silencioso",
            Self::Balanced => "Equilibrado",
            Self::Turbo => "Turbo",
        }
    }

    /// (PL1_uw, PL2_uw)
    ///
    /// Values per §4.2 of the technical report, derived from Intel i9 specs:
    /// - Processor Base Power (PL1): 55 W
    /// - Maximum Turbo Power / MTP (PL2): 157 W
    /// - Minimum Assured Power (MAP): 45 W
    ///
    /// Silent stays below the base PL1 to prioritise thermals and noise.
    /// Balanced respects the Intel envelope. Turbo uses the full MTP ceiling.
    ///
    /// ⚠️ Values above 55 W PL1 are OEM-tuned; validate with stress-ng + turbostat
    /// before sustained use to confirm no thermal throttling.
    pub fn limits_uw(self) -> (u64, u64) {
        match self {
            Self::Silent   => ( 35_000_000,  55_000_000),
            Self::Balanced => ( 55_000_000, 100_000_000),
            Self::Turbo    => ( 95_000_000, 157_000_000),
        }
    }

    /// CPU scaling governor for this profile.
    pub fn governor(self) -> &'static str {
        match self {
            Self::Silent | Self::Balanced => "powersave",
            Self::Turbo                   => "performance",
        }
    }

    /// Intel EPP (energy_performance_preference) for this profile.
    pub fn epp(self) -> &'static str {
        match self {
            Self::Silent   => "power",
            Self::Balanced => "balance_performance",
            Self::Turbo    => "performance",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "silent" | "silencioso" => Some(Self::Silent),
            "balanced" | "equilibrado" => Some(Self::Balanced),
            "turbo" => Some(Self::Turbo),
            _ => None,
        }
    }

    pub fn all() -> &'static [PowerProfile] {
        &[Self::Silent, Self::Balanced, Self::Turbo]
    }
}

#[derive(Debug, Clone)]
pub struct PowerLimits {
    /// PL1 in watts.
    pub pl1_w: f32,
    /// PL2 in watts.
    pub pl2_w: f32,
}

/// Read current PL1/PL2 from RAPL sysfs.
pub fn read_limits() -> Option<PowerLimits> {
    let root = Path::new(RAPL_ROOT);
    let pl1_uw: u64 = fs::read_to_string(root.join("constraint_0_power_limit_uw"))
        .ok()?.trim().parse().ok()?;
    let pl2_uw: u64 = fs::read_to_string(root.join("constraint_1_power_limit_uw"))
        .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(pl1_uw);
    Some(PowerLimits {
        pl1_w: pl1_uw as f32 / 1_000_000.0,
        pl2_w: pl2_uw as f32 / 1_000_000.0,
    })
}

/// Apply a power profile: sets PL1/PL2 (RAPL), CPU governor and EPP on all cores.
/// Requires root.
pub fn apply_profile(profile: PowerProfile) -> std::io::Result<()> {
    let (pl1, pl2) = profile.limits_uw();
    let root = Path::new(RAPL_ROOT);
    fs::write(root.join("constraint_0_power_limit_uw"), pl1.to_string())?;
    let _ = fs::write(root.join("constraint_1_power_limit_uw"), pl2.to_string());

    // Apply governor and EPP to all CPU cores (best-effort — ignore missing cores).
    let governor = profile.governor();
    let epp = profile.epp();
    apply_cpu_governor_epp(governor, epp);

    Ok(())
}
/// Write governor and EPP to all online CPU cores.
fn apply_cpu_governor_epp(governor: &str, epp: &str) {
    // Iterate cpu0..cpu63 — stop at first missing
    for i in 0..64u32 {
        let base = format!("/sys/devices/system/cpu/cpu{i}/cpufreq");
        let gov_path = format!("{base}/scaling_governor");
        let epp_path = format!("{base}/energy_performance_preference");
        if !Path::new(&gov_path).exists() {
            break;
        }
        let _ = fs::write(&gov_path, governor);
        let _ = fs::write(&epp_path, epp);
    }
}

/// Apply arbitrary PL1 in watts. PL2 is set to max(PL1, PL2_current). Requires root.
pub fn apply_tdp_w(pl1_w: f32) -> std::io::Result<()> {
    let pl1_uw = (pl1_w * 1_000_000.0) as u64;
    let root = Path::new(RAPL_ROOT);
    fs::write(root.join("constraint_0_power_limit_uw"), pl1_uw.to_string())?;
    Ok(())
}

/// Read current governor from cpu0 (representative).
pub fn read_governor() -> Option<String> {
    fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .ok()
        .map(|s| s.trim().to_string())
}

/// Read current EPP from cpu0 (representative).
pub fn read_epp() -> Option<String> {
    fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
        .ok()
        .map(|s| s.trim().to_string())
}

/// Detect the closest named profile from current PL1 + governor.
pub fn detect_profile() -> Option<PowerProfile> {
    let limits = read_limits()?;
    let pl1 = limits.pl1_w as u64;
    let governor = read_governor().unwrap_or_default();
    // Prefer matching by governor first, then closest PL1
    PowerProfile::all().iter().copied().min_by_key(|p| {
        let (target, _) = p.limits_uw();
        let target_w = target / 1_000_000;
        let gov_bonus: u64 = if p.governor() == governor { 0 } else { 100 };
        (pl1 as i64 - target_w as i64).unsigned_abs() + gov_bonus
    })
}
