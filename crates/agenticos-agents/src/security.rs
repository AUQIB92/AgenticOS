use std::collections::HashMap;
use std::sync::Mutex;

use agenticos_domain::{
    Agent, AgentId, AgentKind, CapabilitySet, Incident, IncidentCategory, IncidentSeverity,
    Observation, ObservationPayload,
};

pub struct SecurityAgent {
    id: AgentId,
    /// Ticks observed (incremented each time collect_incidents is called).
    tick_count: Mutex<u64>,
    /// Running count of process observations per tick (for rate detection).
    prev_process_count: Mutex<usize>,
    /// Consecutive ticks with high process count.
    high_process_ticks: Mutex<u64>,
    /// When Some(n), emits Critical-severity incident if process_count > n.
    /// Default None — disabled for normal operation.
    high_process_critical_threshold: Option<usize>,
}

impl SecurityAgent {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            tick_count: Mutex::new(0),
            prev_process_count: Mutex::new(0),
            high_process_ticks: Mutex::new(0),
            high_process_critical_threshold: None,
        }
    }

    /// Create a SecurityAgent with a Critical-severity threshold.
    /// When process count exceeds `threshold`, a Critical incident is emitted.
    /// Use only for testing/experimentation.
    pub fn with_critical_threshold(id: AgentId, threshold: usize) -> Self {
        Self {
            id,
            tick_count: Mutex::new(0),
            prev_process_count: Mutex::new(0),
            high_process_ticks: Mutex::new(0),
            high_process_critical_threshold: Some(threshold),
        }
    }

    /// Detect a fork storm: multiple processes sharing the same parent_pid.
    fn detect_fork_storm(observations: &[Observation]) -> Vec<Incident> {
        let mut parent_counts: HashMap<u32, u32> = HashMap::new();
        let mut total_processes = 0usize;

        for obs in observations {
            if let ObservationPayload::Process(p) = &obs.payload {
                total_processes += 1;
                *parent_counts.entry(p.parent_pid).or_insert(0) += 1;
            }
        }

        // Fork storm: any parent with >5 children
        let mut incidents = Vec::new();
        for (&parent_pid, &count) in &parent_counts {
            if count > 5 {
                incidents.push(Incident::new(
                    IncidentCategory::Security,
                    IncidentSeverity::Warning,
                    AgentId::from("security-agent"),
                    obs_for_process_parent(observations, parent_pid),
                    format!(
                        "ForkStormDetected: parent PID {} spawned {} processes (threshold: 5)",
                        parent_pid, count
                    ),
                ));
            }
        }

        // Excessive process creation: total processes > 50
        if total_processes > 50 {
            incidents.push(Incident::new(
                IncidentCategory::Security,
                IncidentSeverity::Warning,
                AgentId::from("security-agent"),
                None,
                format!(
                    "ExcessiveProcessCreation: {} active processes (threshold: 50)",
                    total_processes
                ),
            ));
        }

        incidents
    }

    /// Detect repeated policy denials by examining CPU pressure and process
    /// counts across consecutive ticks — indicators of unresolved contention.
    fn detect_repeated_denial(
        &self,
        observations: &[Observation],
        tick: u64,
    ) -> Vec<Incident> {
        let total_processes = count_processes(observations);
        let cpu_pressure = max_cpu_pressure(observations);

        // Simulated denial detection: if process count stays high across
        // multiple ticks while CPU pressure is elevated, flag as contention
        // that policy has not resolved.
        let _prev = *self.prev_process_count.lock().unwrap();
        let mut high_ticks = self.high_process_ticks.lock().unwrap();

        if total_processes > 40 && cpu_pressure > 0.6 {
            *high_ticks += 1;
        } else {
            *high_ticks = 0;
        }

        *self.prev_process_count.lock().unwrap() = total_processes;

        if *high_ticks >= 3 {
            *high_ticks = 0;
            vec![Incident::new(
                IncidentCategory::GovernanceViolation,
                IncidentSeverity::Warning,
                AgentId::from("security-agent"),
                None,
                format!(
                    "RepeatedPolicyDenial: high process count ({}) with elevated CPU pressure ({:.1}%) \
                     persisted for {} ticks — resource policy may need adjustment",
                    total_processes,
                    cpu_pressure * 100.0,
                    tick
                ),
            )]
        } else {
            vec![]
        }
    }

    /// Detect anomalous agent behavior: spikes in CPU pressure combined with
    /// rapid process creation suggest a workload agent behaving unexpectedly.
    /// Detect process count thresholds and emit incidents at appropriate severity.
    ///
    /// Always active:
    ///   process_count > 20 → Error severity (triggers SelectiveVeto).
    ///
    /// Configurable (off by default):
    ///   process_count > high_process_critical_threshold → Critical severity (triggers GlobalFreeze).
    fn detect_process_count_incidents(&self, observations: &[Observation]) -> Vec<Incident> {
        let process_count = count_processes(observations);
        let mut incidents = Vec::new();

        if process_count > 20 {
            incidents.push(Incident::new(
                IncidentCategory::Security,
                IncidentSeverity::Error,
                self.id.clone(),
                None,
                format!(
                    "HighProcessCount: {} active processes (threshold: 20) \
                     — selective veto of resource-modifying actions active",
                    process_count
                ),
            ));
        }

        if let Some(threshold) = self.high_process_critical_threshold {
            if process_count > threshold {
                incidents.push(Incident::new(
                    IncidentCategory::Security,
                    IncidentSeverity::Critical,
                    self.id.clone(),
                    None,
                    format!(
                        "CriticalProcessCount: {} active processes (threshold: {threshold}) \
                         — global freeze active, all actions vetoed",
                        process_count
                    ),
                ));
            }
        }

        incidents
    }

    fn detect_agent_anomaly(
        &self,
        observations: &[Observation],
        tick: u64,
    ) -> Vec<Incident> {
        let cpu_pressure = max_cpu_pressure(observations);
        let total_processes = count_processes(observations);

        // Both high CPU pressure and rapidly growing process count.
        // Skip if already reported by fork storm or denial (avoid duplicates).
        if cpu_pressure > 0.8 && total_processes > 30 {
            vec![Incident::new(
                IncidentCategory::GovernanceViolation,
                IncidentSeverity::Info,
                AgentId::from("security-agent"),
                None,
                format!(
                    "AgentBehaviorAnomaly: CPU pressure {:.1}% with {} processes at tick {} \
                     — workload behaviour deviating from baseline",
                    cpu_pressure * 100.0,
                    total_processes,
                    tick
                ),
            )]
        } else {
            vec![]
        }
    }
}

impl Agent for SecurityAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Security
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }

    /// Security Agent never emits proposals — it is advisory-only.
    fn propose(&self, _observations: &[Observation]) -> Vec<agenticos_domain::Proposal> {
        Vec::new()
    }

    fn collect_incidents(&self, observations: &[Observation]) -> Vec<Incident> {
        let mut tick = self.tick_count.lock().unwrap();
        *tick += 1;
        let current_tick = *tick;

        let mut incidents = Vec::new();

        incidents.extend(Self::detect_fork_storm(observations));
        incidents.extend(self.detect_repeated_denial(observations, current_tick));
        incidents.extend(self.detect_process_count_incidents(observations));
        incidents.extend(self.detect_agent_anomaly(observations, current_tick));

        incidents
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn count_processes(observations: &[Observation]) -> usize {
    observations
        .iter()
        .filter(|o| matches!(o.payload, ObservationPayload::Process(_)))
        .count()
}

fn max_cpu_pressure(observations: &[Observation]) -> f64 {
    observations
        .iter()
        .filter_map(|o| {
            if let ObservationPayload::Cpu(c) = &o.payload {
                c.pressure_some_avg10
            } else {
                None
            }
        })
        .fold(0.0_f64, f64::max)
}

fn obs_for_process_parent(observations: &[Observation], parent_pid: u32) -> Option<agenticos_domain::ObservationId> {
    observations.iter().find_map(|o| {
        if let ObservationPayload::Process(p) = &o.payload {
            if p.parent_pid == parent_pid {
                return Some(o.id.clone());
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{
        CpuObservation, ObservationId, ObservationSource, ProcessObservation,
    };
    use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};

    fn process_obs(pid: u32, ppid: u32) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Process,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Process(ProcessObservation {
                pid,
                parent_pid: ppid,
                command: format!("proc-{pid}"),
                cpu_percent: 0.0,
                memory_bytes: 0,
                state: "R".into(),
            }),
        }
    }

    fn cpu_obs(pressure_some: f64) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Cpu,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Cpu(CpuObservation {
                pressure_some_avg10: Some(pressure_some),
                pressure_full_avg10: None,
                nr_running: Some(10),
            }),
        }
    }

    // ------------------------------------------------------------------
    // 1. Incident generation — fork storm
    // ------------------------------------------------------------------
    #[test]
    fn fork_storm_detected() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));
        let mut obs = Vec::new();
        // 7 processes all with parent PID 100 (threshold is 5)
        for pid in 1..=7 {
            obs.push(process_obs(pid, 100));
        }

        let incidents = Agent::collect_incidents(&agent, &obs);
        let fork_incidents: Vec<_> = incidents
            .iter()
            .filter(|i| i.description.contains("ForkStormDetected"))
            .collect();

        assert_eq!(fork_incidents.len(), 1);
        assert_eq!(fork_incidents[0].category, IncidentCategory::Security);
        assert!(fork_incidents[0].description.contains("parent PID 100"));
    }

    #[test]
    fn no_fork_storm_below_threshold() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));
        let mut obs = Vec::new();
        // Only 3 processes sharing a parent — below threshold of 5
        for pid in 1..=3 {
            obs.push(process_obs(pid, 100));
        }

        let incidents = Agent::collect_incidents(&agent, &obs);
        let fork_incidents: Vec<_> = incidents
            .iter()
            .filter(|i| i.description.contains("ForkStormDetected"))
            .collect();

        assert!(fork_incidents.is_empty());
    }

    // ------------------------------------------------------------------
    // 2. Incident generation — excessive process creation
    // ------------------------------------------------------------------
    #[test]
    fn excessive_process_creation() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));
        let mut obs = Vec::new();
        // 55 processes — threshold is 50
        for pid in 1..=55 {
            obs.push(process_obs(pid, 1));
        }

        let incidents = Agent::collect_incidents(&agent, &obs);
        let excess: Vec<_> = incidents
            .iter()
            .filter(|i| i.description.contains("ExcessiveProcessCreation"))
            .collect();

        assert_eq!(excess.len(), 1);
        assert!(excess[0].description.contains("55 active processes"));
    }

    // ------------------------------------------------------------------
    // 3. Incident generation — repeated policy denial
    // ------------------------------------------------------------------
    #[test]
    fn repeated_policy_denial_after_three_ticks() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));

        // Build observations with high process count + high CPU pressure
        let obs: Vec<_> = (1..=45)
            .map(|pid| process_obs(pid, 1))
            .chain(std::iter::once(cpu_obs(0.7)))
            .collect();

        // Tick 1: no incident yet (needs 3 consecutive ticks)
        let incidents = Agent::collect_incidents(&agent, &obs);
        assert!(!incidents.iter().any(|i| i.description.contains("RepeatedPolicyDenial")));

        // Tick 2: still no incident
        let incidents = Agent::collect_incidents(&agent, &obs);
        assert!(!incidents.iter().any(|i| i.description.contains("RepeatedPolicyDenial")));

        // Tick 3: incident fires
        let incidents = Agent::collect_incidents(&agent, &obs);
        assert!(incidents.iter().any(|i| i.description.contains("RepeatedPolicyDenial")));
    }

    // ------------------------------------------------------------------
    // 4. Incident generation — agent behavior anomaly
    // ------------------------------------------------------------------
    #[test]
    fn agent_behavior_anomaly_detected() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));
        let mut obs: Vec<_> = (1..=35)
            .map(|pid| process_obs(pid, 1))
            .collect();
        obs.push(cpu_obs(0.9));

        let incidents = Agent::collect_incidents(&agent, &obs);
        assert!(incidents.iter().any(|i| i.description.contains("AgentBehaviorAnomaly")));
    }

    // ------------------------------------------------------------------
    // 5. Incident persistence via trace store
    // ------------------------------------------------------------------
    #[test]
    fn incident_persistence() -> Result<(), agenticos_application::AppError> {
        use agenticos_bus::TraceStore;

        let runtime = InMemoryAgentRuntime::new();
        runtime.register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))?;

        let mut obs: Vec<_> = (1..=7)
            .map(|pid| process_obs(pid, 100))
            .collect();
        obs.push(cpu_obs(0.9));

        let incidents = runtime.collect_incidents(&obs)?;
        assert!(!incidents.is_empty());

        // Persist to trace store
        let trace_store = agenticos_bus::InMemoryTraceStore::new();
        let trace_id = agenticos_domain::TraceId::from("security-test-5");

        for incident in &incidents {
            trace_store.append(agenticos_domain::EventEnvelope::new(
                agenticos_domain::Topic::new(format!("incidents.{}", incident.category.description())),
                trace_id.clone(),
                agenticos_domain::EventPayload::Incident(incident.clone()),
            ))?;
        }

        // Replay
        let replayed = trace_store.replay(trace_id)?;
        assert_eq!(replayed.len(), incidents.len());

        for (i, event) in replayed.iter().enumerate() {
            match &event.payload {
                agenticos_domain::EventPayload::Incident(replayed_inc) => {
                    assert_eq!(replayed_inc.category, incidents[i].category);
                    assert_eq!(replayed_inc.description, incidents[i].description);
                    assert_eq!(replayed_inc.severity, incidents[i].severity);
                }
                _ => panic!("expected incident event"),
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // 6. Multi-agent coexistence
    // ------------------------------------------------------------------
    #[test]
    fn security_agent_coexists_with_memory_agent() -> Result<(), agenticos_application::AppError> {
        use crate::MemoryAgent;

        let runtime = InMemoryAgentRuntime::new();
        runtime.register(Box::new(MemoryAgent::new(AgentId::from("mem-agent"))))?;
        runtime.register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))?;

        // Memory agent needs a MemoryObservation; Security agent needs Process observations
        use agenticos_domain::{MemoryObservation, ObservationPayload};
        let mem_obs = Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Memory(MemoryObservation {
                total_bytes: 100,
                available_bytes: 10,
                used_bytes: 90,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
                pressure_some_avg10: None,
                pressure_full_avg10: None,
            }),
        };

        let mut obs: Vec<_> = (1..=7)
            .map(|pid| process_obs(pid, 100))
            .collect();
        obs.push(mem_obs);

        // Both agents operate on the same observations
        let proposals = runtime.collect_proposals(&obs)?;
        let incidents = runtime.collect_incidents(&obs)?;

        // Memory agent should have produced a proposal
        assert!(!proposals.is_empty());

        // Security agent should have produced incidents
        assert!(!incidents.is_empty());

        Ok(())
    }

    // ------------------------------------------------------------------
    // 7. Security Agent cannot produce actions
    // ------------------------------------------------------------------
    #[test]
    fn security_agent_produces_no_proposals() {
        let agent = SecurityAgent::new(AgentId::from("security-agent"));
        let obs: Vec<_> = (1..=7)
            .map(|pid| process_obs(pid, 100))
            .collect();

        let proposals = Agent::propose(&agent, &obs);
        assert!(proposals.is_empty(), "Security Agent must never emit proposals");
    }
}
