use agenticos_domain::{PlanId, PlanStep, TaskPlan};
use rusqlite::Connection;

/// SQLite-backed persistent storage for TaskPlans.
pub struct PlanStore {
    conn: Connection,
}

impl PlanStore {
    /// Open (or create) the plan store at the given path.
    ///
    /// Creates the `plans` table if it does not exist.
    pub fn new(path: &str) -> Result<Self, String> {
        let conn =
            Connection::open(path).map_err(|e| format!("failed to open plan store: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS plans (
                id TEXT PRIMARY KEY,
                source_intent_id TEXT NOT NULL,
                steps TEXT NOT NULL,
                status TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );",
        )
        .map_err(|e| format!("failed to create plans table: {e}"))?;
        Ok(Self { conn })
    }

    /// Create an in-memory store for testing.
    pub fn in_memory() -> Result<Self, String> {
        Self::new(":memory:")
    }

    /// Generate a monotonically increasing ID from the database row count.
    /// Ensures PlanId is persistent across process restarts.
    pub fn generate_id(&self) -> PlanId {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM plans", [], |row| row.get(0))
            .unwrap_or(0);
        PlanId::from_string(format!("PlanId-{}", count + 1))
    }

    /// Insert a plan into the store.
    pub fn insert(&self, plan: &TaskPlan) -> Result<(), String> {
        let steps_json =
            serde_json::to_string(&plan.steps).map_err(|e| format!("serialize steps: {e}"))?;
        self.conn
            .execute(
                "INSERT INTO plans (id, source_intent_id, steps, status, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    plan.id.as_str(),
                    plan.source_intent_id.as_str(),
                    steps_json,
                    plan.status,
                    plan.timestamp,
                ],
            )
            .map_err(|e| format!("failed to insert plan: {e}"))?;
        Ok(())
    }

    /// Retrieve a plan by its ID.
    pub fn get(&self, id: &PlanId) -> Result<Option<TaskPlan>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_intent_id, steps, status, timestamp FROM plans WHERE id = ?1",
            )
            .map_err(|e| format!("prepare get: {e}"))?;

        let mut rows = stmt
            .query_map(rusqlite::params![id.as_str()], |row| {
                let id_str: String = row.get(0)?;
                let source_intent_id: String = row.get(1)?;
                let steps_json: String = row.get(2)?;
                let status: String = row.get(3)?;
                let timestamp: String = row.get(4)?;
                Ok((id_str, source_intent_id, steps_json, status, timestamp))
            })
            .map_err(|e| format!("query get: {e}"))?;

        match rows.next() {
            Some(Ok((id_str, source_intent_id, steps_json, status, timestamp))) => {
                let steps: Vec<PlanStep> = serde_json::from_str(&steps_json)
                    .map_err(|e| format!("deserialize steps: {e}"))?;
                Ok(Some(TaskPlan {
                    id: PlanId::from_string(id_str),
                    source_intent_id: agenticos_domain::IntentId::from_string(source_intent_id),
                    steps,
                    status,
                    timestamp,
                }))
            }
            Some(Err(e)) => Err(format!("read plan row: {e}")),
            None => Ok(None),
        }
    }

    /// List all plans, ordered by newest first.
    pub fn list(&self) -> Result<Vec<TaskPlan>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_intent_id, steps, status, timestamp FROM plans ORDER BY rowid DESC",
            )
            .map_err(|e| format!("prepare list: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let source_intent_id: String = row.get(1)?;
                let steps_json: String = row.get(2)?;
                let status: String = row.get(3)?;
                let timestamp: String = row.get(4)?;
                Ok((id_str, source_intent_id, steps_json, status, timestamp))
            })
            .map_err(|e| format!("query list: {e}"))?;

        let mut plans = Vec::new();
        for row in rows {
            let (id_str, source_intent_id, steps_json, status, timestamp) =
                row.map_err(|e| format!("read plan row: {e}"))?;
            let steps: Vec<PlanStep> = serde_json::from_str(&steps_json)
                .map_err(|e| format!("deserialize steps: {e}"))?;
            plans.push(TaskPlan {
                id: PlanId::from_string(id_str),
                source_intent_id: agenticos_domain::IntentId::from_string(source_intent_id),
                steps,
                status,
                timestamp,
            });
        }
        Ok(plans)
    }

    /// Return the number of stored plans.
    pub fn len(&self) -> Result<usize, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM plans", [], |row| row.get(0))
            .map_err(|e| format!("count plans: {e}"))
    }

    /// Return true if the store has no plans.
    pub fn is_empty(&self) -> Result<bool, String> {
        self.len().map(|n| n == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use agenticos_domain::Intent;

    fn create_test_plan() -> TaskPlan {
        let intent = Intent::new("Open VS Code", "launch_application", HashMap::new(), 0.9);
        let step = PlanStep::new(1, "launch_application", {
            let mut p = HashMap::new();
            p.insert("application".into(), "vscode".into());
            p
        });
        TaskPlan::new(intent.id, vec![step], "pending")
    }

    #[test]
    fn store_insert_and_retrieve() {
        let store = PlanStore::in_memory().unwrap();
        let plan = create_test_plan();
        store.insert(&plan).unwrap();

        let retrieved = store.get(&plan.id).unwrap().unwrap();
        assert_eq!(retrieved.status, "pending");
        assert_eq!(retrieved.steps.len(), 1);
        assert_eq!(retrieved.steps[0].action, "launch_application");
    }

    #[test]
    fn store_list_returns_all() {
        let store = PlanStore::in_memory().unwrap();
        let a = create_test_plan();
        let b = {
            let intent = Intent::new("Create project", "create_project", HashMap::new(), 0.85);
            let step = PlanStep::new(1, "create_directory", HashMap::new());
            TaskPlan::new(intent.id, vec![step], "pending")
        };
        store.insert(&a).unwrap();
        store.insert(&b).unwrap();

        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn store_get_missing_returns_none() {
        let store = PlanStore::in_memory().unwrap();
        let id = PlanId::new();
        assert!(store.get(&id).unwrap().is_none());
    }

    #[test]
    fn store_len_tracks_entries() {
        let store = PlanStore::in_memory().unwrap();
        assert_eq!(store.len().unwrap(), 0);
        let plan = create_test_plan();
        store.insert(&plan).unwrap();
        assert_eq!(store.len().unwrap(), 1);
    }

    #[test]
    fn store_empty_check() {
        let store = PlanStore::in_memory().unwrap();
        assert!(store.is_empty().unwrap());
        let plan = create_test_plan();
        store.insert(&plan).unwrap();
        assert!(!store.is_empty().unwrap());
    }
}
