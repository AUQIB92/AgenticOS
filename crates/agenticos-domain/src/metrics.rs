#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MetricValue {
    Gauge(f64),
    Counter(u64),
    Histogram(Vec<f64>),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MetricLabel {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MetricSample {
    pub name: String,
    pub value: MetricValue,
    pub labels: Vec<MetricLabel>,
    pub timestamp: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MetricCollection {
    pub samples: Vec<MetricSample>,
    pub source: String,
}

impl MetricCollection {
    /// Append an `incident_count` gauge sample.
    pub fn with_incident_count(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "incident_count".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `safety_veto_count` gauge.
    pub fn with_safety_veto_count(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "safety_veto_count".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `safety_escalations` gauge.
    pub fn with_safety_escalations(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "safety_escalations".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `safety_freeze_ticks` gauge.
    pub fn with_safety_freeze_ticks(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "safety_freeze_ticks".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `safety_selective_vetoes` gauge.
    pub fn with_safety_selective_vetoes(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "safety_selective_vetoes".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `safety_global_vetoes` gauge.
    pub fn with_safety_global_vetoes(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "safety_global_vetoes".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_successful_mutations` gauge.
    pub fn with_executor_successful_mutations(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_successful_mutations".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_failed_mutations` gauge.
    pub fn with_executor_failed_mutations(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_failed_mutations".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_rollback_count` gauge.
    pub fn with_executor_rollback_count(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_rollback_count".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_cpu_weight_changes` gauge.
    pub fn with_executor_cpu_weight_changes(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_cpu_weight_changes".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_cpu_max_changes` gauge.
    pub fn with_executor_cpu_max_changes(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_cpu_max_changes".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `executor_memory_max_changes` gauge.
    pub fn with_executor_memory_max_changes(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "executor_memory_max_changes".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "daemon".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `classification_count` gauge.
    pub fn with_classification_count(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "classification_count".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "intelligence".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append a classification class counter gauge.
    pub fn with_classification_class(mut self, class: &str, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: format!("classification_{class}"),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "intelligence".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `recommendations_consumed` gauge.
    pub fn with_recommendations_consumed(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "recommendations_consumed".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "bridge".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `recommendations_ignored` gauge.
    pub fn with_recommendations_ignored(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "recommendations_ignored".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "bridge".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `recommendations_converted` gauge.
    pub fn with_recommendations_converted(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "recommendations_converted".into(),
            value: MetricValue::Gauge(count),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "bridge".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }

    /// Append `classifications_skipped_total` counter.
    pub fn with_classifications_skipped(mut self, count: f64) -> Self {
        self.samples.push(MetricSample {
            name: "classifications_skipped_total".into(),
            value: MetricValue::Counter(count as u64),
            labels: vec![MetricLabel {
                name: "service".into(),
                value: "intelligence".into(),
            }],
            timestamp: now_utc(),
        });
        self
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}
