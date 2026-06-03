use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_domain::{CgroupObservation, CpuObservation, MemoryObservation, ProcessObservation};

use crate::parsing::{
    parse_meminfo_value, parse_pressure_avg10, parse_proc_stat,
};
use crate::traits::{CgroupCollector, CpuCollector, MemoryCollector, ProcessCollector};

// ---------------------------------------------------------------------------
// Process collector — reads /proc/<pid>/stat and /proc/<pid>/status
// ---------------------------------------------------------------------------

pub struct ProcfsProcessCollector {
    proc_root: PathBuf,
}

impl ProcfsProcessCollector {
    pub fn new() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
        }
    }

    #[allow(dead_code)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { proc_root: root }
    }
}

impl Default for ProcfsProcessCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessCollector for ProcfsProcessCollector {
    fn collect_processes(&self) -> Vec<ProcessObservation> {
        let mut processes = Vec::new();

        let dir = match std::fs::read_dir(&self.proc_root) {
            Ok(d) => d,
            Err(_) => return processes,
        };

        for entry in dir.flatten() {
            let path = entry.path();
            let pid_str = match path.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_owned(),
                None => continue,
            };

            let pid: u32 = match pid_str.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };

            let stat_path = path.join("stat");
            let status_path = path.join("status");

            let stat_content = match std::fs::read_to_string(&stat_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let status_content = match std::fs::read_to_string(&status_path) {
                Ok(c) => c,
                Err(_) => String::new(),
            };

            processes.push(parse_proc_stat(&stat_content, pid, &status_content));
        }

        processes
    }
}

// ---------------------------------------------------------------------------
// Memory collector — reads /proc/meminfo and /proc/pressure/memory
// ---------------------------------------------------------------------------

pub struct ProcfsMemoryCollector {
    proc_root: PathBuf,
}

impl ProcfsMemoryCollector {
    pub fn new() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
        }
    }

    #[allow(dead_code)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { proc_root: root }
    }
}

impl Default for ProcfsMemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryCollector for ProcfsMemoryCollector {
    fn collect_memory(&self) -> MemoryObservation {
        let meminfo_path = self.proc_root.join("meminfo");
        let meminfo_content = std::fs::read_to_string(&meminfo_path).unwrap_or_default();

        let pressure_path = self.proc_root.join("pressure/memory");
        let pressure_content = std::fs::read_to_string(&pressure_path).ok();

        let total_kb = parse_meminfo_value(&meminfo_content, "MemTotal:");
        let free_kb = parse_meminfo_value(&meminfo_content, "MemFree:");
        let available_kb = parse_meminfo_value(&meminfo_content, "MemAvailable:");
        let swap_total_kb = parse_meminfo_value(&meminfo_content, "SwapTotal:");
        let swap_free_kb = parse_meminfo_value(&meminfo_content, "SwapFree:");

        let total_bytes = total_kb.saturating_mul(1024);
        let available_bytes = available_kb.saturating_mul(1024);
        let used_bytes = total_bytes.saturating_sub(free_kb.saturating_mul(1024));
        let swap_total_bytes = swap_total_kb.saturating_mul(1024);
        let swap_used_bytes = swap_total_kb.saturating_sub(swap_free_kb).saturating_mul(1024);

        let (pressure_some, pressure_full) = if let Some(content) = pressure_content {
            (
                parse_pressure_avg10(&content, "some"),
                parse_pressure_avg10(&content, "full"),
            )
        } else {
            (None, None)
        };

        MemoryObservation {
            total_bytes,
            available_bytes,
            used_bytes,
            swap_total_bytes,
            swap_used_bytes,
            pressure_some_avg10: pressure_some,
            pressure_full_avg10: pressure_full,
        }
    }
}

// ---------------------------------------------------------------------------
// CPU collector — reads /proc/pressure/cpu and /proc/stat for nr_running
// ---------------------------------------------------------------------------

pub struct ProcfsCpuCollector {
    proc_root: PathBuf,
}

impl ProcfsCpuCollector {
    pub fn new() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
        }
    }

    #[allow(dead_code)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { proc_root: root }
    }
}

impl Default for ProcfsCpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuCollector for ProcfsCpuCollector {
    fn collect_cpu(&self) -> CpuObservation {
        let pressure_path = self.proc_root.join("pressure/cpu");
        let pressure_content = std::fs::read_to_string(&pressure_path).ok();

        let (pressure_some, pressure_full) = if let Some(content) = pressure_content {
            (
                parse_pressure_avg10(&content, "some"),
                parse_pressure_avg10(&content, "full"),
            )
        } else {
            (None, None)
        };

        // Parse nr_running from /proc/stat (line "procs_running")
        let stat_path = self.proc_root.join("stat");
        let nr_running = std::fs::read_to_string(&stat_path).ok().and_then(|content| {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("procs_running ") {
                    return val.trim().parse::<u64>().ok();
                }
            }
            None
        });

        CpuObservation {
            pressure_some_avg10: pressure_some,
            pressure_full_avg10: pressure_full,
            nr_running,
        }
    }
}

// ---------------------------------------------------------------------------
// Cgroup collector — reads cgroup v2 files
// ---------------------------------------------------------------------------

pub struct CgroupFsCollector {
    cgroup_root: PathBuf,
}

impl CgroupFsCollector {
    pub fn new(cgroup_root: PathBuf) -> Self {
        Self { cgroup_root }
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self::new(root)
    }
}

impl CgroupCollector for CgroupFsCollector {
    fn collect_cgroups(&self) -> Vec<CgroupObservation> {
        let mut result = Vec::new();

        if !self.cgroup_root.exists() {
            return result;
        }

        let entries = match std::fs::read_dir(&self.cgroup_root) {
            Ok(d) => d,
            Err(_) => return result,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(obs) = read_cgroup(&path) {
                    result.push(obs);
                }
            }
        }

        result
    }
}

fn read_cgroup(group_path: &std::path::Path) -> Result<CgroupObservation, String> {
    let cgroup_path = group_path.to_string_lossy().into_owned();

    let memory_current = read_u64(group_path, "memory.current").unwrap_or(0);
    let memory_swap_current = read_u64(group_path, "memory.swap.current").unwrap_or(0);

    let (cpu_usage, cpu_user, cpu_system, nr_throttled, throttled_usec) =
        read_cpu_stat(group_path).unwrap_or_default();
    let processes = count_procs(group_path).unwrap_or(0);

    Ok(CgroupObservation {
        cgroup_path,
        memory_current_bytes: memory_current,
        memory_swap_current_bytes: memory_swap_current,
        cpu_usage_usec: cpu_usage,
        cpu_user_usec: cpu_user,
        cpu_system_usec: cpu_system,
        cpu_nr_throttled: nr_throttled,
        cpu_throttled_usec: throttled_usec,
        processes,
    })
}

fn read_u64(group_path: &std::path::Path, filename: &str) -> Option<u64> {
    let content = std::fs::read_to_string(group_path.join(filename)).ok()?;
    content.trim().parse().ok()
}

fn read_cpu_stat(group_path: &std::path::Path) -> Option<(u64, u64, u64, u64, u64)> {
    let content = std::fs::read_to_string(group_path.join("cpu.stat")).ok()?;
    let mut usage = 0u64;
    let mut user = 0u64;
    let mut system = 0u64;
    let mut nr_throttled = 0u64;
    let mut throttled_usec = 0u64;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("usage_usec ") {
            usage = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("user_usec ") {
            user = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("system_usec ") {
            system = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("nr_throttled ") {
            nr_throttled = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("throttled_usec ") {
            throttled_usec = val.trim().parse().unwrap_or(0);
        }
    }
    Some((usage, user, system, nr_throttled, throttled_usec))
}

fn count_procs(group_path: &std::path::Path) -> Option<u32> {
    let content = std::fs::read_to_string(group_path.join("cgroup.procs")).ok()?;
    Some(content.lines().count() as u32)
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

pub fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}
