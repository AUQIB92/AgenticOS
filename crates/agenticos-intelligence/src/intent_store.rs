use std::collections::HashMap;

use agenticos_domain::{Intent, IntentId};
use rusqlite::Connection;

/// SQLite-backed persistent storage for intents.
pub struct IntentStore {
    conn: Connection,
}

impl IntentStore {
    /// Open (or create) the intent store at the given path.
    ///
    /// Creates the `intents` table if it does not exist.
    pub fn new(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("failed to open intent store: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS intents (
                id TEXT PRIMARY KEY,
                source_text TEXT NOT NULL,
                intent_type TEXT NOT NULL,
                parameters TEXT NOT NULL,
                confidence REAL NOT NULL,
                timestamp TEXT NOT NULL
            );",
        )
        .map_err(|e| format!("failed to create intents table: {e}"))?;
        Ok(Self { conn })
    }

    /// Create an in-memory store for testing.
    pub fn in_memory() -> Result<Self, String> {
        Self::new(":memory:")
    }

    /// Generate a monotonically increasing ID from the database row count.
    /// This ensures IDs are persistent across process restarts.
    pub fn generate_id(&self) -> IntentId {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM intents", [], |row| row.get(0))
            .unwrap_or(0);
        IntentId::from_string(format!("IntentId-{}", count + 1))
    }

    /// Insert an intent into the store.
    pub fn insert(&self, intent: &Intent) -> Result<(), String> {
        let params_json =
            serde_json::to_string(&intent.parameters).map_err(|e| format!("serialize params: {e}"))?;
        self.conn
            .execute(
                "INSERT INTO intents (id, source_text, intent_type, parameters, confidence, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    intent.id.as_str(),
                    intent.source_text,
                    intent.intent_type,
                    params_json,
                    intent.confidence,
                    intent.timestamp,
                ],
            )
            .map_err(|e| format!("failed to insert intent: {e}"))?;
        Ok(())
    }

    /// Retrieve an intent by its ID.
    pub fn get(&self, id: &IntentId) -> Result<Option<Intent>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_text, intent_type, parameters, confidence, timestamp FROM intents WHERE id = ?1")
            .map_err(|e| format!("prepare get: {e}"))?;

        let mut rows = stmt
            .query_map(rusqlite::params![id.as_str()], |row| {
                let id_str: String = row.get(0)?;
                let source_text: String = row.get(1)?;
                let intent_type: String = row.get(2)?;
                let params_json: String = row.get(3)?;
                let confidence: f64 = row.get(4)?;
                let timestamp: String = row.get(5)?;
                Ok((
                    id_str, source_text, intent_type, params_json, confidence, timestamp,
                ))
            })
            .map_err(|e| format!("query get: {e}"))?;

        match rows.next() {
            Some(Ok((id_str, source_text, intent_type, params_json, confidence, timestamp))) => {
                let params: HashMap<String, String> = serde_json::from_str(&params_json)
                    .map_err(|e| format!("deserialize params: {e}"))?;
                Ok(Some(Intent {
                    id: IntentId::from_string(id_str),
                    source_text,
                    intent_type,
                    parameters: params,
                    confidence,
                    timestamp,
                }))
            }
            Some(Err(e)) => Err(format!("read intent row: {e}")),
            None => Ok(None),
        }
    }

    /// List all intents, ordered by timestamp descending.
    pub fn list(&self) -> Result<Vec<Intent>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_text, intent_type, parameters, confidence, timestamp FROM intents ORDER BY rowid DESC")
            .map_err(|e| format!("prepare list: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let source_text: String = row.get(1)?;
                let intent_type: String = row.get(2)?;
                let params_json: String = row.get(3)?;
                let confidence: f64 = row.get(4)?;
                let timestamp: String = row.get(5)?;
                Ok((
                    id_str, source_text, intent_type, params_json, confidence, timestamp,
                ))
            })
            .map_err(|e| format!("query list: {e}"))?;

        let mut intents = Vec::new();
        for row in rows {
            let (id_str, source_text, intent_type, params_json, confidence, timestamp) =
                row.map_err(|e| format!("read intent row: {e}"))?;
            let params: HashMap<String, String> = serde_json::from_str(&params_json)
                .map_err(|e| format!("deserialize params: {e}"))?;
            intents.push(Intent {
                id: IntentId::from_string(id_str),
                source_text,
                intent_type,
                parameters: params,
                confidence,
                timestamp,
            });
        }
        Ok(intents)
    }

    /// Return the number of stored intents.
    pub fn len(&self) -> Result<usize, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM intents", [], |row| row.get(0))
            .map_err(|e| format!("count intents: {e}"))
    }

    /// Return true if the store has no intents.
    pub fn is_empty(&self) -> Result<bool, String> {
        self.len().map(|n| n == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_insert_and_retrieve() {
        let store = IntentStore::in_memory().unwrap();
        let mut params = HashMap::new();
        params.insert("application".into(), "vscode".into());
        let intent = Intent::new("Open VS Code", "launch_application", params, 0.9);
        store.insert(&intent).unwrap();

        let retrieved = store.get(&intent.id).unwrap().unwrap();
        assert_eq!(retrieved.intent_type, "launch_application");
        assert_eq!(retrieved.parameters.get("application").unwrap(), "vscode");
    }

    #[test]
    fn store_list_returns_all() {
        let store = IntentStore::in_memory().unwrap();
        let a = Intent::new("Open VS Code", "launch_application", HashMap::new(), 0.9);
        let b = Intent::new("Create project", "create_project", HashMap::new(), 0.85);
        store.insert(&a).unwrap();
        store.insert(&b).unwrap();

        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn store_get_missing_returns_none() {
        let store = IntentStore::in_memory().unwrap();
        let id = IntentId::new();
        assert!(store.get(&id).unwrap().is_none());
    }

    #[test]
    fn store_len_tracks_entries() {
        let store = IntentStore::in_memory().unwrap();
        assert_eq!(store.len().unwrap(), 0);
        store
            .insert(&Intent::new("test", "unknown", HashMap::new(), 0.5))
            .unwrap();
        assert_eq!(store.len().unwrap(), 1);
    }

    #[test]
    fn store_empty_check() {
        let store = IntentStore::in_memory().unwrap();
        assert!(store.is_empty().unwrap());
        store
            .insert(&Intent::new("test", "unknown", HashMap::new(), 0.5))
            .unwrap();
        assert!(!store.is_empty().unwrap());
    }
}
