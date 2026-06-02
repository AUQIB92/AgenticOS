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
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}
