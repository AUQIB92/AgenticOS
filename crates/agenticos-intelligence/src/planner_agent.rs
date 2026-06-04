use std::collections::HashMap;

use agenticos_domain::{Intent, PlanStep, TaskPlan};

/// A planner that converts an Intent into a deterministic TaskPlan.
///
/// This is a pure advisory component. It may NOT:
/// - Execute commands
/// - Create proposals or action requests
/// - Mutate OS resources
/// - Interface with policy or safety
pub trait PlannerAgent: Send + Sync {
    fn create_plan(&self, intent: &Intent) -> Result<TaskPlan, String>;
}

/// A deterministic rule-based planner for testing and basic use.
///
/// Supported intent types:
/// - `launch_application` → 1–2 steps (launch + optional open_url)
/// - `open_url` → 1 step (open_url)
/// - `create_project` → 2 steps (create_directory, initialize_project)
pub struct MockPlannerAgent;

impl MockPlannerAgent {
    pub fn new() -> Self {
        Self
    }
}

impl PlannerAgent for MockPlannerAgent {
    fn create_plan(&self, intent: &Intent) -> Result<TaskPlan, String> {
        match intent.intent_type.as_str() {
            "launch_application" => plan_launch_application(intent),
            "open_url" => plan_open_url(intent),
            "create_project" => plan_create_project(intent),
            "run_command" => plan_run_command(intent),
            _ => Err(format!("unsupported intent type: {}", intent.intent_type)),
        }
    }
}

fn plan_launch_application(intent: &Intent) -> Result<TaskPlan, String> {
    let mut steps = Vec::new();

    if let Some(app) = intent.parameters.get("application") {
        let mut p = HashMap::new();
        p.insert("application".into(), app.clone());
        steps.push(PlanStep::new(1, "launch_application", p));
    }

    if let Some(url) = intent.parameters.get("url") {
        let mut p = HashMap::new();
        p.insert("url".into(), url.clone());
        let order = if steps.is_empty() { 1 } else { 2 };
        steps.push(PlanStep::new(order, "open_url", p));
    }

    if steps.is_empty() {
        return Err(
            "launch_application intent has no application or url parameter".into(),
        );
    }

    Ok(TaskPlan::new(intent.id.clone(), steps, "pending"))
}

fn plan_open_url(intent: &Intent) -> Result<TaskPlan, String> {
    let url = intent
        .parameters
        .get("url")
        .ok_or_else(|| "open_url intent has no url parameter".to_string())?;

    let mut p = HashMap::new();
    p.insert("url".into(), url.clone());
    let step = PlanStep::new(1, "open_url", p);

    Ok(TaskPlan::new(intent.id.clone(), vec![step], "pending"))
}

fn plan_create_project(intent: &Intent) -> Result<TaskPlan, String> {
    let project_name = intent
        .parameters
        .get("project_name")
        .cloned()
        .unwrap_or_else(|| "project".to_string());

    let framework = intent
        .parameters
        .get("framework")
        .cloned()
        .unwrap_or_else(|| "generic".to_string());

    let mut steps = Vec::new();

    // Step 1: Create project directory
    {
        let mut p = HashMap::new();
        p.insert("path".into(), project_name.clone());
        steps.push(PlanStep::new(1, "create_directory", p));
    }

    // Step 2: Initialize project with framework
    {
        let mut p = HashMap::new();
        p.insert("framework".into(), framework);
        p.insert("project_name".into(), project_name);
        steps.push(PlanStep::new(2, "initialize_project", p));
    }

    Ok(TaskPlan::new(intent.id.clone(), steps, "pending"))
}

fn plan_run_command(intent: &Intent) -> Result<TaskPlan, String> {
    let cmd = intent
        .parameters
        .get("command")
        .ok_or_else(|| "run_command intent has no command parameter".to_string())?;

    let (base, args) = match cmd.find(' ') {
        Some(idx) => (cmd[..idx].to_string(), cmd[idx + 1..].trim().to_string()),
        None => (cmd.clone(), String::new()),
    };

    let mut p = std::collections::HashMap::new();
    p.insert("command".into(), base);
    p.insert("args".into(), args);
    let step = PlanStep::new(1, "run_command", p);

    Ok(TaskPlan::new(intent.id.clone(), vec![step], "pending"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::Intent;
    use crate::intent_parser::{IntentParser, MockIntentParser};

    fn parse(text: &str) -> Intent {
        let parser = MockIntentParser::new();
        parser.parse_intent(text).unwrap()
    }

    #[test]
    fn mock_planner_launch_application() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Open VS Code");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action, "launch_application");
        assert_eq!(
            plan.steps[0].parameters.get("application").unwrap(),
            "vscode"
        );
        assert_eq!(plan.status, "pending");
    }

    #[test]
    fn mock_planner_open_firefox_and_github() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Open Firefox and go to github.com");
        assert_eq!(intent.intent_type, "launch_application");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].action, "launch_application");
        assert_eq!(
            plan.steps[0].parameters.get("application").unwrap(),
            "firefox"
        );
        assert_eq!(plan.steps[1].action, "open_url");
        assert_eq!(
            plan.steps[1].parameters.get("url").unwrap(),
            "https://github.com"
        );
    }

    #[test]
    fn mock_planner_create_project() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Create Next.js project called examgenius");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].action, "create_directory");
        assert_eq!(
            plan.steps[0].parameters.get("path").unwrap(),
            "examgenius"
        );
        assert_eq!(plan.steps[1].action, "initialize_project");
        assert_eq!(
            plan.steps[1].parameters.get("framework").unwrap(),
            "nextjs"
        );
        assert_eq!(
            plan.steps[1].parameters.get("project_name").unwrap(),
            "examgenius"
        );
    }

    #[test]
    fn mock_planner_unsupported_intent_returns_error() {
        let planner = MockPlannerAgent::new();
        let intent = Intent::new("unknown", "unknown", HashMap::new(), 0.5);
        let result = planner.create_plan(&intent);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("unsupported intent type"));
    }

    #[test]
    fn mock_planner_is_deterministic() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Create React app called myapp");
        let plan1 = planner.create_plan(&intent).unwrap();
        let plan2 = planner.create_plan(&intent).unwrap();
        assert_eq!(plan1.steps.len(), plan2.steps.len());
        for (s1, s2) in plan1.steps.iter().zip(plan2.steps.iter()) {
            assert_eq!(s1.action, s2.action);
            assert_eq!(s1.parameters, s2.parameters);
        }
    }

    #[test]
    fn mock_planner_run_command() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Run cargo build");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action, "run_command");
        assert_eq!(
            plan.steps[0].parameters.get("command").unwrap(),
            "cargo"
        );
        assert_eq!(plan.steps[0].parameters.get("args").unwrap(), "build");
    }

    #[test]
    fn mock_planner_run_command_no_args() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Run ls");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.steps[0].action, "run_command");
        assert_eq!(
            plan.steps[0].parameters.get("command").unwrap(),
            "ls"
        );
        assert_eq!(
            plan.steps[0].parameters.get("args").unwrap(),
            ""
        );
    }

    #[test]
    fn mock_planner_source_intent_id_matches() {
        let planner = MockPlannerAgent::new();
        let intent = parse("Open VS Code");
        let plan = planner.create_plan(&intent).unwrap();
        assert_eq!(plan.source_intent_id, intent.id);
    }
}
