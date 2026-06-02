use agenticos_application::{AppError, EventStream};
use agenticos_domain::{EventEnvelope, Topic};
use std::sync::{Arc, Mutex};

pub trait AgenticEventBus: Send + Sync {
    fn publish(&self, event: EventEnvelope) -> Result<(), AppError>;
    fn subscribe(&self, topic: Topic) -> Result<EventStream, AppError>;
}

#[derive(Clone, Default)]
pub struct InMemoryEventBus {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> Result<usize, AppError> {
        Ok(self.events()?.len())
    }

    fn events(&self) -> Result<std::sync::MutexGuard<'_, Vec<EventEnvelope>>, AppError> {
        self.events
            .lock()
            .map_err(|_| AppError::Message("event bus lock poisoned".to_owned()))
    }
}

impl AgenticEventBus for InMemoryEventBus {
    fn publish(&self, event: EventEnvelope) -> Result<(), AppError> {
        self.events()?.push(event);
        Ok(())
    }

    fn subscribe(&self, topic: Topic) -> Result<EventStream, AppError> {
        let snapshot = self
            .events()?
            .iter()
            .filter(|event| event.topic.matches(&topic))
            .cloned()
            .map(Ok)
            .collect::<Vec<_>>();

        Ok(Box::new(snapshot.into_iter()))
    }
}

impl agenticos_application::EventBus for InMemoryEventBus {
    fn publish(&self, event: EventEnvelope) -> Result<(), AppError> {
        <Self as AgenticEventBus>::publish(self, event)
    }

    fn subscribe(&self, topic: Topic) -> Result<EventStream, AppError> {
        <Self as AgenticEventBus>::subscribe(self, topic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{EventPayload, TraceEvent, TraceId};

    #[test]
    fn publishes_and_reads_exact_topic_snapshot() {
        let bus = InMemoryEventBus::new();
        let trace_id = TraceId::from("trace-1");

        bus.publish(EventEnvelope::new(
            Topic::new("observations.memory"),
            trace_id.clone(),
            EventPayload::Trace(TraceEvent {
                message: "memory".to_owned(),
            }),
        ))
        .unwrap();
        bus.publish(EventEnvelope::new(
            Topic::new("observations.process"),
            trace_id,
            EventPayload::Trace(TraceEvent {
                message: "process".to_owned(),
            }),
        ))
        .unwrap();

        let events = bus
            .subscribe(Topic::new("observations.memory"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, Topic::new("observations.memory"));
    }

    #[test]
    fn wildcard_topic_matches_children() {
        let bus = InMemoryEventBus::new();
        let trace_id = TraceId::from("trace-1");

        bus.publish(EventEnvelope::new(
            Topic::new("observations.memory"),
            trace_id,
            EventPayload::Trace(TraceEvent {
                message: "memory".to_owned(),
            }),
        ))
        .unwrap();

        let events = bus
            .subscribe(Topic::new("observations.*"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(events.len(), 1);
    }
}
