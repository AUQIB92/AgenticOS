use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkloadClass {
    Database,
    Interactive,
    Build,
    Batch,
    SystemService,
    Unknown,
}

impl WorkloadClass {
    pub fn label(&self) -> &'static str {
        match self {
            WorkloadClass::Database => "Database",
            WorkloadClass::Interactive => "Interactive",
            WorkloadClass::Build => "Build",
            WorkloadClass::Batch => "Batch",
            WorkloadClass::SystemService => "SystemService",
            WorkloadClass::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkloadObservationSummary {
    pub cpu_utilization: f64,
    pub memory_utilization: f64,
    pub process_count: u32,
    pub process_names: Vec<String>,
    pub cpu_pressure: Option<f64>,
}

impl WorkloadObservationSummary {
    pub fn new(
        cpu_utilization: f64,
        memory_utilization: f64,
        process_count: u32,
        process_names: Vec<String>,
        cpu_pressure: Option<f64>,
    ) -> Self {
        Self {
            cpu_utilization,
            memory_utilization,
            process_count,
            process_names,
            cpu_pressure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_class_label_database() {
        assert_eq!(WorkloadClass::Database.label(), "Database");
    }

    #[test]
    fn workload_class_label_interactive() {
        assert_eq!(WorkloadClass::Interactive.label(), "Interactive");
    }

    #[test]
    fn workload_class_label_build() {
        assert_eq!(WorkloadClass::Build.label(), "Build");
    }

    #[test]
    fn workload_class_label_batch() {
        assert_eq!(WorkloadClass::Batch.label(), "Batch");
    }

    #[test]
    fn workload_class_label_system_service() {
        assert_eq!(WorkloadClass::SystemService.label(), "SystemService");
    }

    #[test]
    fn workload_class_label_unknown() {
        assert_eq!(WorkloadClass::Unknown.label(), "Unknown");
    }

    #[test]
    fn workload_class_serde_round_trip() {
        let classes = [
            WorkloadClass::Database,
            WorkloadClass::Interactive,
            WorkloadClass::Build,
            WorkloadClass::Batch,
            WorkloadClass::SystemService,
            WorkloadClass::Unknown,
        ];
        for class in &classes {
            let json = serde_json::to_string(class).unwrap();
            let back: WorkloadClass = serde_json::from_str(&json).unwrap();
            assert_eq!(*class, back);
        }
    }

    #[test]
    fn workload_observation_summary_constructs() {
        let s = WorkloadObservationSummary::new(
            85.0,
            60.0,
            42,
            vec!["postgres".into(), "python".into()],
            Some(0.3),
        );
        assert!((s.cpu_utilization - 85.0).abs() < 0.001);
        assert!((s.memory_utilization - 60.0).abs() < 0.001);
        assert_eq!(s.process_count, 42);
        assert_eq!(s.process_names.len(), 2);
        assert!((s.cpu_pressure.unwrap() - 0.3).abs() < 0.001);
    }

    #[test]
    fn workload_observation_summary_serde_round_trip() {
        let s = WorkloadObservationSummary::new(
            85.0, 60.0, 42,
            vec!["postgres".into()],
            Some(0.3),
        );
        let json = serde_json::to_string(&s).unwrap();
        let back: WorkloadObservationSummary = serde_json::from_str(&json).unwrap();
        assert!((back.cpu_utilization - 85.0).abs() < 0.001);
        assert_eq!(back.process_names, vec!["postgres"]);
    }

    #[test]
    fn workload_observation_summary_empty_process_names() {
        let s = WorkloadObservationSummary::new(10.0, 30.0, 0, vec![], None);
        assert!(s.process_names.is_empty());
        assert!(s.cpu_pressure.is_none());
    }
}
