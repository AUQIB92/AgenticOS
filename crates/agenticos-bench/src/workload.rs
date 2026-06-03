use agenticos_domain::{
    CpuObservation, MemoryObservation, Observation, ObservationId, ObservationPayload,
    ObservationSource, ProcessObservation,
};
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};
use serde::Serialize;

/// Enumeration of all supported synthetic workload types.
#[derive(Clone, Debug, Serialize)]
pub enum WorkloadKind {
    CpuContention,
    MemoryPressure,
    Mixed,
    ProcessExplosion,
    IncidentStorm,
}

impl WorkloadKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CpuContention => "cpu-contention",
            Self::MemoryPressure => "memory-pressure",
            Self::Mixed => "mixed",
            Self::ProcessExplosion => "process-explosion",
            Self::IncidentStorm => "incident-storm",
        }
    }
}

/// Configuration for a single workload execution.
#[derive(Clone, Debug)]
pub struct WorkloadConfig {
    pub kind: WorkloadKind,
    pub seed: u64,
    pub tick_count: u64,
    /// Observations generated per tick (where applicable).
    pub obs_per_tick: usize,
    /// For process explosion: total processes per tick.
    pub process_count: usize,
    /// For CPU contention: target avg pressure_some (0.0–1.0).
    pub target_pressure: f64,
    /// For memory pressure: target usage fraction (0.0–1.0).
    pub target_memory_usage: f64,
    /// For incident storm: probability of triggering an incident per observation.
    pub incident_probability: f64,
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            kind: WorkloadKind::CpuContention,
            seed: 42,
            tick_count: 10,
            obs_per_tick: 5,
            process_count: 60,
            target_pressure: 0.85,
            target_memory_usage: 0.85,
            incident_probability: 0.3,
        }
    }
}

/// Generates synthetic observations for a given workload.
pub struct WorkloadGenerator {
    config: WorkloadConfig,
    rng: SmallRng,
    tick: u64,
}

impl WorkloadGenerator {
    pub fn new(config: WorkloadConfig) -> Self {
        let rng = SmallRng::seed_from_u64(config.seed);
        Self {
            config,
            rng,
            tick: 0,
        }
    }

    /// Produce the set of observations for the next tick.
    pub fn next_tick(&mut self) -> Vec<Observation> {
        self.tick += 1;
        match &self.config.kind {
            WorkloadKind::CpuContention => self.gen_cpu_contention(),
            WorkloadKind::MemoryPressure => self.gen_memory_pressure(),
            WorkloadKind::Mixed => self.gen_mixed(),
            WorkloadKind::ProcessExplosion => self.gen_process_explosion(),
            WorkloadKind::IncidentStorm => self.gen_incident_storm(),
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn config(&self) -> &WorkloadConfig {
        &self.config
    }

    // ── Generators ────────────────────────────────────────────────────

    fn gen_cpu_contention(&mut self) -> Vec<Observation> {
        let n = self.config.obs_per_tick;
        let mut obs: Vec<Observation> = Vec::with_capacity(n + 1);

        for _i in 0..n {
            let jitter = (self.rng.next_u32() % 20) as f64 / 100.0;
            let pressure = (self.config.target_pressure + jitter).clamp(0.0, 1.0);
            let nr_running = 4 + (self.rng.next_u32() % 8) as u64;
            obs.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Cpu,
                observed_at: tick_ts(self.tick),
                collection_duration_ms: 5,
                payload: ObservationPayload::Cpu(CpuObservation {
                    pressure_some_avg10: Some(pressure),
                    pressure_full_avg10: Some(pressure * 0.6),
                    nr_running: Some(nr_running),
                }),
            });
        }

        // Process observation last
        let cpu_pct = 85.0 + (self.rng.next_u32() % 15) as f64;
        obs.push(Observation {
            id: ObservationId::new(),
            source: ObservationSource::Process,
            observed_at: tick_ts(self.tick),
            collection_duration_ms: 5,
            payload: ObservationPayload::Process(ProcessObservation {
                pid: (self.tick as u32 * 1000),
                parent_pid: 1,
                command: "cpu-burn".into(),
                cpu_percent: cpu_pct,
                memory_bytes: 50_000_000,
                state: "R".into(),
            }),
        });

        obs
    }

    fn gen_memory_pressure(&mut self) -> Vec<Observation> {
        let n = self.config.obs_per_tick;
        (0..n)
            .map(|_| {
                let jitter = (self.rng.next_u32() % 10) as f64 / 100.0;
                let usage = (self.config.target_memory_usage + jitter).clamp(0.0, 1.0);
                let total: u64 = 16_000_000_000;
                let used = (total as f64 * usage) as u64;
                Observation {
                    id: ObservationId::new(),
                    source: ObservationSource::Memory,
                    observed_at: tick_ts(self.tick),
                    collection_duration_ms: 5,
                    payload: ObservationPayload::Memory(MemoryObservation {
                        total_bytes: total,
                        available_bytes: total - used,
                        used_bytes: used,
                        swap_total_bytes: 2_000_000_000,
                        swap_used_bytes: (500_000_000.0 * usage) as u64,
                        pressure_some_avg10: Some(usage * 0.8),
                        pressure_full_avg10: Some(usage * 0.4),
                    }),
                }
            })
            .collect()
    }

    fn gen_mixed(&mut self) -> Vec<Observation> {
        let mut obs = self.gen_cpu_contention();
        obs.extend(self.gen_memory_pressure());
        obs
    }

    fn gen_process_explosion(&mut self) -> Vec<Observation> {
        let count = self.config.process_count;
        // Spawn many processes under a few parent PIDs to trigger fork storm
        (0..count)
            .map(|i| {
                let parent_pid = if i < count / 3 { 100u32 } else { 200u32 };
                Observation {
                    id: ObservationId::new(),
                    source: ObservationSource::Process,
                    observed_at: tick_ts(self.tick),
                    collection_duration_ms: 5,
                    payload: ObservationPayload::Process(ProcessObservation {
                        pid: (self.tick * 1_000_000 + i as u64) as u32,
                        parent_pid,
                        command: format!("fork-child-{i}"),
                        cpu_percent: 0.0,
                        memory_bytes: 1_000_000,
                        state: "R".into(),
                    }),
                }
            })
            .collect()
    }

    fn gen_incident_storm(&mut self) -> Vec<Observation> {
        // Generate process observations with high fork counts (triggers SecurityAgent)
        self.gen_process_explosion()
    }
}

fn tick_ts(tick: u64) -> String {
    format!("1728000000.{tick:09}Z")
}
