use agenticos_domain::{CgroupObservation, CpuObservation, MemoryObservation, ProcessObservation};

/// Collects process observations from the system.
/// On Linux, reads /proc directly. On other platforms, returns empty.
pub trait ProcessCollector: Send + Sync {
    fn collect_processes(&self) -> Vec<ProcessObservation>;
}

/// Collects a single system-wide memory observation.
/// On Linux, reads /proc/meminfo and /proc/pressure/memory.
/// On other platforms, returns zeroed data.
pub trait MemoryCollector: Send + Sync {
    fn collect_memory(&self) -> MemoryObservation;
}

/// Collects a single system-wide CPU observation.
/// On Linux, reads /proc/pressure/cpu and /proc/stat for run queue.
/// On other platforms, returns zeroed data.
pub trait CpuCollector: Send + Sync {
    fn collect_cpu(&self) -> CpuObservation;
}

/// Collects observations from cgroup v2 subtrees.
/// On Linux, reads cgroupfs files. On other platforms, returns empty.
pub trait CgroupCollector: Send + Sync {
    fn collect_cgroups(&self) -> Vec<CgroupObservation>;
}
