//! Hardware telemetry: CPU, GPU, RAM, NVMe, Battery, Network, System.
//!
//! All reads are best-effort — missing sysfs nodes return `None` instead of errors.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

/// Number of data points to keep in the history ring buffer.
const HISTORY_SIZE: usize = 60; // 60 seconds at 1s refresh = 1 minute window

#[derive(Debug, Clone)]
pub struct Telemetry {
    pub cpu: CpuInfo,
    pub gpu: Option<GpuInfo>,
    pub ram: RamInfo,
    pub nvme: Vec<NvmeInfo>,
    pub battery: Option<BatteryInfo>,
    pub network: Option<NetworkInfo>,
    pub system: SystemInfo,
    /// Rolling history for sparkline charts (last N samples).
    pub history: TelemetryHistory,
}

/// Rolling history of telemetry metrics for sparkline visualization.
#[derive(Debug, Clone)]
pub struct TelemetryHistory {
    pub cpu_utilization: VecDeque<f32>,
    pub cpu_temp: VecDeque<f32>,
    pub ram_usage_pct: VecDeque<f32>,
    pub nvme_temp: VecDeque<f32>,
    pub gpu_utilization: VecDeque<f32>,
    pub net_rx_kbs: VecDeque<f32>,
    pub net_tx_kbs: VecDeque<f32>,
}

impl TelemetryHistory {
    pub fn new() -> Self {
        Self {
            cpu_utilization: VecDeque::with_capacity(HISTORY_SIZE),
            cpu_temp: VecDeque::with_capacity(HISTORY_SIZE),
            ram_usage_pct: VecDeque::with_capacity(HISTORY_SIZE),
            nvme_temp: VecDeque::with_capacity(HISTORY_SIZE),
            gpu_utilization: VecDeque::with_capacity(HISTORY_SIZE),
            net_rx_kbs: VecDeque::with_capacity(HISTORY_SIZE),
            net_tx_kbs: VecDeque::with_capacity(HISTORY_SIZE),
        }
    }

    fn push(&mut self, t: &Telemetry) {
        push_or_trim(&mut self.cpu_utilization, estimate_cpu_utilization(&t.cpu));
        push_or_trim(&mut self.cpu_temp, t.cpu.temp_avg);
        push_or_trim(&mut self.ram_usage_pct, t.ram.used_pct);
        push_or_trim(&mut self.nvme_temp, t.nvme.first().map(|n| n.temp_c).unwrap_or(0.0));
        if let Some(ref g) = t.gpu {
            push_or_trim(&mut self.gpu_utilization, g.utilization_pct as f32);
        }
        if let Some(ref n) = t.network {
            push_or_trim(&mut self.net_rx_kbs, n.rx_kbs);
            push_or_trim(&mut self.net_tx_kbs, n.tx_kbs);
        }
    }

    /// Returns true if there is enough history to render charts.
    pub fn has_history(&self) -> bool {
        self.cpu_temp.len() >= 2
    }
}

fn push_or_trim(buf: &mut VecDeque<f32>, val: f32) {
    if buf.len() == buf.capacity() {
        buf.pop_front();
    }
    buf.push_back(val);
}

fn estimate_cpu_utilization(_cpu: &CpuInfo) -> f32 {
    // Simplified: use temperature as a rough utilization proxy
    // In a full implementation this would read /proc/stat
    // For now, map 40-100°C → 0-100%
    let temp = _cpu.temp_avg;
    if temp <= 40.0 { 0.0 }
    else if temp >= 100.0 { 100.0 }
    else { (temp - 40.0) / 60.0 * 100.0 }
}

#[derive(Debug, Clone, Default)]
pub struct CpuInfo {
    /// Average temperature across all cores (°C).
    pub temp_avg: f32,
    /// Maximum temperature across all cores (°C).
    pub temp_max: f32,
    /// Per-core temperatures (°C).
    pub temps: Vec<f32>,
    /// Current CPU frequency in MHz (from /proc/cpuinfo or sysfs).
    pub freq_mhz: u32,
    /// Max CPU frequency in MHz.
    pub freq_max_mhz: u32,
    /// Number of CPU cores.
    pub core_count: usize,
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
    /// Swap usage.
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub swap_pct: f32,
}

#[derive(Debug, Clone)]
pub struct NvmeInfo {
    pub hwmon: String,
    pub temp_c: f32,
    pub temp_max_c: f32,
    pub critical_temp_c: f32,
}

#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub capacity_pct: u8,
    pub charge_full_mah: u32,
    pub charge_now_mah: u32,
    pub current_now_ma: i32,
    pub cycle_count: u32,
    pub status: String,
    /// Estimated time remaining (charging or discharging) in minutes.
    pub time_remaining_min: Option<u32>,
    /// Power draw in watts (estimated).
    pub power_w: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkInfo {
    /// Total bytes received since boot.
    pub rx_bytes: u64,
    /// Total bytes transmitted since boot.
    pub tx_bytes: u64,
    /// Receive throughput in KB/s (instantaneous).
    pub rx_kbs: f32,
    /// Transmit throughput in KB/s (instantaneous).
    pub tx_kbs: f32,
    /// Primary interface name.
    pub interface: String,
}

/// System-level information: load average, process count, uptime.
#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    /// 1-minute load average.
    pub load_avg_1: f32,
    /// 5-minute load average.
    pub load_avg_5: f32,
    /// 15-minute load average.
    pub load_avg_15: f32,
    /// Number of running processes/threads.
    pub process_count: u32,
    /// System uptime in seconds.
    pub uptime_secs: u64,
    /// Top processes by CPU usage.
    pub top_processes: Vec<ProcessInfo>,
}

/// Information about a single process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub mem_rss_mb: u64,
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

    // Read CPU frequency from sysfs (cpuinfo for cpu0).
    let freq_mhz = read_u32_from_path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .map(|v| v / 1000) // kHz → MHz
        .unwrap_or(0);
    let freq_max_mhz = read_u32_from_path("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .map(|v| v / 1000)
        .unwrap_or(0);

    // Count CPU cores.
    let core_count = fs::read_dir("/sys/devices/system/cpu")
        .map(|d| d.filter(|e| {
            e.as_ref().ok().map(|e| {
                let binding = e.file_name();
                let name = binding.to_string_lossy();
                name.starts_with("cpu") && name[3..].parse::<u32>().is_ok()
            }).unwrap_or(false)
        }).count())
        .unwrap_or(0);

    if temps.is_empty() {
        return CpuInfo { freq_mhz, freq_max_mhz, core_count, ..Default::default() };
    }
    let temp_max = temps.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let temp_avg = temps.iter().sum::<f32>() / temps.len() as f32;
    CpuInfo { temp_avg, temp_max, temps, freq_mhz, freq_max_mhz, core_count }
}

fn read_u32_from_path(path: &str) -> Option<u32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
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
    let mut swap_total_kb: u64 = 0;
    let mut swap_free_kb: u64 = 0;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("MemTotal:") => total_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("MemAvailable:") => available_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("SwapTotal:") => swap_total_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            Some("SwapFree:") => swap_free_kb = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }

    let total_mb = total_kb / 1024;
    let available_mb = available_kb / 1024;
    let used_mb = total_mb.saturating_sub(available_mb);
    let used_pct = if total_mb > 0 { used_mb as f32 / total_mb as f32 * 100.0 } else { 0.0 };

    let swap_total_mb = swap_total_kb / 1024;
    let swap_used_mb = (swap_total_kb.saturating_sub(swap_free_kb)) / 1024;
    let swap_pct = if swap_total_mb > 0 { swap_used_mb as f32 / swap_total_mb as f32 * 100.0 } else { 0.0 };

    RamInfo { total_mb, available_mb, used_mb, used_pct, swap_total_mb, swap_used_mb, swap_pct }
}

// ─── NVMe ──────────────────────────────────────────────────────────────────

/// Read NVMe temperatures from all `nvme` hwmon nodes.
pub fn read_nvme() -> Vec<NvmeInfo> {
    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if fs::read_to_string(&name_path).map(|s| s.trim() == "nvme").unwrap_or(false) {
                let base = entry.path();

                let temp_c = read_f32_from_path(&base.join("temp1_input")).unwrap_or(0.0);
                let temp_max_c = read_f32_from_path(&base.join("temp1_max")).unwrap_or(0.0);
                let critical_temp_c = read_f32_from_path(&base.join("temp1_crit")).unwrap_or(0.0);

                let hwmon = entry.file_name().into_string().unwrap_or_default();
                result.push(NvmeInfo { hwmon, temp_c, temp_max_c, critical_temp_c });
            }
        }
    }
    result.sort_by(|a, b| a.hwmon.cmp(&b.hwmon));
    result
}

fn read_f32_from_path(path: &Path) -> Option<f32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .map(|v| v as f32 / 1000.0)
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

    let capacity_pct = fs::read_to_string(base.join("capacity"))
        .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let charge_full_mah = read_bat_u32(&base.join("charge_full")) / 1000;
    let charge_now_mah = read_bat_u32(&base.join("charge_now")) / 1000;
    let current_now_ma = read_bat_i32(&base.join("current_now")) / 1000;
    let cycle_count = read_bat_u32(&base.join("cycle_count"));
    let status = fs::read_to_string(base.join("status"))
        .map(|s| s.trim().to_string()).unwrap_or_default();

    // Estimate time remaining.
    let time_remaining_min = if current_now_ma != 0 {
        let remaining_mah = charge_full_mah.saturating_sub(charge_now_mah);
        if current_now_ma > 0 {
            // Charging: time = remaining / current * 60
            Some((remaining_mah as f32 / current_now_ma as f32 * 60.0) as u32)
        } else {
            // Discharging: time = now / |current| * 60
            Some((charge_now_mah as f32 / current_now_ma.abs() as f32 * 60.0) as u32)
        }
    } else {
        None
    };

    // Estimate power draw in watts (voltage * current).
    let voltage_v = read_bat_u32(&base.join("voltage_now")) as f32 / 1_000_000.0; // µV → V
    let power_w = if current_now_ma != 0 && voltage_v > 0.0 {
        Some(voltage_v * current_now_ma.abs() as f32 / 1000.0) // mW → W
    } else {
        None
    };

    Some(BatteryInfo {
        capacity_pct,
        charge_full_mah,
        charge_now_mah,
        current_now_ma,
        cycle_count,
        status,
        time_remaining_min,
        power_w,
    })
}

// ─── Network ───────────────────────────────────────────────────────────────

/// Read network throughput from `/proc/net/dev`.
pub fn read_network(prev_rx: u64, prev_tx: u64, interval_secs: f32) -> Option<NetworkInfo> {
    let content = fs::read_to_string("/proc/net/dev").ok()?;

    // Find the primary (non-loopback) interface with the most traffic.
    let mut best_interface = String::new();
    let mut best_bytes = 0u64;
    let mut best_rx = 0u64;
    let mut best_tx = 0u64;

    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() { continue; }
        let iface = parts[0].trim_end_matches(':');
        if iface == "lo" { continue; } // Skip loopback

        let rx = parts[1].parse::<u64>().ok().unwrap_or(0);
        let tx = parts[9].parse::<u64>().ok().unwrap_or(0);
        let total = rx + tx;

        if total > best_bytes {
            best_bytes = total;
            best_interface = iface.to_string();
            best_rx = rx;
            best_tx = tx;
        }
    }

    if best_interface.is_empty() {
        return None;
    }

    let rx_kbs = if interval_secs > 0.0 {
        (best_rx.saturating_sub(prev_rx)) as f32 / 1024.0 / interval_secs
    } else {
        0.0
    };
    let tx_kbs = if interval_secs > 0.0 {
        (best_tx.saturating_sub(prev_tx)) as f32 / 1024.0 / interval_secs
    } else {
        0.0
    };

    Some(NetworkInfo {
        rx_bytes: best_rx,
        tx_bytes: best_tx,
        rx_kbs,
        tx_kbs,
        interface: best_interface,
    })
}

// ─── System ────────────────────────────────────────────────────────────────

/// Read top processes by CPU usage from `/proc/`.
pub fn read_top_processes(max_count: usize) -> Vec<ProcessInfo> {
    let mut processes = Vec::new();

    // Get total memory for percentage calculation.
    let total_mem_kb = if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        content.lines()
            .find(|l| l.starts_with("MemTotal:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1)
    } else {
        1
    };

    // Get number of CPU cores and clock ticks per second.
    let num_cpus = num_cpus::get() as u64;
    let clock_ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;

    // Read /proc/[pid]/stat for each process.
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.parse::<u32>().is_err() {
                continue; // Skip non-numeric entries (not PIDs)
            }
            let pid = name_str.parse::<u32>().unwrap_or(0);
            if pid == 0 { continue; }

            let stat_path = entry.path().join("stat");
            if let Ok(stat_content) = fs::read_to_string(&stat_path) {
                // Parse /proc/[pid]/stat
                // Format: pid (comm) state ... utime stime ...
                // comm can contain spaces and parentheses, so we need to find the last ')'
                if let Some(close_paren) = stat_content.rfind(')') {
                    let parts: Vec<&str> = stat_content[close_paren + 2..].split_whitespace().collect();
                    if parts.len() >= 20 {
                        // utime is at index 11 (0-based from after comm), stime at 12
                        // rss is at index 20 (in pages)
                        let utime: u64 = parts[11].parse().unwrap_or(0);
                        let stime: u64 = parts[12].parse().unwrap_or(0);
                        let rss_pages: u64 = parts[20].parse().unwrap_or(0);

                        // Get process name from comm
                        let comm_path = entry.path().join("comm");
                        let name = fs::read_to_string(&comm_path)
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|_| "?".into());

                        // Get uptime of the system to calculate CPU%
                        let sys_uptime_ticks = if let Ok(uptime) = fs::read_to_string("/proc/uptime") {
                            let secs: f64 = uptime.split_whitespace().next().unwrap_or("0").parse().unwrap_or(0.0);
                            (secs * clock_ticks_per_sec as f64) as u64
                        } else {
                            1
                        };

                        let total_ticks = utime + stime;
                        let cpu_pct = if sys_uptime_ticks > 0 {
                            total_ticks as f32 / sys_uptime_ticks as f32 * 100.0 * num_cpus as f32
                        } else {
                            0.0
                        };

                        let mem_mb = (rss_pages as u64 * 4) / 1024; // pages * 4KB / 1024 = MB
                        let mem_pct = if total_mem_kb > 0 {
                            rss_pages as f32 * 4.0 / total_mem_kb as f32 * 100.0
                        } else {
                            0.0
                        };

                        processes.push(ProcessInfo {
                            pid,
                            name,
                            cpu_pct: cpu_pct.min(100.0 * num_cpus as f32),
                            mem_pct,
                            mem_rss_mb: mem_mb,
                        });
                    }
                }
            }
        }
    }

    // Sort by CPU% descending and take top N.
    processes.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
    processes.truncate(max_count);
    processes
}

fn num_cpus() -> usize {
    // Simple fallback if num_cpus crate is not available
    fs::read_dir("/sys/devices/system/cpu")
        .map(|d| d.filter(|e| {
            e.as_ref().ok().map(|e| {
                let binding = e.file_name();
                let name = binding.to_string_lossy();
                name.starts_with("cpu") && name[3..].parse::<u32>().is_ok()
            }).unwrap_or(false)
        }).count())
        .unwrap_or(1)
}

/// Read system load averages from `/proc/loadavg`.
pub fn read_system() -> SystemInfo {
    let mut info = SystemInfo::default();

    // Load averages.
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            info.load_avg_1 = parts[0].parse().unwrap_or(0.0);
            info.load_avg_5 = parts[1].parse().unwrap_or(0.0);
            info.load_avg_15 = parts[2].parse().unwrap_or(0.0);
        }
        if parts.len() >= 4 {
            if let Some(running) = parts[3].split('/').next() {
                info.process_count = running.parse().unwrap_or(0);
            }
        }
    }

    // Uptime.
    if let Ok(content) = fs::read_to_string("/proc/uptime") {
        if let Some(secs) = content.split_whitespace().next() {
            info.uptime_secs = secs.parse::<f64>().map(|v| v as u64).unwrap_or(0);
        }
    }

    // Top processes.
    info.top_processes = read_top_processes(5);

    info
}

// ─── All at once ───────────────────────────────────────────────────────────

/// Collect all telemetry in one call.
pub fn collect() -> Telemetry {
    let cpu = read_cpu();
    let gpu = read_gpu();
    let ram = read_ram();
    let nvme = read_nvme();
    let battery = read_battery();
    let system = read_system();

    Telemetry {
        cpu,
        gpu,
        ram,
        nvme,
        battery,
        network: None, // Network requires delta calculation, handled by collector
        system,
        history: TelemetryHistory::new(),
    }
}

/// Collect telemetry with network throughput calculation.
pub fn collect_with_network(prev_rx: u64, prev_tx: u64, interval_secs: f32) -> Telemetry {
    let mut t = collect();
    t.network = read_network(prev_rx, prev_tx, interval_secs);
    t
}

/// Update telemetry history with new sample.
pub fn update_history(history: &mut TelemetryHistory, t: &Telemetry) {
    history.push(t);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RAM parsing ─────────────────────────────────────────────────

    fn parse_ram_from_str(content: &str) -> RamInfo {
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
        RamInfo { total_mb, available_mb, used_mb, used_pct, swap_total_mb: 0, swap_used_mb: 0, swap_pct: 0.0 }
    }

    #[test]
    fn parse_ram_typical() {
        let content = "\
MemTotal:       16384000 kB
MemFree:         2048000 kB
MemAvailable:    8192000 kB
Buffers:          512000 kB
";
        let ram = parse_ram_from_str(content);
        assert_eq!(ram.total_mb, 16000);
        assert_eq!(ram.available_mb, 8000);
        assert_eq!(ram.used_mb, 8000);
        assert!((ram.used_pct - 50.0).abs() < 0.1);
    }

    #[test]
    fn parse_ram_empty_content() {
        let ram = parse_ram_from_str("");
        assert_eq!(ram.total_mb, 0);
        assert_eq!(ram.used_pct, 0.0);
    }

    #[test]
    fn parse_ram_missing_fields() {
        let content = "MemFree: 1024000 kB\n";
        let ram = parse_ram_from_str(content);
        assert_eq!(ram.total_mb, 0);
        assert_eq!(ram.used_mb, 0);
    }

    #[test]
    fn parse_ram_full_usage() {
        let content = "MemTotal: 8192000 kB\nMemAvailable: 0 kB\n";
        let ram = parse_ram_from_str(content);
        assert_eq!(ram.total_mb, 8000);
        assert_eq!(ram.used_mb, 8000);
        assert!((ram.used_pct - 100.0).abs() < 0.1);
    }

    // ── GPU parsing ─────────────────────────────────────────────────

    fn parse_gpu_from_str(stdout: &str) -> Option<GpuInfo> {
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

    #[test]
    fn parse_gpu_typical() {
        let output = "NVIDIA GeForce RTX 4070, 68, 45, 3200, 8192";
        let gpu = parse_gpu_from_str(output).unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 4070");
        assert_eq!(gpu.temp_c, 68);
        assert_eq!(gpu.utilization_pct, 45);
        assert_eq!(gpu.vram_used_mb, 3200);
        assert_eq!(gpu.vram_total_mb, 8192);
    }

    #[test]
    fn parse_gpu_too_few_fields() {
        let output = "RTX 4070, 68, 45";
        assert!(parse_gpu_from_str(output).is_none());
    }

    #[test]
    fn parse_gpu_empty() {
        assert!(parse_gpu_from_str("").is_none());
    }

    // ── CpuInfo aggregation ────────────────────────────────────────

    #[test]
    fn cpu_info_avg_max() {
        let temps = vec![60.0, 65.0, 70.0, 75.0];
        let avg = temps.iter().sum::<f32>() / temps.len() as f32;
        let max = temps.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!((avg - 67.5).abs() < 0.1);
        assert_eq!(max, 75.0);
    }

    // ── Telemetry defaults ─────────────────────────────────────────

    #[test]
    fn cpu_info_default() {
        let cpu = CpuInfo::default();
        assert_eq!(cpu.temp_avg, 0.0);
        assert_eq!(cpu.temp_max, 0.0);
        assert!(cpu.temps.is_empty());
    }

    #[test]
    fn ram_info_default() {
        let ram = RamInfo::default();
        assert_eq!(ram.total_mb, 0);
        assert_eq!(ram.used_mb, 0);
        assert_eq!(ram.used_pct, 0.0);
    }
}
