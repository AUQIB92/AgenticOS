use std::path::PathBuf;
use std::time::Instant;

use agenticos_application::{AppError, ObserverPort};
use agenticos_domain::{
    Observation, ObservationId, ObservationPayload, ObservationSource,
};
#[cfg(target_os = "linux")]
use crate::linux::{
    timestamp as ts_impl, CgroupFsCollector, ProcfsCpuCollector, ProcfsMemoryCollector,
    ProcfsProcessCollector,
};
#[cfg(not(target_os = "linux"))]
use crate::noop::{
    NoopCgroupCollector as CgroupFsCollector, NoopCpuCollector as ProcfsCpuCollector,
    NoopMemoryCollector as ProcfsMemoryCollector,
    NoopProcessCollector as ProcfsProcessCollector,
};

use crate::traits::{CgroupCollector, CpuCollector, MemoryCollector, ProcessCollector};

/// Single entry point for all system observations.
pub struct SystemSampler {
    process_collector: ProcfsProcessCollector,
    memory_collector: ProcfsMemoryCollector,
    cpu_collector: ProcfsCpuCollector,
    cgroup_collector: Option<CgroupFsCollector>,
}

impl SystemSampler {
    pub fn new(cgroup_root: Option<PathBuf>) -> Self {
        Self {
            process_collector: ProcfsProcessCollector::new(),
            memory_collector: ProcfsMemoryCollector::new(),
            cpu_collector: ProcfsCpuCollector::new(),
            cgroup_collector: cgroup_root.map(CgroupFsCollector::with_root),
        }
    }
}

impl ObserverPort for SystemSampler {
    fn observe(&self) -> Result<Vec<Observation>, AppError> {
        let observed_at = timestamp();
        let start = Instant::now();

        let process_obs = self.process_collector.collect_processes();
        let memory_obs = self.memory_collector.collect_memory();
        let cpu_obs = self.cpu_collector.collect_cpu();
        let cgroup_obs: Vec<agenticos_domain::CgroupObservation> = self
            .cgroup_collector
            .as_ref()
            .map(|c| c.collect_cgroups())
            .unwrap_or_default();

        let duration_ms = start.elapsed().as_millis() as u64;

        let mut observations =
            Vec::with_capacity(process_obs.len() + 2 + cgroup_obs.len());

        for p in process_obs {
            observations.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Process,
                observed_at: observed_at.clone(),
                collection_duration_ms: duration_ms,
                payload: ObservationPayload::Process(p),
            });
        }

        observations.push(Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: observed_at.clone(),
            collection_duration_ms: duration_ms,
            payload: ObservationPayload::Memory(memory_obs),
        });

        observations.push(Observation {
            id: ObservationId::new(),
            source: ObservationSource::Cpu,
            observed_at: observed_at.clone(),
            collection_duration_ms: duration_ms,
            payload: ObservationPayload::Cpu(cpu_obs),
        });

        for cg in cgroup_obs {
            observations.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Cgroup,
                observed_at: observed_at.clone(),
                collection_duration_ms: duration_ms,
                payload: ObservationPayload::Cgroup(cg),
            });
        }

        Ok(observations)
    }
}

#[cfg(target_os = "linux")]
fn timestamp() -> String {
    ts_impl()
}

#[cfg(not(target_os = "linux"))]
fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}
