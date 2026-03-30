//! Hardware telemetry: CPU, GPU, RAM, NVMe, Battery.
//!
//! All reads are best-effort — missing sysfs nodes return `None` instead of errors.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Telemetry {
    pub cpu: CpuInfo,
    pub gpu: Option<GpuInfo>,
    pub ram: RamInfo,
    pub nvme: Vec<NvmeInfo>,
    pub battery: Option<BatteryInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct CpuInfo {
    /// Average temperature across all cores (°C).
    pub temp_avg: f32,
    /// Maximum temperature across all cores (°C).
    pub temp_max: f32,
    /// Per-core temperatures (°C).
    pub temps: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub temp_c: u32,
    pub utilization_pct: u32,
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RamInfo {
    pub total_mb: u64,
    pub available_mb: u64,
    pub used_mb: u64,
    pub used_pct: f32,
}

#[derive(Debug, Clone)]
pub struct NvmeInfo {
    pub hwmon: String,
    pub temp_c: f32,
}

#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub capacity_pct: u8,
    pub charge_full_mah: u32,
    pub charge_now_mah: u32,
    pub current_now_ma: i32,
    pub cycle_count: u32,
    pub status: String,
}

// ─── CPU ───────────────────────────────────────────────────────────────────

/// Read CPU temperatures from the `coretemp` hwmon node.
pub fn read_cpu() -> CpuInfo {
    let mut temps: Vec<f32> = Vec::new();

    // Find the coretemp hwmon node.
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if fs::read_to_string(&name_path).map(|s| s.trim() == "coretemp").unwrap_or(false) {
                // Read all tempN_input files.
                if let Ok(files) = fs::read_dir(entry.path()) {
                    let mut inputs: Vec<(u32, f32)> = files
                        .flatten()
                        .filter_map(|f| {
                            let name = f.file_name().into_string().ok()?;
                            if name.ends_with("_input") && name.starts_with("temp") {
                                let idx: u32 = name
                                    .trim_start_matches("temp")
                                    .trim_end_matches("_input")
                                    .parse()
                                    .ok()?;
                                let val: f32 = fs::read_to_string(f.path())
                                    .ok()?
                                    .trim()
                                    .parse::<i32>()
                                    .ok()
                                    .map(|v| v as f32 / 1000.0)?;
                                Some((idx, val))
                            } else {
                                None
                            }
                        })
                        .collect();
                    inputs.sort_by_key(|(i, _)| *i);
                    temps = inputs.into_iter().map(|(_, v)| v).collect();
                }
                break;
            }
        }
    }

    if temps.is_empty() {
        return CpuInfo::default();
    }
    let temp_max = temps.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let temp_avg = temps.iter().sum::<f32>() / temps.len() as f32;
    CpuInfo { temp_avg, temp_max, temps }
}

// ─── GPU ───────────────────────────────────────────────────────────────────

/// Parse GPU info from `nvidia-smi`. Returns `None` if not available.
pub fn read_gpu() -> Option<GpuInfo> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,temperature.gpu,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?;
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 5 {
        return None;
    }
    Some(GpuInfo {
        name: parts[0].to_string(),
        temp_c: parts[1].parse().unwrap_or(0),
        utilization_pct: parts[2].parse().unwrap_or(0),
        vram_used_mb: parts[3].parse().unwrap_or(0),
        vram_total_mb: parts[4].parse().unwrap_or(0),
    })
}

// ─── RAM ───────────────────────────────────────────────────────────────────

/// Read RAM info from `/proc/meminfo`.
pub fn read_ram() -> RamInfo {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(_) => return RamInfo::default(),
    };

    let mut total_kb: u64 = 0;
    let mut available_kb: u64 = 0;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("MemTotal:") => total_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("MemAvailable:") => available_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }

    let total_mb = total_kb / 1024;
    let available_mb = available_kb / 1024;
    let used_mb = total_mb.saturating_sub(available_mb);
    let used_pct = if total_mb > 0 { used_mb as f32 / total_mb as f32 * 100.0 } else { 0.0 };
    RamInfo { total_mb, available_mb, used_mb, used_pct }
}

// ─── NVMe ──────────────────────────────────────────────────────────────────

/// Read NVMe temperatures from all `nvme` hwmon nodes.
pub fn read_nvme() -> Vec<NvmeInfo> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if fs::read_to_string(&name_path).map(|s| s.trim() == "nvme").unwrap_or(false) {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(val) = fs::read_to_string(&temp_path) {
                    if let Ok(raw) = val.trim().parse::<i32>() {
                        let hwmon = entry.file_name().into_string().unwrap_or_default();
                        result.push(NvmeInfo { hwmon, temp_c: raw as f32 / 1000.0 });
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| a.hwmon.cmp(&b.hwmon));
    result
}

// ─── Battery ───────────────────────────────────────────────────────────────

fn read_bat_u32(path: &Path) -> u32 {
    fs::read_to_string(path).ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn read_bat_i32(path: &Path) -> i32 {
    fs::read_to_string(path).ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Read battery info from `/sys/class/power_supply/BAT0/`.
pub fn read_battery() -> Option<BatteryInfo> {
    let base = Path::new("/sys/class/power_supply/BAT0");
    if !base.exists() {
        return None;
    }
    Some(BatteryInfo {
        capacity_pct: fs::read_to_string(base.join("capacity"))
            .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        charge_full_mah: read_bat_u32(&base.join("charge_full")) / 1000,
        charge_now_mah: read_bat_u32(&base.join("charge_now")) / 1000,
        current_now_ma: read_bat_i32(&base.join("current_now")) / 1000,
        cycle_count: read_bat_u32(&base.join("cycle_count")),
        status: fs::read_to_string(base.join("status"))
            .map(|s| s.trim().to_string()).unwrap_or_default(),
    })
}

// ─── All at once ───────────────────────────────────────────────────────────

/// Collect all telemetry in one call.
pub fn collect() -> Telemetry {
    Telemetry {
        cpu: read_cpu(),
        gpu: read_gpu(),
        ram: read_ram(),
        nvme: read_nvme(),
        battery: read_battery(),
    }
}
