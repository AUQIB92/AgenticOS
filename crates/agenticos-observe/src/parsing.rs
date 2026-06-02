/// Parse /proc/<pid>/stat content and extract key fields.
///
/// Format: pid (comm) state ppid ...
/// Field 2 (comm) is in parentheses and may contain spaces.
#[allow(dead_code)]
pub fn parse_proc_stat(stat: &str, pid: u32, status: &str) -> agenticos_domain::ProcessObservation {
    let comm_start = match stat.find('(') {
        Some(i) => i,
        None => {
            return zeroed_process(pid);
        }
    };
    let comm_end = match stat.rfind(')') {
        Some(i) => i,
        None => {
            return zeroed_process(pid);
        }
    };

    let command = &stat[comm_start + 1..comm_end];
    let rest = stat[comm_end + 2..].trim();
    let fields: Vec<&str> = rest.split_whitespace().collect();

    let state = fields.first().unwrap_or(&"").to_string();
    let parent_pid: u32 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let memory_bytes = parse_vmrss_kb(status).saturating_mul(1024);

    agenticos_domain::ProcessObservation {
        pid,
        parent_pid,
        command: command.to_owned(),
        cpu_percent: 0.0,
        memory_bytes,
        state,
    }
}

fn zeroed_process(pid: u32) -> agenticos_domain::ProcessObservation {
    agenticos_domain::ProcessObservation {
        pid,
        parent_pid: 0,
        command: String::new(),
        cpu_percent: 0.0,
        memory_bytes: 0,
        state: String::new(),
    }
}

/// Extract VmRSS value (in kB) from /proc/<pid>/status content.
#[allow(dead_code)]
pub fn parse_vmrss_kb(status: &str) -> u64 {
    for line in status.lines() {
        if let Some(val) = line.strip_prefix("VmRSS:") {
            let cleaned: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
            return cleaned.parse().unwrap_or(0);
        }
    }
    0
}

/// Extract an integer value from /proc/meminfo-style content.
/// e.g. `parse_meminfo_value("MemTotal: 16384000 kB\n", "MemTotal:")` -> 16384000
#[allow(dead_code)]
pub fn parse_meminfo_value(content: &str, key: &str) -> u64 {
    for line in content.lines() {
        if let Some(val) = line.strip_prefix(key) {
            let cleaned: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
            return cleaned.parse().unwrap_or(0);
        }
    }
    0
}

/// Parse PSI avg10 value from /proc/pressure/* content.
/// e.g. `parse_pressure_avg10("some avg10=1.23 ...", "some")` -> Some(1.23)
#[allow(dead_code)]
pub fn parse_pressure_avg10(content: &str, prefix: &str) -> Option<f64> {
    for line in content.lines() {
        if line.starts_with(prefix) {
            for part in line.split_whitespace() {
                if let Some(val) = part.strip_prefix("avg10=") {
                    return val.parse::<f64>().ok();
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // process parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_proc_stat_basic() {
        let stat = "1234 (bash) S 1220 1234 1220 34816 1234 4194304 1234 0 0 0 0 0 0 0 20 0 1 0 12345 67890 123 456 789 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let status = "Name: bash\nVmRSS: 4096 kB\n";
        let obs = parse_proc_stat(stat, 1234, status);

        assert_eq!(obs.pid, 1234);
        assert_eq!(obs.parent_pid, 1220);
        assert_eq!(obs.command, "bash");
        assert_eq!(obs.state, "S");
        assert_eq!(obs.memory_bytes, 4096 * 1024);
    }

    #[test]
    fn parse_proc_stat_no_parenthesis_returns_defaults() {
        let obs = parse_proc_stat("1234 no-parens", 1234, "");
        assert_eq!(obs.pid, 1234);
        assert_eq!(obs.parent_pid, 0);
        assert_eq!(obs.command, "");
    }

    #[test]
    fn parse_vmrss_from_status() {
        let status = "Name: test\nVmRSS: 8192 kB\nThreads: 1\n";
        assert_eq!(parse_vmrss_kb(status), 8192);
        assert_eq!(parse_vmrss_kb(""), 0);
        assert_eq!(parse_vmrss_kb("Name: test\n"), 0);
    }

    // -----------------------------------------------------------------------
    // meminfo parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_meminfo_values() {
        let content = "MemTotal:       16384000 kB\nMemFree:         2048000 kB\nMemAvailable:    8192000 kB\nSwapTotal:       8388608 kB\nSwapFree:        4194304 kB\n";
        assert_eq!(parse_meminfo_value(content, "MemTotal:"), 16384000);
        assert_eq!(parse_meminfo_value(content, "MemFree:"), 2048000);
        assert_eq!(parse_meminfo_value(content, "MemAvailable:"), 8192000);
        assert_eq!(parse_meminfo_value(content, "SwapTotal:"), 8388608);
        assert_eq!(parse_meminfo_value(content, "SwapFree:"), 4194304);
    }

    #[test]
    fn parse_meminfo_missing_key() {
        assert_eq!(parse_meminfo_value("MemTotal: 123 kB\n", "SwapTotal:"), 0);
    }

    // -----------------------------------------------------------------------
    // pressure parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_pressure_values() {
        let content = "some avg10=1.23 avg60=4.56 avg300=7.89 total=1000\nfull avg10=0.50 avg60=1.00 avg300=2.00 total=500\n";
        assert_eq!(parse_pressure_avg10(content, "some"), Some(1.23));
        assert_eq!(parse_pressure_avg10(content, "full"), Some(0.50));
    }

    #[test]
    fn parse_pressure_missing_prefix() {
        assert_eq!(parse_pressure_avg10("some avg10=1.23\n", "full"), None);
    }

    // -----------------------------------------------------------------------
    // cgroup stat parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_cpu_stat_values() {
        let content = "usage_usec 123456\nuser_usec 100000\nsystem_usec 23456\nnr_periods 100\n";
        let mut usage = 0u64;
        let mut user = 0u64;
        let mut system = 0u64;
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("usage_usec ") {
                usage = val.trim().parse().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("user_usec ") {
                user = val.trim().parse().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("system_usec ") {
                system = val.trim().parse().unwrap_or(0);
            }
        }
        assert_eq!(usage, 123456);
        assert_eq!(user, 100000);
        assert_eq!(system, 23456);
    }

    #[test]
    fn parse_u64_from_content() {
        assert_eq!("1024".parse::<u64>(), Ok(1024));
        assert!("not_a_number".parse::<u64>().is_err());
    }
}
