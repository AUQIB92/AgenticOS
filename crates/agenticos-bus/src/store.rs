use agenticos_application::AppError;
use agenticos_domain::{EventEnvelope, EventPayload, MessageId, TraceId, Topic};
use std::sync::{Arc, Mutex};

pub trait TraceStore: Send + Sync {
    fn append(&self, event: EventEnvelope) -> Result<(), AppError>;
    fn replay(&self, trace_id: TraceId) -> Result<Vec<EventEnvelope>, AppError>;
}

#[derive(Clone, Default)]
pub struct InMemoryTraceStore {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl InMemoryTraceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> Result<usize, AppError> {
        Ok(self.events()?.len())
    }

    fn events(&self) -> Result<std::sync::MutexGuard<'_, Vec<EventEnvelope>>, AppError> {
        self.events
            .lock()
            .map_err(|_| AppError::Message("trace store lock poisoned".to_owned()))
    }
}

impl TraceStore for InMemoryTraceStore {
    fn append(&self, event: EventEnvelope) -> Result<(), AppError> {
        self.events()?.push(event);
        Ok(())
    }

    fn replay(&self, trace_id: TraceId) -> Result<Vec<EventEnvelope>, AppError> {
        Ok(self
            .events()?
            .iter()
            .filter(|event| event.trace_id == trace_id)
            .cloned()
            .collect())
    }
}

#[derive(Clone)]
pub struct SqliteTraceStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SqliteTraceStore {
    pub fn new(db_path: &str) -> Result<Self, AppError> {
        let conn = rusqlite::Connection::open(db_path)
            .map_err(|e| AppError::Message(format!("failed to open sqlite db: {e}")))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT NOT NULL,
                trace_id TEXT NOT NULL,
                causation_id TEXT,
                topic TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                payload_json TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Message(format!("failed to create traces table: {e}")))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_traces_trace_id ON traces(trace_id)",
            [],
        )
        .map_err(|e| AppError::Message(format!("failed to create index: {e}")))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

impl TraceStore for SqliteTraceStore {
    fn append(&self, event: EventEnvelope) -> Result<(), AppError> {
        let payload_json = serde_json::to_string(&event.payload)
            .map_err(|e| AppError::Message(format!("failed to serialize event payload: {e}")))?;

        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Message("sqlite store lock poisoned".to_owned()))?;

        conn.execute(
            "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                event.id.as_str(),
                event.trace_id.as_str(),
                event.causation_id.as_ref().map(|c| c.as_str()),
                event.topic.as_str(),
                event.timestamp,
                payload_json,
            ],
        )
        .map_err(|e| AppError::Message(format!("failed to insert trace: {e}")))?;

        Ok(())
    }

    fn replay(&self, trace_id: TraceId) -> Result<Vec<EventEnvelope>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Message("sqlite store lock poisoned".to_owned()))?;

        let mut stmt = conn
            .prepare("SELECT message_id, causation_id, topic, timestamp, payload_json FROM traces WHERE trace_id = ?1 ORDER BY id")
            .map_err(|e| AppError::Message(format!("failed to prepare replay query: {e}")))?;

        let rows = stmt
            .query_map(rusqlite::params![trace_id.as_str()], |row| {
                let message_id: String = row.get(0)?;
                let causation_id: Option<String> = row.get(1)?;
                let topic: String = row.get(2)?;
                let timestamp: String = row.get(3)?;
                let payload_json: String = row.get(4)?;
                Ok((
                    message_id, causation_id, topic, timestamp, payload_json,
                ))
            })
            .map_err(|e| AppError::Message(format!("failed to query traces: {e}")))?;

        let mut events = Vec::new();
        for row in rows {
            let (message_id, causation_id, topic, timestamp, payload_json) =
                row.map_err(|e| AppError::Message(format!("failed to read trace row: {e}")))?;

            let payload: EventPayload = serde_json::from_str(&payload_json).map_err(|e| {
                AppError::Message(format!("failed to deserialize event payload: {e}"))
            })?;

            let envelope = EventEnvelope {
                id: MessageId::from_string(message_id),
                trace_id: trace_id.clone(),
                causation_id: causation_id.map(MessageId::from_string),
                topic: Topic::new(topic),
                timestamp,
                payload,
            };
            events.push(envelope);
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{TraceEvent};

    #[test]
    fn replays_events_for_one_trace_in_append_order() {
        let store = InMemoryTraceStore::new();
        let trace_a = TraceId::from("trace-a");
        let trace_b = TraceId::from("trace-b");

        store
            .append(EventEnvelope::new(
                Topic::new("traces.test"),
                trace_a.clone(),
                EventPayload::Trace(TraceEvent {
                    message: "first".to_owned(),
                }),
            ))
            .unwrap();
        store
            .append(EventEnvelope::new(
                Topic::new("traces.test"),
                trace_b,
                EventPayload::Trace(TraceEvent {
                    message: "other".to_owned(),
                }),
            ))
            .unwrap();
        store
            .append(EventEnvelope::new(
                Topic::new("traces.test"),
                trace_a.clone(),
                EventPayload::Trace(TraceEvent {
                    message: "second".to_owned(),
                }),
            ))
            .unwrap();

        let replayed = store.replay(trace_a).unwrap();

        assert_eq!(replayed.len(), 2);
        assert_trace_message(&replayed[0], "first");
        assert_trace_message(&replayed[1], "second");
    }

    #[test]
    fn sqlite_store_round_trip() {
        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("agenticos-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteTraceStore::new(db_path.to_str().unwrap()).unwrap();
        let trace_id = TraceId::from("sqlite-test-1");

        // Write two events for trace_a and one for trace_b
        store
            .append(EventEnvelope::new(
                Topic::new("observations.memory"),
                trace_id.clone(),
                EventPayload::Trace(TraceEvent {
                    message: "mem_obs_1".to_owned(),
                }),
            ))
            .unwrap();
        store
            .append(EventEnvelope::new(
                Topic::new("proposals.memory"),
                trace_id.clone(),
                EventPayload::Trace(TraceEvent {
                    message: "mem_proposal_1".to_owned(),
                }),
            ))
            .unwrap();

        // Replay and verify
        let replayed = store.replay(trace_id).unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].topic, Topic::new("observations.memory"));
        assert_eq!(replayed[1].topic, Topic::new("proposals.memory"));

        // Cleanup
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn sqlite_recommendation_round_trip() {
        use agenticos_domain::{AgentId, Recommendation};

        let dir = std::env::temp_dir();
        let db_path = dir.join(format!("agenticos-rec-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);

        let store = SqliteTraceStore::new(db_path.to_str().unwrap()).unwrap();
        let trace_id = TraceId::from("recommendation-test");

        let rec = Recommendation::new(
            AgentId::from("cpu-agent"),
            0.9,
            "High CPU detected",
            "CPU at 95% for 30s",
        );

        store
            .append(EventEnvelope::new(
                Topic::new("recommendations.cpu"),
                trace_id.clone(),
                EventPayload::Recommendation(rec.clone()),
            ))
            .unwrap();

        let replayed = store.replay(trace_id).unwrap();
        assert_eq!(replayed.len(), 1);

        match &replayed[0].payload {
            EventPayload::Recommendation(recovered) => {
                assert_eq!(recovered.id, rec.id);
                assert_eq!(recovered.summary, rec.summary);
                assert_eq!(recovered.reasoning, rec.reasoning);
                assert_eq!(recovered.confidence, rec.confidence);
            }
            _ => panic!("expected Recommendation payload"),
        }

        let _ = std::fs::remove_file(&db_path);
    }

    fn assert_trace_message(event: &EventEnvelope, expected: &str) {
        match &event.payload {
            EventPayload::Trace(trace) => assert_eq!(trace.message, expected),
            _ => panic!("expected trace payload"),
        }
    }
}
