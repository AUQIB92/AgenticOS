use std::collections::HashMap;

use agenticos_domain::{Decision, Incident};
use agenticos_policy::PolicyInput;

pub mod governor;
pub mod veto;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use governor::DefaultSafetyGovernor;
pub use veto::{VetoDecision, VetoReason};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub struct SafetyInput<'a> {
    pub policy_input: &'a PolicyInput,
    pub decisions: &'a [Decision],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SafetyMetrics {
    pub veto_count: u64,
    pub veto_reason_breakdown: HashMap<String, u64>,
    pub safety_escalations: u64,
    pub policy_violation_attempts: u64,
}

pub struct SafetyOutput {
    pub vetoes: Vec<VetoDecision>,
    pub escalations: Vec<Incident>,
    /// Subset of input decisions that pass all safety checks.
    pub approved: Vec<Decision>,
    pub metrics: SafetyMetrics,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SafetyConfig {
    /// Maximum allowed CPU weight (default: 1000).
    pub max_cpu_weight: u64,
    /// Maximum allowed memory bytes (None = unlimited).
    pub max_memory_bytes: Option<u64>,
    /// Veto non-essential actions when security incidents exist.
    pub veto_on_security_incidents: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            max_cpu_weight: 1000,
            max_memory_bytes: Some(16 * 1024 * 1024 * 1024), // 16 GB
            veto_on_security_incidents: true,
        }
    }
}
