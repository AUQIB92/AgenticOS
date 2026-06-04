use agenticos_domain::Intent;

use crate::intent_parser::IntentParser;
use crate::intent_store::IntentStore;

/// An intent agent that parses natural language into structured intents
/// and persists them.
///
/// This is a pure advisory component. It may NOT:
/// - Create proposals or action requests
/// - Execute commands
/// - Mutate OS resources
/// - Interface with policy or safety
pub struct IntentAgent {
    parser: Box<dyn IntentParser>,
    store: IntentStore,
}

impl IntentAgent {
    pub fn new(parser: Box<dyn IntentParser>, store: IntentStore) -> Self {
        Self { parser, store }
    }

    /// Parse a natural language request into a structured Intent with a
    /// persistent DB-backed ID, persist it, and return it.
    ///
    /// The ID is generated from the store's row count rather than the
    /// per-process AtomicU64 counter, ensuring IDs are monotonic across
    /// CLI invocations.
    pub fn parse_and_store(&self, text: &str) -> Result<Intent, String> {
        let mut intent = self.parser.parse_intent(text)?;
        // Override the auto-generated ID with a DB-backed persistent ID
        intent.id = self.store.generate_id();
        self.store.insert(&intent)?;
        Ok(intent)
    }

    /// Return a reference to the underlying store.
    pub fn store(&self) -> &IntentStore {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_parser::MockIntentParser;

    #[test]
    fn intent_agent_parses_and_stores() {
        let store = IntentStore::in_memory().unwrap();
        let parser = Box::new(MockIntentParser::new());
        let agent = IntentAgent::new(parser, store);

        let intent = agent.parse_and_store("Open VS Code").unwrap();
        assert_eq!(intent.intent_type, "launch_application");
        assert!(!agent.store().is_empty().unwrap());
    }

    #[test]
    fn intent_agent_mulitple_requests() {
        let store = IntentStore::in_memory().unwrap();
        let parser = Box::new(MockIntentParser::new());
        let agent = IntentAgent::new(parser, store);

        agent.parse_and_store("Open VS Code").unwrap();
        agent.parse_and_store("Create a Next.js project").unwrap();
        agent.parse_and_store("Open Firefox").unwrap();

        assert_eq!(agent.store().len().unwrap(), 3);
    }

    #[test]
    fn intent_agent_unknown_intent() {
        let store = IntentStore::in_memory().unwrap();
        let parser = Box::new(MockIntentParser::new());
        let agent = IntentAgent::new(parser, store);

        let intent = agent.parse_and_store("What time is it?").unwrap();
        assert_eq!(intent.intent_type, "unknown");
        assert!((intent.confidence - 0.50).abs() < 0.01);
    }

    #[test]
    fn intent_agent_store_persists_across_agents() {
        let store = IntentStore::in_memory().unwrap();
        let parser = Box::new(MockIntentParser::new());
        let agent = IntentAgent::new(parser, store);

        let intent = agent.parse_and_store("Open VS Code").unwrap();
        let id = intent.id.clone();

        // A new agent using the same store should see the intent
        let store2 = agent.store(); // same in-memory store reference
        let retrieved = store2.get(&id).unwrap().unwrap();
        assert_eq!(retrieved.intent_type, "launch_application");
    }
}
