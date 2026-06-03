use agenticos_domain::{
    AgentId, Observation, ObservationPayload, Recommendation, WorkloadClass,
    WorkloadObservationSummary,
};

use crate::{LlmProvider, RecommendationContext};

const DATABASE_PROCESSES: &[&str] = &[
    "postgres", "mysql", "mysqld", "mariadb", "mongod", "mongodb",
    "redis", "sqlite", "oracle", "mssql",
];

const BUILD_PROCESSES: &[&str] = &[
    "gcc", "g++", "rustc", "cargo", "make", "cmake", "clang",
    "msbuild", "mvn", "gradle", "ninja", "cc", "gdc",
];

const INTERACTIVE_PROCESSES: &[&str] = &[
    "Xorg", "wayland", "gnome-shell", "kde", "terminal", "konsole",
    "gnome-terminal", "firefox", "chrome", "chromium", "electron",
    "xterm", "alacritty", "kitty", "foot",
];

const SYSTEM_PROCESSES: &[&str] = &[
    "systemd", "journald", "udev", "dbus", "polkit", "resolved",
    "networkd", "syslogd", "cron", "sshd",
];

pub struct WorkloadClassifier {
    pub agent_id: AgentId,
}

impl WorkloadClassifier {
    pub fn new(agent_id: AgentId) -> Self {
        Self { agent_id }
    }

    pub fn classify(
        &self,
        summary: &WorkloadObservationSummary,
    ) -> (WorkloadClass, f64, String) {
        if has_any_name(&summary.process_names, DATABASE_PROCESSES)
            && summary.cpu_utilization > 50.0
        {
            return (
                WorkloadClass::Database,
                0.92,
                format!(
                    "High sustained CPU utilization ({:.0}%) with database process signature",
                    summary.cpu_utilization
                ),
            );
        }

        let compiler_count = count_matching(&summary.process_names, BUILD_PROCESSES);
        if compiler_count >= 2 {
            return (
                WorkloadClass::Build,
                0.88,
                format!(
                    "{} compiler processes detected ({:.0}% CPU, {} processes)",
                    compiler_count,
                    summary.cpu_utilization,
                    summary.process_count
                ),
            );
        }

        if has_any_name(&summary.process_names, INTERACTIVE_PROCESSES)
            && summary.cpu_utilization < 40.0
        {
            return (
                WorkloadClass::Interactive,
                0.85,
                format!(
                    "User-facing processes with low CPU utilization ({:.0}%)",
                    summary.cpu_utilization
                ),
            );
        }

        if has_any_name(&summary.process_names, SYSTEM_PROCESSES)
            && summary.process_count <= 15
        {
            return (
                WorkloadClass::SystemService,
                0.80,
                format!(
                    "System service processes detected ({:.0}% CPU, {} processes)",
                    summary.cpu_utilization,
                    summary.process_count
                ),
            );
        }

        if summary.cpu_utilization > 70.0 && summary.process_count > 20 {
            return (
                WorkloadClass::Batch,
                0.75,
                format!(
                    "High CPU utilization ({:.0}%) with {} processes, no specific workload signature",
                    summary.cpu_utilization,
                    summary.process_count
                ),
            );
        }

        (
            WorkloadClass::Unknown,
            0.50,
            format!(
                "Insufficient data to classify workload (CPU={:.0}%, {} processes)",
                summary.cpu_utilization,
                summary.process_count
            ),
        )
    }
}

impl LlmProvider for WorkloadClassifier {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation {
        let summary = parse_summary_from_context(&context);
        let (classification, confidence, reasoning) = self.classify(&summary);
        Recommendation::new(
            self.agent_id.clone(),
            confidence,
            format!("Workload classified as {}", classification.label()),
            reasoning,
        )
    }
}

fn has_any_name(names: &[String], keywords: &[&str]) -> bool {
    names.iter().any(|n| {
        let lower = n.to_lowercase();
        keywords.iter().any(|k| lower.contains(k))
    })
}

fn count_matching(names: &[String], keywords: &[&str]) -> usize {
    names
        .iter()
        .filter(|n| {
            let lower = n.to_lowercase();
            keywords.iter().any(|k| lower.contains(k))
        })
        .count()
}

fn parse_summary_from_context(context: &RecommendationContext) -> WorkloadObservationSummary {
    let summary_str = &context.observation_summary;
    let cpu_str = summary_str
        .lines()
        .find(|l| l.starts_with("cpu:"))
        .and_then(|l| l.trim_start_matches("cpu:").trim().split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(50.0);

    let mem_str = summary_str
        .lines()
        .find(|l| l.starts_with("mem:"))
        .and_then(|l| l.trim_start_matches("mem:").trim().split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(50.0);

    let proc_count = summary_str
        .lines()
        .find(|l| l.starts_with("procs:"))
        .and_then(|l| l.trim_start_matches("procs:").trim().split_whitespace().next())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let pressure = summary_str
        .lines()
        .find(|l| l.starts_with("pressure:"))
        .and_then(|l| l.trim_start_matches("pressure:").trim().split_whitespace().next())
        .and_then(|s| s.parse::<f64>().ok());

    let names: Vec<String> = summary_str
        .lines()
        .find(|l| l.starts_with("names:"))
        .map(|l| {
            l.trim_start_matches("names:")
                .trim()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    WorkloadObservationSummary::new(cpu_str, mem_str, proc_count, names, pressure)
}

fn extract_process_observations(observations: &[Observation]) -> Vec<&str> {
    observations
        .iter()
        .filter_map(|o| match &o.payload {
            ObservationPayload::Process(p) => Some(p.command.as_str()),
            _ => None,
        })
        .collect()
}

fn extract_cgroup_processes(observations: &[Observation]) -> Option<u32> {
    observations.iter().find_map(|o| match &o.payload {
        ObservationPayload::Cgroup(c) => Some(c.processes),
        _ => None,
    })
}

fn extract_cpu_utilization(observations: &[Observation]) -> Option<f64> {
    let process_cpus: Vec<f64> = observations
        .iter()
        .filter_map(|o| match &o.payload {
            ObservationPayload::Process(p) => Some(p.cpu_percent),
            _ => None,
        })
        .collect();
    if !process_cpus.is_empty() {
        process_cpus.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).copied()
    } else {
        observations.iter().find_map(|o| match &o.payload {
            ObservationPayload::Cgroup(c) => {
                let usage = c.cpu_usage_usec;
                if usage > 0 {
                    Some((usage as f64 / 10_000.0).min(100.0))
                } else {
                    None
                }
            }
            _ => None,
        })
    }
}

fn extract_memory_utilization(observations: &[Observation]) -> Option<f64> {
    observations.iter().find_map(|o| match &o.payload {
        ObservationPayload::Memory(m) if m.total_bytes > 0 => {
            Some((m.used_bytes as f64 / m.total_bytes as f64) * 100.0)
        }
        _ => None,
    })
}

fn extract_cpu_pressure(observations: &[Observation]) -> Option<f64> {
    observations.iter().find_map(|o| match &o.payload {
        ObservationPayload::Cpu(c) => c.pressure_some_avg10,
        ObservationPayload::Memory(m) => m.pressure_some_avg10,
        _ => None,
    })
}

fn collect_process_names(observations: &[Observation]) -> Vec<String> {
    observations
        .iter()
        .filter_map(|o| match &o.payload {
            ObservationPayload::Process(p) => Some(p.command.clone()),
            ObservationPayload::Cgroup(c) => {
                if c.processes > 0 {
                    Some(format!("<{} processes>", c.processes))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

pub struct WorkloadClassificationAgent {
    agent_id: AgentId,
    classifier: WorkloadClassifier,
    classification_count: u64,
    class_counts: std::collections::HashMap<String, u64>,
}

impl WorkloadClassificationAgent {
    pub fn new(agent_id: AgentId) -> Self {
        let classifier = WorkloadClassifier::new(agent_id.clone());
        Self {
            agent_id,
            classifier,
            classification_count: 0,
            class_counts: std::collections::HashMap::new(),
        }
    }

    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    pub fn classification_count(&self) -> u64 {
        self.classification_count
    }

    pub fn class_count(&self, class: &WorkloadClass) -> u64 {
        self.class_counts.get(class.label()).copied().unwrap_or(0)
    }

    pub fn classify_workload(&mut self, observations: &[Observation]) -> Recommendation {
        let summary = self.build_summary(observations);
        let context = self.summary_to_context(&summary);
        let recommendation = self.classifier.generate_recommendation(context);

        self.classification_count += 1;
        let class_label = extract_class_label(&recommendation);
        *self.class_counts.entry(class_label).or_insert(0) += 1;

        recommendation
    }

    fn build_summary(&self, observations: &[Observation]) -> WorkloadObservationSummary {
        let cpu = extract_cpu_utilization(observations).unwrap_or(50.0);
        let mem = extract_memory_utilization(observations).unwrap_or(50.0);
        let proc_count = extract_cgroup_processes(observations).unwrap_or(
            extract_process_observations(observations).len() as u32,
        );
        let names = collect_process_names(observations);
        let pressure = extract_cpu_pressure(observations);

        WorkloadObservationSummary::new(cpu, mem, proc_count, names, pressure)
    }

    fn summary_to_context(&self, summary: &WorkloadObservationSummary) -> RecommendationContext {
        let observation_summary = format!(
            "cpu: {:.0}
mem: {:.0}
procs: {}
pressure: {}
names: {}",
            summary.cpu_utilization,
            summary.memory_utilization,
            summary.process_count,
            summary
                .cpu_pressure
                .map(|p| format!("{:.2}", p))
                .unwrap_or_else(|| "none".into()),
            summary.process_names.join(","),
        );

        let system_state = format!(
            "CPU {:.0}% | Memory {:.0}% | {} processes",
            summary.cpu_utilization,
            summary.memory_utilization,
            summary.process_count,
        );

        RecommendationContext::new(
            observation_summary,
            self.agent_id.as_str(),
            system_state,
        )
    }
}

fn extract_class_label(recommendation: &Recommendation) -> String {
    if recommendation.summary.contains("Database") {
        "Database".into()
    } else if recommendation.summary.contains("Interactive") {
        "Interactive".into()
    } else if recommendation.summary.contains("Build") {
        "Build".into()
    } else if recommendation.summary.contains("Batch") {
        "Batch".into()
    } else if recommendation.summary.contains("SystemService") {
        "SystemService".into()
    } else {
        "Unknown".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{
        CgroupObservation, CpuObservation, MemoryObservation, ObservationId, ObservationSource,
        ProcessObservation,
    };

    fn make_observation(payload: ObservationPayload) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Process,
            observed_at: "2026-06-03T00:00:00Z".into(),
            collection_duration_ms: 100,
            payload,
        }
    }

    fn make_process_obs(command: &str, cpu: f64) -> Observation {
        make_observation(ObservationPayload::Process(ProcessObservation {
            pid: 1,
            parent_pid: 0,
            command: command.into(),
            cpu_percent: cpu,
            memory_bytes: 1024,
            state: "running".into(),
        }))
    }

    fn make_cgroup_obs(cpu_usage: u64, processes: u32) -> Observation {
        make_observation(ObservationPayload::Cgroup(CgroupObservation {
            cgroup_path: "/sys/fs/cgroup/test".into(),
            memory_current_bytes: 0,
            memory_swap_current_bytes: 0,
            cpu_usage_usec: cpu_usage,
            cpu_user_usec: 0,
            cpu_system_usec: 0,
            cpu_nr_throttled: 0,
            cpu_throttled_usec: 0,
            processes,
        }))
    }

    fn make_cgroup_obs_only(processes: u32) -> Observation {
        make_observation(ObservationPayload::Cgroup(CgroupObservation {
            cgroup_path: "/sys/fs/cgroup/test".into(),
            memory_current_bytes: 0,
            memory_swap_current_bytes: 0,
            cpu_usage_usec: 0,
            cpu_user_usec: 0,
            cpu_system_usec: 0,
            cpu_nr_throttled: 0,
            cpu_throttled_usec: 0,
            processes,
        }))
    }

    fn make_cpu_obs(pressure: f64) -> Observation {
        make_observation(ObservationPayload::Cpu(CpuObservation {
            pressure_some_avg10: Some(pressure),
            pressure_full_avg10: None,
            nr_running: Some(4),
        }))
    }

    fn make_memory_obs(used: u64, total: u64) -> Observation {
        let mem = MemoryObservation {
            total_bytes: total,
            available_bytes: total - used,
            used_bytes: used,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            pressure_some_avg10: None,
            pressure_full_avg10: None,
        };
        make_observation(ObservationPayload::Memory(mem))
    }

    #[test]
    fn classify_database_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("postgres", 60.0),
            make_process_obs("python", 2.0),
            make_cgroup_obs_only(14),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("Database"));
        assert!((rec.confidence - 0.92).abs() < 0.01);
    }

    #[test]
    fn classify_build_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("rustc", 55.0),
            make_process_obs("cargo", 10.0),
            make_process_obs("cc", 25.0),
            make_cgroup_obs_only(32),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("Build"));
        assert!((rec.confidence - 0.88).abs() < 0.01);
    }

    #[test]
    fn classify_interactive_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("firefox", 15.0),
            make_process_obs("Xorg", 8.0),
            make_process_obs("terminal", 2.0),
            make_cgroup_obs_only(45),
            make_cpu_obs(0.12),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("Interactive"));
        assert!((rec.confidence - 0.85).abs() < 0.01);
    }

    #[test]
    fn classify_unknown_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("someproc", 5.0),
            make_process_obs("another", 3.0),
            make_cgroup_obs_only(6),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("Unknown"));
    }

    #[test]
    fn classify_system_service_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("systemd", 5.0),
            make_process_obs("journald", 3.0),
            make_process_obs("dbus", 2.0),
            make_cgroup_obs_only(8),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("SystemService"));
    }

    #[test]
    fn classify_batch_workload() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_cgroup_obs(900_000, 45),
        ];
        let rec = agent.classify_workload(&obs);
        assert!(rec.summary.contains("Batch"));
    }

    #[test]
    fn deterministic_classification() {
        let mut agent1 = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let mut agent2 = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("postgres", 60.0),
            make_process_obs("python", 2.0),
            make_cgroup_obs_only(14),
        ];
        let r1 = agent1.classify_workload(&obs);
        let r2 = agent2.classify_workload(&obs);
        assert_eq!(r1.summary, r2.summary);
        assert_eq!(r1.reasoning, r2.reasoning);
    }

    #[test]
    fn classification_counts_metrics() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));

        let db_obs = vec![
            make_process_obs("postgres", 60.0),
            make_cgroup_obs_only(14),
        ];
        agent.classify_workload(&db_obs);
        assert_eq!(agent.classification_count(), 1);
        assert_eq!(agent.class_count(&WorkloadClass::Database), 1);

        let build_obs = vec![
            make_process_obs("rustc", 55.0),
            make_process_obs("cc", 25.0),
            make_cgroup_obs_only(32),
        ];
        agent.classify_workload(&build_obs);
        assert_eq!(agent.classification_count(), 2);
        assert_eq!(agent.class_count(&WorkloadClass::Build), 1);
        assert_eq!(agent.class_count(&WorkloadClass::Database), 1);

        agent.classify_workload(&db_obs);
        assert_eq!(agent.classification_count(), 3);
        assert_eq!(agent.class_count(&WorkloadClass::Database), 2);
    }

    #[test]
    fn recommendation_is_purely_advisory() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![make_process_obs("postgres", 45.0), make_cgroup_obs(950_000, 14)];
        let rec = agent.classify_workload(&obs);
        // Verify the Recommendation has no action-related fields by serializing
        // and checking the JSON keys.
        let json = serde_json::to_value(&rec).unwrap();
        let map = json.as_object().unwrap();
        assert!(map.contains_key("id"));
        assert!(map.contains_key("summary"));
        assert!(map.contains_key("reasoning"));
        assert!(map.contains_key("confidence"));
        assert!(map.contains_key("source_agent"));
        assert!(!map.contains_key("requested_action"));
        assert!(!map.contains_key("safety_level"));
        assert!(!map.contains_key("decision_id"));
    }

    #[test]
    fn trace_persistence_and_replay() {
        use agenticos_domain::{EventEnvelope, EventPayload, TraceId};
        use agenticos_bus::{InMemoryTraceStore, Topic, TraceStore};

        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let store = InMemoryTraceStore::new();
        let trace_id = TraceId::new();
        let topic = Topic::new("recommendations.classifier");

        let obs = vec![
            make_process_obs("postgres", 45.0),
            make_cgroup_obs(950_000, 14),
        ];
        let rec = agent.classify_workload(&obs);

        let env = EventEnvelope {
            id: agenticos_domain::MessageId::new(),
            trace_id: trace_id.clone(),
            causation_id: None,
            topic: topic.clone(),
            timestamp: "2026-06-03T00:00:00Z".into(),
            payload: EventPayload::Recommendation(rec.clone()),
        };
        store.append(env).unwrap();

        let replayed = store.replay(trace_id).unwrap();
        assert_eq!(replayed.len(), 1);
        if let EventPayload::Recommendation(r) = &replayed[0].payload {
            assert_eq!(r.summary, rec.summary);
            assert_eq!(r.reasoning, rec.reasoning);
            assert_eq!(r.confidence, rec.confidence);
        } else {
            panic!("expected Recommendation payload");
        }
    }

    #[test]
    fn parse_summary_from_context_full() {
        let ctx = RecommendationContext::new(
            "cpu: 85\nmem: 60\nprocs: 14\npressure: 0.45\nnames: postgres,python",
            "classifier",
            "CPU 85% | Memory 60% | 14 processes",
        );
        let summary = parse_summary_from_context(&ctx);
        assert!((summary.cpu_utilization - 85.0).abs() < 0.01);
        assert!((summary.memory_utilization - 60.0).abs() < 0.01);
        assert_eq!(summary.process_count, 14);
        assert!((summary.cpu_pressure.unwrap() - 0.45).abs() < 0.01);
        assert!(summary.process_names.contains(&"postgres".to_string()));
        assert!(summary.process_names.contains(&"python".to_string()));
    }

    #[test]
    fn parse_summary_from_context_partial() {
        let ctx = RecommendationContext::new("cpu: 50", "classifier", "nominal");
        let summary = parse_summary_from_context(&ctx);
        assert!((summary.cpu_utilization - 50.0).abs() < 0.01);
        assert!((summary.memory_utilization - 50.0).abs() < 0.01);
        assert_eq!(summary.process_count, 0);
        assert!(summary.cpu_pressure.is_none());
        assert!(summary.process_names.is_empty());
    }

    #[test]
    fn build_summary_from_observations() {
        let agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![
            make_process_obs("postgres", 45.0),
            make_process_obs("worker", 10.0),
            make_cgroup_obs(850_000, 12),
            make_cpu_obs(0.35),
            make_memory_obs(6_000_000_000, 16_000_000_000),
        ];
        let summary = agent.build_summary(&obs);
        assert!((summary.cpu_utilization - 45.0).abs() < 0.01);
        assert!((summary.memory_utilization - 37.5).abs() < 0.01);
        assert_eq!(summary.process_count, 12);
        assert!((summary.cpu_pressure.unwrap() - 0.35).abs() < 0.01);
    }

    #[test]
    fn recommendation_has_no_action_fields() {
        let mut agent = WorkloadClassificationAgent::new(AgentId::from("classifier"));
        let obs = vec![make_process_obs("postgres", 45.0), make_cgroup_obs(950_000, 14)];
        let rec = agent.classify_workload(&obs);
        let json = serde_json::to_value(&rec).unwrap();
        let map = json.as_object().unwrap();
        assert!(!map.contains_key("requested_action"), "Recommendation must not have action fields");
        assert!(!map.contains_key("safety_level"), "Recommendation must not have safety fields");
        assert!(!map.contains_key("decision_id"), "Recommendation must not have decision fields");
    }
}
