use agenticos_domain::{
    Decision, Incident, MetricCollection, Observation, Proposal,
};

/// Stable snapshot of all inputs available to the Policy Kernel for one tick.
///
/// All fields are collected during tick *N* and frozen into this struct
/// before any policy evaluation begins. No events from tick *N+1* may
/// enter the snapshot (see ADR-0011).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PolicyInput {
    /// Tick number (monotonically increasing, 1-indexed per daemon lifetime).
    pub tick: u64,

    /// All observations collected during this tick.
    pub observations: Vec<Observation>,

    /// All proposals submitted by agents during this tick,
    /// in registration order.
    pub proposals: Vec<Proposal>,

    /// All incidents emitted since the last tick that have not
    /// yet been consumed by policy evaluation.
    pub incidents: Vec<Incident>,

    /// Decisions made in prior ticks (bounded window, configurable size).
    pub prior_decisions: Vec<Decision>,

    /// System metrics collected during this tick.
    pub metrics: MetricCollection,
}
