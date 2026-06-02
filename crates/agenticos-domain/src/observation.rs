use crate::ids::ObservationId;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub source: ObservationSource,
    pub observed_at: String,
    pub collection_duration_ms: u64,
    pub payload: ObservationPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ObservationSource {
    Process,
    Memory,
    Cgroup,
    Cpu,
    File,
    Device,
    Network,
    Security,
    Benchmark,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ObservationPayload {
    Process(ProcessObservation),
    Memory(MemoryObservation),
    Cgroup(CgroupObservation),
    Cpu(CpuObservation),
    File(FileObservation),
    Device(DeviceObservation),
    Security(SecurityObservation),
    Empty,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProcessObservation {
    pub pid: u32,
    pub parent_pid: u32,
    pub command: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub state: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryObservation {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub pressure_some_avg10: Option<f64>,
    pub pressure_full_avg10: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CgroupObservation {
    pub cgroup_path: String,
    pub memory_current_bytes: u64,
    pub memory_swap_current_bytes: u64,
    pub cpu_usage_usec: u64,
    pub cpu_user_usec: u64,
    pub cpu_system_usec: u64,
    pub cpu_nr_throttled: u64,
    pub cpu_throttled_usec: u64,
    pub processes: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CpuObservation {
    pub pressure_some_avg10: Option<f64>,
    pub pressure_full_avg10: Option<f64>,
    pub nr_running: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FileObservation {
    pub path: String,
    pub event: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceObservation {
    pub device: String,
    pub event: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SecurityObservation {
    pub signal: String,
    pub severity: Severity,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}
