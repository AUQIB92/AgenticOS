use std::{error::Error, fmt};

use agenticos_domain::{
    ActionResult, ApprovedAction, Decision, EventEnvelope, MetricCollection, Observation, Proposal,
    Topic,
};

pub type EventStream = Box<dyn Iterator<Item = Result<EventEnvelope, AppError>> + Send>;

#[derive(Debug)]
pub enum AppError {
    Message(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => write!(f, "application error: {message}"),
        }
    }
}

impl Error for AppError {}

pub trait EventBus: Send + Sync {
    fn publish(&self, event: EventEnvelope) -> Result<(), AppError>;
    fn subscribe(&self, topic: Topic) -> Result<EventStream, AppError>;
}

pub trait PolicyKernelPort: Send + Sync {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, AppError>;
}

pub trait ActionExecutorPort: Send + Sync {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError>;
}

pub trait ObserverPort: Send + Sync {
    fn observe(&self) -> Result<Vec<Observation>, AppError>;
}

pub trait MetricExporterPort: Send + Sync {
    fn export(&self, collection: MetricCollection) -> Result<(), AppError>;
}
