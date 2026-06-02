use agenticos_domain::{CgroupObservation, CpuObservation, MemoryObservation};

use crate::traits::{CgroupCollector, CpuCollector, MemoryCollector, ProcessCollector};

pub struct NoopProcessCollector;

impl NoopProcessCollector {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessCollector for NoopProcessCollector {
    fn collect_processes(&self) -> Vec<agenticos_domain::ProcessObservation> {
        Vec::new()
    }
}

pub struct NoopMemoryCollector;

impl NoopMemoryCollector {
    pub fn new() -> Self {
        Self
    }
}

impl MemoryCollector for NoopMemoryCollector {
    fn collect_memory(&self) -> MemoryObservation {
        MemoryObservation {
            total_bytes: 0,
            available_bytes: 0,
            used_bytes: 0,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            pressure_some_avg10: None,
            pressure_full_avg10: None,
        }
    }
}

pub struct NoopCpuCollector;

impl NoopCpuCollector {
    pub fn new() -> Self {
        Self
    }
}

impl CpuCollector for NoopCpuCollector {
    fn collect_cpu(&self) -> CpuObservation {
        CpuObservation {
            pressure_some_avg10: None,
            pressure_full_avg10: None,
            nr_running: None,
        }
    }
}

pub struct NoopCgroupCollector;

impl NoopCgroupCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn with_root(_root: std::path::PathBuf) -> Self {
        Self
    }
}

impl CgroupCollector for NoopCgroupCollector {
    fn collect_cgroups(&self) -> Vec<CgroupObservation> {
        Vec::new()
    }
}
