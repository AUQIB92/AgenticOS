use agenticos_domain::{ActionEdge, ActionGraph, ActionNode, PlanId};
use rusqlite::Connection;

/// SQLite-backed persistent storage for ActionGraphs.
pub struct ActionStore {
    conn: Connection,
}

impl ActionStore {
    pub fn new(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("failed to open action store: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS action_graphs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                plan_id TEXT NOT NULL UNIQUE,
                source_intent_id TEXT NOT NULL,
                nodes TEXT NOT NULL,
                edges TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS action_status_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_id INTEGER NOT NULL,
                node_id TEXT NOT NULL,
                status TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (graph_id) REFERENCES action_graphs(id)
            );",
        )
        .map_err(|e| format!("failed to create action store tables: {e}"))?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::new(":memory:")
    }

    /// Insert an ActionGraph into the store.
    pub fn insert(&self, graph: &ActionGraph) -> Result<(), String> {
        let nodes_json =
            serde_json::to_string(&graph.nodes).map_err(|e| format!("serialize nodes: {e}"))?;
        let edges_json =
            serde_json::to_string(&graph.edges).map_err(|e| format!("serialize edges: {e}"))?;
        let created_at = now_utc();

        self.conn
            .execute(
                "INSERT OR REPLACE INTO action_graphs (plan_id, source_intent_id, nodes, edges, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    graph.source_plan_id.as_str(),
                    graph.source_intent_id.as_str(),
                    nodes_json,
                    edges_json,
                    created_at,
                ],
            )
            .map_err(|e| format!("failed to insert action graph: {e}"))?;
        Ok(())
    }

    /// Retrieve an ActionGraph by its plan ID.
    pub fn get(&self, plan_id: &PlanId) -> Result<Option<ActionGraph>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT source_intent_id, nodes, edges FROM action_graphs WHERE plan_id = ?1",
            )
            .map_err(|e| format!("prepare get action graph: {e}"))?;

        let mut rows = stmt
            .query_map(rusqlite::params![plan_id.as_str()], |row| {
                let source_intent_id: String = row.get(0)?;
                let nodes_json: String = row.get(1)?;
                let edges_json: String = row.get(2)?;
                Ok((source_intent_id, nodes_json, edges_json))
            })
            .map_err(|e| format!("query get action graph: {e}"))?;

        match rows.next() {
            Some(Ok((source_intent_id, nodes_json, edges_json))) => {
                let nodes: Vec<ActionNode> = serde_json::from_str(&nodes_json)
                    .map_err(|e| format!("deserialize nodes: {e}"))?;
                let edges: Vec<ActionEdge> = serde_json::from_str(&edges_json)
                    .map_err(|e| format!("deserialize edges: {e}"))?;
                Ok(Some(ActionGraph {
                    nodes,
                    edges,
                    source_plan_id: plan_id.clone(),
                    source_intent_id: agenticos_domain::IntentId::from_string(source_intent_id),
                }))
            }
            Some(Err(e)) => Err(format!("read action graph row: {e}")),
            None => Ok(None),
        }
    }

    /// Return the number of stored action graphs.
    pub fn len(&self) -> Result<usize, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM action_graphs", [], |row| row.get(0))
            .map_err(|e| format!("count action graphs: {e}"))
    }

    pub fn is_empty(&self) -> Result<bool, String> {
        self.len().map(|n| n == 0)
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use agenticos_domain::{
        ActionId, ActionKind, ActionMetadata, IntentId,
    };

    fn sample_graph() -> ActionGraph {
        let plan_id = PlanId::from_string("PlanId-test-1");
        let intent_id = IntentId::from_string("IntentId-test-1");
        let node1 = ActionNode::new(
            ActionId::from_string("ActionId-1"),
            ActionKind::LaunchApplication {
                application: "firefox".into(),
            },
            {
                let mut p = HashMap::new();
                p.insert("application".into(), "firefox".into());
                p
            },
            ActionMetadata {
                source_step: 1,
                source_plan_id: plan_id.clone(),
                source_intent_id: intent_id.clone(),
                tool: Some("firefox".into()),
                capability: Some("launch_application".into()),
            },
        );
        ActionGraph::new(vec![node1], vec![], plan_id, intent_id)
    }

    #[test]
    fn store_insert_and_retrieve() {
        let store = ActionStore::in_memory().unwrap();
        let graph = sample_graph();
        store.insert(&graph).unwrap();

        let retrieved = store
            .get(&graph.source_plan_id)
            .unwrap()
            .expect("should find graph");
        assert_eq!(retrieved.node_count(), 1);
        assert_eq!(retrieved.nodes[0].id.as_str(), "ActionId-1");
    }

    #[test]
    fn store_get_missing_returns_none() {
        let store = ActionStore::in_memory().unwrap();
        let id = PlanId::from_string("nonexistent");
        assert!(store.get(&id).unwrap().is_none());
    }

    #[test]
    fn store_len_tracks_entries() {
        let store = ActionStore::in_memory().unwrap();
        assert_eq!(store.len().unwrap(), 0);
        store.insert(&sample_graph()).unwrap();
        assert_eq!(store.len().unwrap(), 1);
    }

    #[test]
    fn store_empty_check() {
        let store = ActionStore::in_memory().unwrap();
        assert!(store.is_empty().unwrap());
        store.insert(&sample_graph()).unwrap();
        assert!(!store.is_empty().unwrap());
    }
}
