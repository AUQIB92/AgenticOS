use clap::{Parser, Subcommand};
use rusqlite::Connection;
use serde::Serialize;

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "agenticos", version, about = "AgenticOS research CLI")]
struct Cli {
    /// Path to SQLite trace store database
    #[arg(short, long, default_value = "data/agenticos-dev.db")]
    db: String,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show daemon status, tick count, active agents, recent metrics
    Status,
    /// Show registered agents with proposal/incident counts
    Agents,
    /// Show incidents
    Incidents,
    /// Show recent traces
    Traces,
    /// Show decision outcomes and vetoes
    Decisions,
    /// Show aggregated metrics
    Metrics,
    /// Reconstruct a full trace
    Replay { trace_id: String },
    /// Run governance integrity checks
    Health,
    /// Show live cgroup v2 state (cpu.weight, cpu.max, memory.max)
    #[command(name = "cgroup-state")]
    CgroupState {
        /// Path to cgroup directory (default: /sys/fs/cgroup/agenticos)
        #[arg(default_value = "/sys/fs/cgroup/agenticos")]
        cgroup_path: String,
    },
    /// Show workload classification recommendations
    Recommendations,
    /// Show intelligence provider status (provider, model, API key, cache)
    #[command(name = "intelligence-status")]
    IntelligenceStatus,
    /// Show debug details for the last N Gemini classifications
    #[command(name = "classifications-debug")]
    ClassificationsDebug {
        /// Number of recent classifications to show
        #[arg(long, default_value = "5")]
        last: usize,
    },
    /// Parse a natural language request into a structured intent
    Ask {
        /// The natural language request to parse
        text: String,
    },
    /// Generate a plan from a previously stored intent
    Plan {
        /// The IntentId to plan from (e.g. IntentId-1)
        intent_id: String,
    },
    /// Show the action graph for a plan
    Actions {
        /// The PlanId to show actions for (e.g. PlanId-1)
        plan_id: String,
    },
    /// Execute a plan through the full governance pipeline
    Execute {
        /// The PlanId to execute (e.g. PlanId-1)
        plan_id: String,
    },
    /// List all stored intents
    Intents,
    /// List all stored plans
    Plans,
    /// Show detailed plan information (read-only)
    #[command(name = "plan-show")]
    PlanShow {
        /// The PlanId to inspect (e.g. PlanId-1)
        plan_id: String,
    },
    /// Execute a plan by intent ID (avoids plan selection errors)
    #[command(name = "execute-intent")]
    ExecuteIntent {
        /// The IntentId to execute (e.g. IntentId-1)
        intent_id: String,
    },
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let conn = match Connection::open(&cli.db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot open database '{}': {e}", cli.db);
            std::process::exit(1);
        }
    };

    // Verify the traces table exists
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='traces'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if table_count == 0 {
        eprintln!("error: database has no 'traces' table — is this an agenticos trace store?");
        std::process::exit(1);
    }

    let result = match &cli.command {
        None => {
            eprintln!("agenticos {}", env!("CARGO_PKG_VERSION"));
            eprintln!("Usage: agenticos <command>");
            eprintln!();
            eprintln!("Commands:");
            eprintln!("  status      Show daemon status and tick info");
            eprintln!("  agents      Show registered agents");
            eprintln!("  incidents   Show incidents");
            eprintln!("  traces      Show recent traces");
            eprintln!("  decisions   Show decisions and vetoes");
            eprintln!("  metrics     Show aggregated metrics");
            eprintln!("  replay      Reconstruct a trace by ID");
            eprintln!("  health      Run governance integrity checks");
            eprintln!("  recommendations Show workload classification recommendations");
            eprintln!("  cgroup-state Show live cgroup v2 state (cpu.weight, cpu.max, memory.max)");
            eprintln!("  classifications-debug Show debug info for last N Gemini classifications");
            eprintln!("  ask         Parse a natural language request into a structured intent");
            eprintln!("  plan        Generate a plan from a stored intent by ID");
            eprintln!("  actions     Show the action graph for a plan by ID");
            eprintln!("  execute     Execute a plan through the full governance pipeline");
            eprintln!("  intents     List all stored intents");
            eprintln!("  plans       List all stored plans");
            eprintln!("  plan-show   Show detailed plan information (read-only)");
            eprintln!("  execute-intent Execute a plan by intent ID");
            Ok(())
        }
        Some(Command::Status) => cmd_status(&conn),
        Some(Command::Agents) => cmd_agents(&conn),
        Some(Command::Incidents) => cmd_incidents(&conn),
        Some(Command::Traces) => cmd_traces(&conn),
        Some(Command::Decisions) => cmd_decisions(&conn),
        Some(Command::Metrics) => cmd_metrics(&conn),
        Some(Command::Replay { trace_id }) => cmd_replay(&conn, trace_id),
        Some(Command::Health) => cmd_health(&conn),
        Some(Command::CgroupState { cgroup_path }) => cmd_cgroup_state(cgroup_path),
        Some(Command::Recommendations) => cmd_recommendations(&conn),
        Some(Command::IntelligenceStatus) => cmd_intelligence_status(&conn),
        Some(Command::ClassificationsDebug { last }) => cmd_classifications_debug(&conn, *last),
        Some(Command::Ask { text }) => cmd_ask(&conn, &cli.db, text),
        Some(Command::Plan { intent_id }) => cmd_plan(&conn, &cli.db, intent_id),
        Some(Command::Actions { plan_id }) => cmd_actions(&conn, plan_id),
        Some(Command::Execute { plan_id }) => cmd_execute(&conn, plan_id),
        Some(Command::Intents) => cmd_intents(&conn, &cli.db),
        Some(Command::Plans) => cmd_plans(&conn),
        Some(Command::PlanShow { plan_id }) => cmd_plan_show(&conn, plan_id),
        Some(Command::ExecuteIntent { intent_id }) => cmd_execute_intent(&conn, intent_id),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StatusRow {
    daemon: String,
    tick_count: u64,
    active_agents: u64,
    total_events: u64,
    last_event_timestamp: String,
    recent_proposals: u64,
    recent_incidents: u64,
}

fn cmd_status(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let total_events: u64 = conn
        .query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))
        .unwrap_or(0);

    let last_ts: String = conn
        .query_row(
            "SELECT timestamp FROM traces ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "never".into());

    let tick_count: u64 = count_by_topic_prefix(conn, "metrics.")?;

    let active_agents: u64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT substr(topic, 10)) FROM traces WHERE topic LIKE 'proposals.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let recent_proposals: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'proposals.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let recent_incidents: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'incidents.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let alive = if total_events > 0 { "active" } else { "unknown" };

    let row = StatusRow {
        daemon: alive.into(),
        tick_count,
        active_agents,
        total_events,
        last_event_timestamp: last_ts,
        recent_proposals,
        recent_incidents,
    };

    print_output(&[row], &["DAEMON", "TICKS", "AGENTS", "EVENTS", "LAST_EVENT", "PROPOSALS", "INCIDENTS"],
        |r| vec![
            r.daemon.clone(),
            r.tick_count.to_string(),
            r.active_agents.to_string(),
            r.total_events.to_string(),
            r.last_event_timestamp.clone(),
            r.recent_proposals.to_string(),
            r.recent_incidents.to_string(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AgentRow {
    agent_id: String,
    agent_type: String,
    proposal_count: u64,
    incident_count: u64,
}

fn cmd_agents(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT substr(topic, 10) FROM traces WHERE topic LIKE 'proposals.%'",
    )?;
    let agent_ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut rows: Vec<AgentRow> = Vec::new();
    for agent_id in &agent_ids {
        let proposal_count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE topic = ?1",
            [format!("proposals.{agent_id}")],
            |row| row.get(0),
        )?;
        let incident_count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE ?1 AND payload_json LIKE ?2",
            [
                format!("incidents.%"),
                format!("%\"source_agent\":\"{agent_id}\"%"),
            ],
            |row| row.get(0),
        )?;

        rows.push(AgentRow {
            agent_id: agent_id.clone(),
            agent_type: infer_agent_type(agent_id),
            proposal_count,
            incident_count,
        });
    }

    // Also include agents that only produced incidents
    let mut stmt2 = conn.prepare(
        "SELECT DISTINCT json_extract(payload_json, '$.source_agent') FROM traces WHERE topic LIKE 'incidents.%'",
    )?;
    let incident_only_ids: Vec<String> = stmt2
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter(|id| !agent_ids.contains(id))
        .collect();

    for agent_id in &incident_only_ids {
        let incident_count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'incidents.%' AND payload_json LIKE ?1",
            [format!("%\"source_agent\":\"{agent_id}\"%")],
            |row| row.get(0),
        )?;

        rows.push(AgentRow {
            agent_id: agent_id.clone(),
            agent_type: infer_agent_type(agent_id),
            proposal_count: 0,
            incident_count,
        });
    }

    if rows.is_empty() {
        println!("No agents found. The daemon may not have run yet.");
        return Ok(());
    }

    print_output(&rows, &["AGENT_ID", "TYPE", "PROPOSALS", "INCIDENTS"],
        |r| vec![
            r.agent_id.clone(),
            r.agent_type.clone(),
            r.proposal_count.to_string(),
            r.incident_count.to_string(),
        ],
    );

    Ok(())
}

fn infer_agent_type(agent_id: &str) -> String {
    if agent_id.contains("memory") || agent_id.contains("mem") {
        "Memory".into()
    } else if agent_id.contains("process") || agent_id.contains("proc") {
        "Process".into()
    } else if agent_id.contains("security") || agent_id.contains("sec") {
        "Security".into()
    } else if agent_id.contains("dummy") || agent_id.contains("Dummy") {
        "Dummy".into()
    } else {
        "Unknown".into()
    }
}

// ---------------------------------------------------------------------------
// Incidents
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct IncidentRow {
    incident_id: String,
    category: String,
    severity: String,
    source_agent: String,
    timestamp: String,
}

fn cmd_incidents(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json, timestamp FROM traces WHERE topic LIKE 'incidents.%' ORDER BY id"
    )?;

    let rows: Vec<IncidentRow> = stmt
        .query_map([], |row| {
            let payload_json: String = row.get(0)?;
            let timestamp: String = row.get(1)?;
            Ok((payload_json, timestamp))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(payload_json, timestamp)| {
            // Try to extract incident fields from JSON
            let v: serde_json::Value = serde_json::from_str(&payload_json).ok()?;
            let incident = v.get("Incident")?;
            Some(IncidentRow {
                incident_id: incident.get("incident_id")?.as_str()?.to_owned(),
                category: incident
                    .get("category")?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                severity: incident
                    .get("severity")?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                source_agent: incident
                    .get("source_agent")?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                timestamp,
            })
        })
        .collect();

    if rows.is_empty() {
        println!("No incidents found.");
        return Ok(());
    }

    print_output(&rows, &["ID", "CATEGORY", "SEVERITY", "SOURCE", "TIMESTAMP"],
        |r| vec![
            r.incident_id.clone(),
            r.category.clone(),
            r.severity.clone(),
            r.source_agent.clone(),
            r.timestamp.clone(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Traces
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TraceRow {
    trace_id: String,
    event_count: u64,
    first_timestamp: String,
    last_timestamp: String,
}

fn cmd_traces(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT trace_id, COUNT(*) as cnt, MIN(timestamp) as first_ts, MAX(timestamp) as last_ts
         FROM traces GROUP BY trace_id ORDER BY MAX(id) DESC LIMIT 20"
    )?;

    let rows: Vec<TraceRow> = stmt
        .query_map([], |row| {
            Ok(TraceRow {
                trace_id: row.get(0)?,
                event_count: row.get(1)?,
                first_timestamp: row.get(2)?,
                last_timestamp: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        println!("No traces found.");
        return Ok(());
    }

    print_output(&rows, &["TRACE_ID", "EVENTS", "FIRST", "LAST"],
        |r| vec![
            r.trace_id.clone(),
            r.event_count.to_string(),
            r.first_timestamp.clone(),
            r.last_timestamp.clone(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DecisionRow {
    decision_id: String,
    proposal_id: String,
    outcome: String,
    explanation: String,
}

fn cmd_decisions(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    // Approved/Denied decisions from decisions.* topics
    let mut stmt = conn.prepare(
        "SELECT payload_json FROM traces WHERE topic LIKE 'decisions.%' ORDER BY id"
    )?;

    let mut rows: Vec<DecisionRow> = Vec::new();
    let payloads: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for payload_json in &payloads {
        let v: serde_json::Value = match serde_json::from_str(payload_json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let decision = match v.get("Decision") {
            Some(d) => d,
            None => continue,
        };
        let outcome_val = &decision["outcome"];
        let outcome = match outcome_val.get("Approved") {
            Some(_) => "Approved".into(),
            None => match outcome_val.get("Denied") {
                Some(d) => format!("Denied({})", d.get("reason").and_then(|r| r.as_str()).unwrap_or("?")),
                None => "RequiresApproval".into(),
            },
        };

        rows.push(DecisionRow {
            decision_id: decision["id"].as_str().unwrap_or("?").into(),
            proposal_id: decision["proposal_id"].as_str().unwrap_or("?").into(),
            outcome,
            explanation: decision["explanation"].as_str().unwrap_or("").into(),
        });
    }

    // Vetoes from vetoes.* topics
    let mut stmt2 = conn.prepare(
        "SELECT payload_json FROM traces WHERE topic LIKE 'vetoes.%' ORDER BY id"
    )?;
    let veto_payloads: Vec<String> = stmt2
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for payload_json in &veto_payloads {
        let msg = extract_trace_message(payload_json);
        rows.push(DecisionRow {
            decision_id: "-".into(),
            proposal_id: extract_proposal_id_from_msg(&msg),
            outcome: "Vetoed".into(),
            explanation: msg,
        });
    }

    if rows.is_empty() {
        println!("No decisions found.");
        return Ok(());
    }

    print_output(&rows, &["DECISION_ID", "PROPOSAL_ID", "OUTCOME", "EXPLANATION"],
        |r| vec![
            r.decision_id.clone(),
            r.proposal_id.clone(),
            r.outcome.clone(),
            r.explanation.clone(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct MetricsRow {
    metric: String,
    value: String,
}

fn cmd_metrics(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let proposal_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'proposals.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let incident_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'incidents.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let veto_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'vetoes.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let result_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'results.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Approved / denied decision counts
    let approved_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'decisions.%' AND payload_json LIKE '%\"Approved\":%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let denied_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'decisions.%' AND payload_json LIKE '%\"Denied\":%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Veto reason breakdown — group by topic suffix (e.g., vetoes.selective-veto)
    let mut veto_breakdown: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT topic, COUNT(*) as cnt FROM traces WHERE topic LIKE 'vetoes.%' GROUP BY topic"
        )?;
        let rows = stmt.query_map([], |row| {
            let topic: String = row.get(0)?;
            let cnt: u64 = row.get(1)?;
            Ok((topic, cnt))
        })?;
        for row in rows {
            if let Ok((topic, cnt)) = row {
                // Extract reason from "vetoes.{reason}"
                let reason = topic.strip_prefix("vetoes.").unwrap_or(&topic).to_owned();
                veto_breakdown.insert(reason, cnt);
            }
        }
    }

    // Extract latest decision latency from metrics
    let decision_latency: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE topic = 'metrics.daemon' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| {
            let v: serde_json::Value = serde_json::from_str(&json).ok()?;
            let samples = v.get("samples")?.as_array()?;
            for sample in samples {
                if sample.get("name")?.as_str()? == "decision_latency_ms" {
                    let vals = sample.get("value")?.get("Histogram")?.as_array()?;
                    return vals.first().and_then(|v| v.as_f64()).map(|v| format!("{v:.1}ms"));
                }
            }
            None
        })
        .unwrap_or_else(|| "N/A".into());

    let recommendation_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let gemini_requests: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND json_extract(payload_json, '$.Recommendation.provider.provider_name') = 'gemini'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let cache_hits: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND json_extract(payload_json, '$.Recommendation.provider.cache_hit') = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let cache_misses: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND (json_extract(payload_json, '$.Recommendation.provider.cache_hit') IS NULL OR json_extract(payload_json, '$.Recommendation.provider.cache_hit') = 0)",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Extract classifications_skipped_total from the latest metrics.daemon trace
    let classifications_skipped_total: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE topic = 'metrics.daemon' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| {
            let v: serde_json::Value = serde_json::from_str(&json).ok()?;
            let samples = v.get("samples")?.as_array()?;
            for sample in samples {
                if sample.get("name")?.as_str()? == "classifications_skipped_total" {
                    let vals = sample.get("value")?.get("Counter")?;
                    return vals.as_u64().map(|v| v.to_string());
                }
            }
            None
        })
        .unwrap_or_else(|| "0".into());

    let mut rows = vec![
        MetricsRow { metric: "proposal_count".into(), value: proposal_count.to_string() },
        MetricsRow { metric: "incident_count".into(), value: incident_count.to_string() },
        MetricsRow { metric: "veto_count".into(), value: veto_count.to_string() },
        MetricsRow { metric: "approved_count".into(), value: approved_count.to_string() },
        MetricsRow { metric: "denied_count".into(), value: denied_count.to_string() },
        MetricsRow { metric: "executor_count".into(), value: result_count.to_string() },
        MetricsRow { metric: "decision_latency".into(), value: decision_latency },
        MetricsRow { metric: "recommendation_count".into(), value: recommendation_count.to_string() },
        MetricsRow { metric: "gemini_requests_total".into(), value: gemini_requests.to_string() },
        MetricsRow { metric: "cache_hits_total".into(), value: cache_hits.to_string() },
        MetricsRow { metric: "cache_misses_total".into(), value: cache_misses.to_string() },
        MetricsRow { metric: "classifications_skipped_total".into(), value: classifications_skipped_total },
    ];

    for (reason, cnt) in &veto_breakdown {
        rows.push(MetricsRow {
            metric: format!("veto_reason/{reason}"),
            value: cnt.to_string(),
        });
    }

    print_output(&rows, &["METRIC", "VALUE"],
        |r| vec![r.metric.clone(), r.value.clone()],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ReplayRow {
    seq: u64,
    topic: String,
    payload_type: String,
    summary: String,
}

fn cmd_replay(conn: &Connection, trace_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT id, topic, payload_json, timestamp FROM traces WHERE trace_id = ?1 ORDER BY id"
    )?;

    let rows: Vec<ReplayRow> = stmt
        .query_map(rusqlite::params![trace_id], |row| {
            let id: i64 = row.get(0)?;
            let topic: String = row.get(1)?;
            let payload_json: String = row.get(2)?;
            let timestamp: String = row.get(3)?;
            Ok((id, topic, payload_json, timestamp))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, topic, payload_json, timestamp)| {
            let payload_type = extract_payload_type(&payload_json);
            let summary = summarize_payload(&payload_json, &payload_type);
            ReplayRow {
                seq: id as u64,
                topic,
                payload_type,
                summary: format!("{timestamp} {summary}"),
            }
        })
        .collect();

    if rows.is_empty() {
        println!("No trace found with ID '{trace_id}'.");
        return Ok(());
    }

    print_output(&rows, &["SEQ", "TOPIC", "TYPE", "TIMESTAMP SUMMARY"],
        |r| vec![
            r.seq.to_string(),
            r.topic.clone(),
            r.payload_type.clone(),
            r.summary.clone(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthRow {
    check: String,
    status: String,
    detail: String,
}

fn cmd_health(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks: Vec<HealthRow> = Vec::new();

    // 1. Security Agent proposal count = 0
    let sec_proposals: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic = 'proposals.security-agent'",
        [],
        |row| row.get(0),
    )?;
    checks.push(HealthRow {
        check: "Security Agent proposals".into(),
        status: if sec_proposals == 0 { "PASS" } else { "FAIL" }.into(),
        detail: format!("Security Agent produced {sec_proposals} proposals (must be 0)"),
    });

    // 2. Safety Governor active
    let veto_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic LIKE 'vetoes.%'",
        [],
        |row| row.get(0),
    )?;
    let has_metrics: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic = 'metrics.daemon' AND payload_json LIKE '%safety_veto_count%'",
        [],
        |row| row.get(0),
    )?;
    checks.push(HealthRow {
        check: "Safety Governor active".into(),
        status: if has_metrics > 0 { "PASS" } else { "WARN" }.into(),
        detail: if veto_count > 0 {
            format!("Safety Governor active, {veto_count} vetoes recorded")
        } else {
            "Safety Governor metrics found, no vetoes yet".into()
        },
    });

    // 3. No Observation → Action path (procedural check)
    let obs_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic LIKE 'observations.%'",
        [],
        |row| row.get(0),
    )?;
    let dec_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic LIKE 'decisions.%'",
        [],
        |row| row.get(0),
    )?;
    let has_pipeline = obs_count > 0 && dec_count > 0;
    checks.push(HealthRow {
        check: "Observation→Decision pipeline".into(),
        status: if has_pipeline { "PASS" } else { "WARN" }.into(),
        detail: format!("{obs_count} observations, {dec_count} decisions"),
    });

    // 4. Database integrity
    let total_events: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces",
        [],
        |row| row.get(0),
    )?;
    checks.push(HealthRow {
        check: "Trace store integrity".into(),
        status: "PASS".into(),
        detail: format!("{total_events} events stored"),
    });

    // 5. Proposal → Decision correspondence
    let prop_count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic LIKE 'proposals.%'",
        [],
        |row| row.get(0),
    )?;
    let dec_count2: u64 = conn.query_row(
        "SELECT COUNT(*) FROM traces WHERE topic LIKE 'decisions.%'",
        [],
        |row| row.get(0),
    )?;
    let decision_coverage = if prop_count > 0 {
        format!("{:.1}%", (dec_count2 as f64 / prop_count as f64) * 100.0)
    } else {
        "N/A".into()
    };
    checks.push(HealthRow {
        check: "Decision coverage".into(),
        status: if prop_count == 0 || dec_count2 >= prop_count { "PASS" } else { "WARN" }.into(),
        detail: format!("{prop_count} proposals → {dec_count2} decisions ({decision_coverage})"),
    });

    print_output(&checks, &["CHECK", "STATUS", "DETAIL"],
        |r| vec![r.check.clone(), r.status.clone(), r.detail.clone()],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn print_output<T, F>(rows: &[T], headers: &[&str], extract: F)
where
    T: Serialize,
    F: Fn(&T) -> Vec<String>,
{
    // Determine if we want JSON output
    // We check the CLI args by re-parsing (simple approach)
    let is_json = std::env::args().any(|a| a == "--json");

    if is_json {
        let json = serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".into());
        println!("{json}");
        return;
    }

    // Column widths
    let data: Vec<Vec<String>> = rows.iter().map(|r| extract(r)).collect();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in &data {
        for (i, val) in row.iter().enumerate() {
            widths[i] = widths[i].max(val.len());
        }
    }

    // Header
    let header_line: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
        .collect();
    println!("{}", header_line.join("  "));

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep.join("  "));

    // Data
    for row in &data {
        let line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, val)| format!("{:width$}", val, width = widths[i]))
            .collect();
        println!("{}", line.join("  "));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn count_by_topic_prefix(conn: &Connection, prefix: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE ?1",
            [format!("{prefix}%")],
            |row| row.get(0),
        )?)
}

fn extract_payload_type(payload_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(v) => v,
        Err(_) => return "Unknown".into(),
    };
    match v.as_object() {
        Some(map) => map.keys().next().cloned().unwrap_or_else(|| "Unknown".into()),
        None => "Unknown".into(),
    }
}

fn summarize_payload(payload_json: &str, payload_type: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(v) => v,
        Err(_) => return "".into(),
    };
    let inner = match v.get(payload_type) {
        Some(i) => i,
        None => return "".into(),
    };
    match payload_type {
        "Observation" => inner
            .get("source")
            .and_then(|s| s.as_str())
            .map(|s| format!("source={s}"))
            .unwrap_or_default(),
        "Proposal" => inner
            .get("agent_id")
            .and_then(|a| a.as_str())
            .map(|a| format!("agent={a}"))
            .unwrap_or_default(),
        "Decision" => {
            let outcome = inner.get("outcome").and_then(|o| {
                if o.get("Approved").is_some() {
                    Some("Approved")
                } else if o.get("Denied").is_some() {
                    Some("Denied")
                } else {
                    Some("?")
                }
            }).unwrap_or("?");
            let by = inner.get("decided_by").and_then(|d| d.as_str()).unwrap_or("?");
            format!("outcome={outcome} by={by}")
        }
        "ActionResult" => inner
            .get("status")
            .and_then(|s| s.as_str())
            .map(|s| format!("status={s}"))
            .unwrap_or_default(),
        "Incident" => {
            let cat = inner.get("category").and_then(|c| c.as_str()).unwrap_or("?");
            let sev = inner.get("severity").and_then(|s| s.as_str()).unwrap_or("?");
            format!("category={cat} severity={sev}")
        }
        "Trace" => inner
            .get("message")
            .and_then(|m| m.as_str())
            .map(|m| truncate(m, 60))
            .unwrap_or_default(),
        _ => "".into(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max])
    }
}

fn extract_trace_message(payload_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()
        .and_then(|v| {
            v.get("Trace")
                .and_then(|t| t.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_owned())
        })
        .unwrap_or_default()
}

fn extract_proposal_id_from_msg(msg: &str) -> String {
    // Format: "veto proposal=ProposalId-42 reason=... explanation=..."
    for part in msg.split_whitespace() {
        if let Some(rest) = part.strip_prefix("proposal=") {
            return rest.to_owned();
        }
    }
    "?".into()
}

// ---------------------------------------------------------------------------
// Cgroup State
// ---------------------------------------------------------------------------

fn cmd_cgroup_state(cgroup_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    cmd_cgroup_state_impl(cgroup_path)
}

#[cfg(target_os = "linux")]
fn cmd_cgroup_state_impl(cgroup_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(cgroup_path);
    if !path.exists() {
        return Err(format!("cgroup path '{}' does not exist", cgroup_path).into());
    }

    let read_file = |name: &str| -> String {
        let p = path.join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|_| "N/A".into()).trim().to_owned()
    };

    let cpu_weight = read_file("cpu.weight");
    let cpu_max = read_file("cpu.max");
    let memory_max = read_file("memory.max");
    let procs = read_file("cgroup.procs");
    let controllers = read_file("cgroup.controllers");

    println!("Cgroup:           {}", cgroup_path);
    println!("cpu.weight:       {}", cpu_weight);
    println!("cpu.max:          {}", cpu_max);
    println!("memory.max:       {}", memory_max);
    println!("cgroup.procs:     {}", procs.lines().count());
    println!("controllers:      {}", controllers);

    let procs_line_count = procs.lines().count();
    println!();
    println!("Processes in cgroup: {} PIDs", procs_line_count);

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn cmd_cgroup_state_impl(_cgroup_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("error: cgroup-state requires Linux (cgroup v2)");
    Err("not supported on this platform".into())
}

// ---------------------------------------------------------------------------
// Recommendations
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RecommendationRow {
    timestamp: String,
    agent: String,
    classification: String,
    confidence: f64,
    summary: String,
    provider_name: String,
    model_name: String,
    cache_hit: bool,
}

fn cmd_recommendations(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json, timestamp FROM traces WHERE topic LIKE 'recommendations.%' ORDER BY id"
    )?;

    let rows: Vec<RecommendationRow> = stmt
        .query_map([], |row| {
            let payload_json: String = row.get(0)?;
            let timestamp: String = row.get(1)?;
            Ok((payload_json, timestamp))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(payload_json, timestamp)| {
            let v: serde_json::Value = serde_json::from_str(&payload_json).ok()?;
            let rec = v.get("Recommendation")?;
            let classification = rec
                .get("summary")?
                .as_str()?
                .strip_prefix("Workload classified as ")
                .unwrap_or("?")
                .to_owned();
            let confidence = rec.get("confidence")?.as_f64()?;
            let summary = rec.get("summary")?.as_str()?.to_owned();
            let agent = rec.get("source_agent")?.as_str()?.to_owned();
            let provider = rec.get("provider");
            let (provider_name, model_name, cache_hit) = match provider {
                Some(p) => (
                    p.get("provider_name").and_then(|v| v.as_str()).unwrap_or("unknown").to_owned(),
                    p.get("model_name").and_then(|v| v.as_str()).unwrap_or("unknown").to_owned(),
                    p.get("cache_hit").and_then(|v| v.as_bool()).unwrap_or(false),
                ),
                None => ("unknown".into(), "unknown".into(), false),
            };
            Some(RecommendationRow {
                timestamp,
                agent,
                classification,
                confidence,
                summary,
                provider_name,
                model_name,
                cache_hit,
            })
        })
        .collect();

    if rows.is_empty() {
        println!("No recommendations found.");
        return Ok(());
    }

    print_output(&rows, &["TIMESTAMP", "AGENT", "CLASSIFICATION", "CONFIDENCE", "PROVIDER", "MODEL", "CACHE_HIT", "SUMMARY"],
        |r| vec![
            r.timestamp.clone(),
            r.agent.clone(),
            r.classification.clone(),
            format!("{:.2}", r.confidence),
            r.provider_name.clone(),
            r.model_name.clone(),
            if r.cache_hit { "true".into() } else { "false".into() },
            r.summary.clone(),
        ],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Intelligence Status
// ---------------------------------------------------------------------------

fn cmd_intelligence_status(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = agenticos_intelligence::IntelligenceConfig::default();

    let api_key_present = cfg.api_key_present();

    let cache_entries = match agenticos_intelligence::RecommendationCache::new(&cfg.cache_path) {
        Ok(cache) => cache.len().unwrap_or(0),
        Err(_) => 0,
    };

    let recommendation_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let gemini_requests_total: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND json_extract(payload_json, '$.Recommendation.provider.provider_name') = 'gemini'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let cache_hits_total: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND json_extract(payload_json, '$.Recommendation.provider.cache_hit') = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let cache_misses_total: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM traces WHERE topic LIKE 'recommendations.%' AND (json_extract(payload_json, '$.Recommendation.provider.cache_hit') IS NULL OR json_extract(payload_json, '$.Recommendation.provider.cache_hit') = 0)",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let classifications_skipped_total: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE topic = 'metrics.daemon' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| {
            let v: serde_json::Value = serde_json::from_str(&json).ok()?;
            let samples = v.get("samples")?.as_array()?;
            for sample in samples {
                if sample.get("name")?.as_str()? == "classifications_skipped_total" {
                    let vals = sample.get("value")?.get("Counter")?;
                    return vals.as_u64().map(|v| v.to_string());
                }
            }
            None
        })
        .unwrap_or_else(|| "0".into());

    let last_recommendation: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE topic LIKE 'recommendations.%' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| {
            let v: serde_json::Value = serde_json::from_str(&json).ok()?;
            let rec = v.get("Recommendation")?;
            let provider = rec.get("provider");
            let provider_name = provider
                .and_then(|p| p.get("provider_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_owned();
            let timestamp = rec.get("timestamp")?.as_str()?.to_owned();
            Some((provider_name, timestamp))
        });

    let (last_provider_used, last_recommendation_timestamp) = match last_recommendation {
        Some((p, ts)) => (p, ts),
        None => ("none".into(), "none".into()),
    };

    let output = format!(
        "configured_provider            = {}
selected_provider            = {}
model                        = {}
api_key_present              = {}
cache_entries                = {}
recommendation_count         = {}
gemini_requests_total        = {}
cache_hits_total             = {}
cache_misses_total           = {}
classifications_skipped_total = {}
last_provider_used           = {}
last_recommendation_timestamp = {}",
        cfg.provider_name,
        cfg.provider_name,
        cfg.model,
        if api_key_present { "true" } else { "false" },
        cache_entries,
        recommendation_count,
        gemini_requests_total,
        cache_hits_total,
        cache_misses_total,
        classifications_skipped_total,
        last_provider_used,
        last_recommendation_timestamp,
    );

    println!("{output}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Classifications Debug
// ---------------------------------------------------------------------------

fn cmd_classifications_debug(conn: &Connection, last: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json, timestamp FROM traces WHERE topic LIKE 'recommendations.%' ORDER BY id DESC LIMIT ?1"
    )?;

    let rows: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![last as i64], |row| {
            let payload_json: String = row.get(0)?;
            Ok(payload_json)
        })?
        .filter_map(|r| r.ok())
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .collect();

    if rows.is_empty() {
        println!("No classifications found.");
        return Ok(());
    }

    for (i, v) in rows.iter().enumerate() {
        let rec = match v.get("Recommendation") {
            Some(r) => r,
            None => continue,
        };

        let timestamp = rec.get("timestamp").and_then(|t| t.as_str()).unwrap_or("?");
        let summary = rec.get("summary").and_then(|s| s.as_str()).unwrap_or("?");
        let confidence = rec.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0);
        let provider = rec.get("provider");
        let cache_hit = provider.and_then(|p| p.get("cache_hit")).and_then(|c| c.as_bool()).unwrap_or(false);
        let extra = provider.and_then(|p| p.get("extra")).and_then(|e| e.as_object());

        let prompt = extra
            .and_then(|e| e.get("prompt"))
            .and_then(|v| v.as_str())
            .unwrap_or("(not recorded)");
        let raw_response = extra
            .and_then(|e| e.get("raw_response"))
            .and_then(|v| v.as_str())
            .unwrap_or("(not recorded)");
        let parsed = extra
            .and_then(|e| e.get("parsed_classification"))
            .and_then(|v| v.as_str())
            .unwrap_or("(not recorded)");
        let obs_summary = extra
            .and_then(|e| e.get("observation_summary"))
            .and_then(|v| v.as_str())
            .unwrap_or("(not recorded)");
        let fallback_reason = extra
            .and_then(|e| e.get("fallback_reason"))
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        let parse_error = extra
            .and_then(|e| e.get("parse_error"))
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");

        println!("────────────────────────────────────────────────────────────");
        println!("#{} — {}  cache_hit={}", i + 1, timestamp, if cache_hit { "true" } else { "false" });
        println!();
        println!("Parsed Classification: {}", parsed);
        println!("Confidence:            {:.2}", confidence);
        println!("Fallback Reason:       {}", fallback_reason);
        println!("Parse Error:           {}", parse_error);
        println!("Summary:               {}", summary);
        println!();
        println!("--- Observation Summary ---");
        println!("{}", obs_summary);
        println!();
        println!("--- Prompt Sent To Gemini ---");
        println!("{}", prompt);
        println!();
        if raw_response != "(not recorded)" && raw_response != "API call failed" {
            if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(raw_response) {
                let pretty = serde_json::to_string_pretty(&parsed_json).unwrap_or_else(|_| raw_response.to_owned());
                println!("--- Raw Gemini Response (JSON) ---");
                println!("{}", pretty);
            } else {
                println!("--- Raw Gemini Response ---");
                println!("{}", raw_response);
            }
        } else {
            println!("--- Raw Gemini Response ---");
            println!("{}", raw_response);
        }
        println!();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Ask (Intent Parsing)
// ---------------------------------------------------------------------------

fn cmd_ask(conn: &Connection, db_path: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    // CLI uses MockIntentParser — deterministic, no API key, no rate limits.
    // GeminiIntentParser is available programmatically for daemon integration.
    let parser: Box<dyn agenticos_intelligence::IntentParser> =
        Box::new(agenticos_intelligence::MockIntentParser::new());

    // Store intents in the same database as the trace store.
    // Use a separate table "intents" alongside "traces".
    let store = match agenticos_intelligence::IntentStore::new(db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot open intent store: {e}");
            std::process::exit(1);
        }
    };

    let agent = agenticos_intelligence::IntentAgent::new(parser, store);

    match agent.parse_and_store(text) {
        Ok(intent) => {
            println!("Detected Intent:");
            println!("  type:       {}", intent.intent_type);
            println!("  confidence: {:.2}", intent.confidence);
            if !intent.parameters.is_empty() {
                println!("  parameters:");
                for (k, v) in &intent.parameters {
                    println!("    {}: {}", k, v);
                }
            }
            println!("  id:         {}", intent.id);
            println!("  text:       {}", intent.source_text);

            // Persist to the trace store database as well, so it appears in replay.
            let _ = persist_intent_to_trace_store(conn, &intent);
        }
        Err(e) => {
            eprintln!("error: failed to parse intent: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn cmd_plan(conn: &Connection, db_path: &str, intent_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use agenticos_intelligence::PlannerAgent;

    // Look up the intent from the traces table by message_id.
    let payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![intent_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("intent '{}' not found in trace store", intent_id))?;

    let intent: agenticos_domain::Intent =
        serde_json::from_str(&payload_json)
            .map_err(|e| format!("failed to deserialize intent '{}': {e}", intent_id))?;

    // Create plan store (persistent DB) and planner.
    // The plan store shares the same DB file as the trace store and intent store.
    let plan_store = agenticos_intelligence::PlanStore::new(db_path)
        .map_err(|e| format!("cannot open plan store: {e}"))?;

    let planner = agenticos_intelligence::MockPlannerAgent::new();
    let mut plan = planner
        .create_plan(&intent)
        .map_err(|e| format!("failed to create plan for '{}': {e}", intent.intent_type))?;

    // Override the auto-generated PlanId with a DB-backed persistent ID
    plan.id = plan_store.generate_id();

    plan_store.insert(&plan)?;

    println!("Plan: {}", plan.id);
    println!("Intent: {} ({})", intent.id, intent.intent_type);
    println!("Status: {}", plan.status);
    println!("Steps:");
    for step in &plan.steps {
        println!("  {}. {}", step.order, step.action);
        for (k, v) in &step.parameters {
            println!("     {}: {}", k, v);
        }
    }

    // Persist to trace store
    let _ = persist_plan_to_trace_store(conn, &plan);

    Ok(())
}

fn persist_plan_to_trace_store(
    conn: &Connection,
    plan: &agenticos_domain::TaskPlan,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(plan)?;
    conn.execute(
        "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            plan.id.as_str(),
            "plan-cli",
            Option::<String>::None,
            format!("plans.{}", plan.status),
            plan.timestamp,
            json,
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Execute (Full Governance Pipeline)
// ---------------------------------------------------------------------------

fn cmd_execute(conn: &Connection, plan_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use agenticos_domain::{AgentId, Decision};
    use agenticos_intelligence::{ActionGraphBuilder, StaticToolRegistry, ToolResolver};
    use agenticos_agents::ActionProposalAgent;
    use agenticos_policy::{ActionProposalPolicy, DefaultActionProposalPolicy};
        use agenticos_safety::SafetyActionValidator;
    use agenticos_executor::{DefaultActionExecutor, ApprovedActionExecutor};

    // Step 1: Load the plan from trace store
    println!("=== Execute Pipeline ===");
    println!();

    let payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![plan_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("plan '{}' not found in trace store", plan_id))?;

    let plan: agenticos_domain::TaskPlan =
        serde_json::from_str(&payload_json)
            .map_err(|e| format!("failed to deserialize plan '{}': {e}", plan_id))?;

    println!("Plan: {} ({} steps)", plan.id, plan.steps.len());
    println!("Status: {}", plan.status);
    println!();

    // Step 2: Build ActionGraph
    let registry = StaticToolRegistry::new();
    let resolver = ToolResolver::new(Box::new(registry));
    let builder = ActionGraphBuilder::new(resolver);
    let graph = builder
        .build(&plan)
        .ok_or_else(|| format!("plan '{}' has no steps", plan_id))?;

    println!("Action Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());
    println!();

    // Step 3: Create Proposals
    let proposal_agent = ActionProposalAgent::new(AgentId::from("action-proposal-agent"));
    let proposals = proposal_agent.propose(&graph);

    println!("Proposals created: {}", proposals.len());
    for prop in &proposals {
        let kind_str = format!("{:?}", prop.requested_action.kind);
        println!("  Proposal {}: {} (safety={:?})",
            prop.id, kind_str, prop.requested_action.safety_level);
    }
    println!();

    // Step 4: Policy Evaluation
    let policy = DefaultActionProposalPolicy::default();
    let safety_validator = SafetyActionValidator::default();
    let executor = DefaultActionExecutor::new();
    let mut approved_proposals: Vec<(Decision, agenticos_domain::Proposal)> = Vec::new();
    let mut executor_call_count: u64 = 0;
    let mut action_results: Vec<(String, bool)> = Vec::new(); // (action_id, was_executed)

    println!("--- Governance Results ---");
    println!();

    for proposal in &proposals {
        let kind_str = format!("{:?}", proposal.requested_action.kind);
        let action_label = proposal.id.as_str().split('-').next().unwrap_or("?");

        let policy_ok = match policy.check(&proposal.requested_action) {
            Ok(true) => {
                println!("  Action {}: Policy ALLOW", action_label);
                println!("           Kind: {}", kind_str);
                // Create a synthetic approved decision
                let decision = Decision {
                    id: agenticos_domain::DecisionId::new(),
                    proposal_id: proposal.id.clone(),
                    decided_at: now_utc(),
                    decided_by: AgentId::from("action-proposal-policy"),
                    outcome: agenticos_domain::DecisionOutcome::Approved,
                    explanation: "allowed by action proposal policy".into(),
                };
                approved_proposals.push((decision, proposal.clone()));
                true
            }
            Ok(false) => {
                let reason = policy.explain_denial(&proposal.requested_action);
                println!("  Action {}: Policy DENY — {}", action_label, reason);
                println!("           Kind: {}", kind_str);
                println!("           Executor SKIPPED");
                action_results.push((proposal.id.as_str().to_string(), false));
                false
            }
            Err(e) => {
                println!("  Action {}: Policy ERROR — {e}", action_label);
                println!("           Executor SKIPPED");
                action_results.push((proposal.id.as_str().to_string(), false));
                false
            }
        };

        if policy_ok {
            // Step 5: Safety Validation
            match safety_validator.validate(&proposal.requested_action) {
                Ok(()) => {
                    println!("           Safety ALLOW");
                    // Step 6: Execution (only if both policy and safety pass)
                    let decision = &approved_proposals.last().unwrap().0;
                    let approved_action = agenticos_domain::ApprovedAction {
                        request: proposal.requested_action.clone(),
                        decision_id: decision.id.clone(),
                    };
                    match executor.execute(approved_action) {
                        Ok(result) => {
                            println!("           Executor CALLED — {} ({:?}, {}ms)",
                                result.message, result.status, result.duration_ms);
                            if let Some(token) = &result.rollback {
                                println!("             rollback token: {}", token.token);
                            }
                            executor_call_count += 1;
                            action_results.push((proposal.id.as_str().to_string(), true));

                            // Persist executor result trace for audit
                            let _ = conn.execute(
                                "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                rusqlite::params![
                                    proposal.id.as_str(),
                                    "execute-cli",
                                    Some(plan.id.as_str()),
                                    "executor.called",
                                    now_utc(),
                                    serde_json::to_string(&result).unwrap_or_default(),
                                ],
                            );
                        }
                        Err(e) => {
                            println!("           Executor CALLED — FAILED: {e}");
                            executor_call_count += 1;
                            action_results.push((proposal.id.as_str().to_string(), true));

                            // Persist executor failure trace
                            let _ = conn.execute(
                                "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                rusqlite::params![
                                    proposal.id.as_str(),
                                    "execute-cli",
                                    Some(plan.id.as_str()),
                                    "executor.called",
                                    now_utc(),
                                    format!("{{\"error\":\"{e}\"}}"),
                                ],
                            );
                        }
                    }
                }
                Err(reason) => {
                    println!("           Safety VETO — {reason}");
                    println!("           Executor SKIPPED");
                    action_results.push((proposal.id.as_str().to_string(), false));
                }
            }
        }
        println!();
    }

    println!("--- Summary ---");
    println!("  Total actions: {}", proposals.len());
    println!("  Policy ALLOW:  {}", approved_proposals.len());
    println!("  Executor calls: {}", executor_call_count);
    if executor_call_count == 0 && proposals.len() > 0 {
        println!("  Guard: Actions blocked by governance — executor never called.");
    }
    println!();
    println!("Pipeline complete.");

    // Persist execution trace
    {
        let json = serde_json::to_string(&graph)?;
        conn.execute(
            "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                plan.id.as_str(),
                "execute-cli",
                Option::<String>::None,
                format!("execute.{}", plan.status),
                plan.timestamp,
                json,
            ],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Plans (List all stored plans)
// ---------------------------------------------------------------------------

fn cmd_intents(_conn: &Connection, db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = agenticos_intelligence::IntentStore::new(db_path)
        .map_err(|e| format!("cannot open intent store: {e}"))?;

    let intents = store.list()?;

    if intents.is_empty() {
        println!("No intents found.");
        return Ok(());
    }

    println!("{:<12} {:<16} {:<8} TEXT", "INTENT ID", "TYPE", "CONFIDENCE");
    println!("{:-<12} {:-<16} {:-<8} {:-<20}", "-", "-", "-", "-");
    for intent in &intents {
        let text = if intent.source_text.len() > 20 {
            format!("{}...", &intent.source_text[..17])
        } else {
            intent.source_text.clone()
        };
        println!(
            "{:<12} {:<16} {:<8.2} {}",
            intent.id.as_str(),
            intent.intent_type,
            intent.confidence,
            text,
        );
    }
    Ok(())
}

#[derive(Serialize)]
struct PlanSummaryRow {
    plan_id: String,
    intent_id: String,
    intent_type: String,
    status: String,
}

fn cmd_plans(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, payload_json FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC"
    )?;

    let mut rows: Vec<PlanSummaryRow> = Vec::new();
    for result in stmt.query_map([], |row| {
        let mid: String = row.get(0)?;
        let payload: String = row.get(1)?;
        Ok((mid, payload))
    })? {
        let (plan_id, payload_json) = result?;
        let plan: agenticos_domain::TaskPlan = match serde_json::from_str(&payload_json) {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Look up the intent type from intents trace by matching intent ID
        let intent_type: String = conn
            .query_row(
                "SELECT topic FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![plan.source_intent_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "intents.unknown".into());

        // Extract the type part after "intents."
        let itype = intent_type.strip_prefix("intents.").unwrap_or("unknown").to_string();

        rows.push(PlanSummaryRow {
            plan_id,
            intent_id: plan.source_intent_id.as_str().to_string(),
            intent_type: itype,
            status: plan.status,
        });
    }

    if rows.is_empty() {
        println!("No plans found.");
        return Ok(());
    }

    print_output(&rows, &["PLAN ID", "INTENT ID", "INTENT TYPE", "STATUS"],
        |r| vec![r.plan_id.clone(), r.intent_id.clone(), r.intent_type.clone(), r.status.clone()],
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Plan Show (Read-only plan inspection)
// ---------------------------------------------------------------------------

fn cmd_plan_show(conn: &Connection, plan_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![plan_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("plan '{}' not found in trace store", plan_id))?;

    let plan: agenticos_domain::TaskPlan =
        serde_json::from_str(&payload_json)
            .map_err(|e| format!("failed to deserialize plan '{}': {e}", plan_id))?;

    // Look up source intent
    let intent_type: String = conn
        .query_row(
            "SELECT topic FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![plan.source_intent_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "intents.unknown".into());
    let itype = intent_type.strip_prefix("intents.").unwrap_or("unknown");

    println!("Plan: {}", plan.id);
    println!("Source Intent: {} ({})", plan.source_intent_id, itype);
    println!("Status: {}", plan.status);
    println!("Timestamp: {}", plan.timestamp);
    println!("Steps: {}", plan.steps.len());
    println!();
    for step in &plan.steps {
        println!("  {}. {}", step.order, step.action);
        if !step.parameters.is_empty() {
            let mut keys: Vec<&String> = step.parameters.keys().collect();
            keys.sort();
            for key in keys {
                println!("     {}: {}", key, step.parameters[key]);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Execute by Intent ID (avoids plan selection errors)
// ---------------------------------------------------------------------------

fn cmd_execute_intent(conn: &Connection, intent_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Verify the intent exists
    let _payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![intent_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("intent '{}' not found in trace store", intent_id))?;

    // Step 2: Find the plan bound to this intent
    let plan_id: String = conn
        .query_row(
            "SELECT message_id FROM traces WHERE message_id IN (SELECT message_id FROM traces WHERE topic LIKE 'plans.%') AND payload_json LIKE ?1 ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![format!("%\"source_intent_id\":\"{}\"%", intent_id)],
            |row| row.get(0),
        )
        .map_err(|_| format!("no plan found for intent '{}'", intent_id))?;

    println!("Intent: {}", intent_id);
    println!("Plan: {}", plan_id);
    println!();

    // Step 3: Execute the plan
    cmd_execute(conn, &plan_id)
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Actions (Action Graph)
// ---------------------------------------------------------------------------

fn cmd_actions(conn: &Connection, plan_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    use agenticos_intelligence::{ActionGraphBuilder, StaticToolRegistry, ToolResolver};

    // Load the plan from the trace store
    let payload_json: String = conn
        .query_row(
            "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![plan_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("plan '{}' not found in trace store", plan_id))?;

    let plan: agenticos_domain::TaskPlan =
        serde_json::from_str(&payload_json)
            .map_err(|e| format!("failed to deserialize plan '{}': {e}", plan_id))?;

    // Build action graph from plan
    let registry = StaticToolRegistry::new();
    let resolver = ToolResolver::new(Box::new(registry));
    let builder = ActionGraphBuilder::new(resolver);
    let graph = builder
        .build(&plan)
        .ok_or_else(|| format!("plan '{}' has no steps — cannot build action graph", plan_id))?;

    // Store the action graph
    let action_store = agenticos_intelligence::ActionStore::new(":memory:")
        .map_err(|e| format!("cannot open action store: {e}"))?;
    action_store.insert(&graph)?;

    // Display the graph
    println!("Action Graph for Plan: {}", plan.id);
    println!("Source Intent: {}", plan.source_intent_id);
    println!("Nodes: {}  Edges: {}", graph.node_count(), graph.edge_count());
    println!();

    for node in &graph.nodes {
        let kind_str = format!("{:?}", node.kind);
        let status_str = format!("{:?}", node.status);
        let tool_str = node.metadata.tool.as_deref().unwrap_or("none");
        let cap_str = node.metadata.capability.as_deref().unwrap_or("none");
        println!("  Action {} [step {}]", node.id, node.metadata.source_step);
        println!("    kind:     {}", kind_str);
        println!("    status:   {}", status_str);
        println!("    tool:     {}", tool_str);
        println!("    cap:      {}", cap_str);

        // Show parameters
        if !node.params.is_empty() {
            println!("    params:");
            let mut keys: Vec<&String> = node.params.keys().collect();
            keys.sort();
            for key in keys {
                println!("      {}: {}", key, node.params[key]);
            }
        }

        // Show dependencies
        let prereqs: Vec<&agenticos_domain::ActionNode> = graph.prerequisites_of(&node.id);
        if !prereqs.is_empty() {
            println!("    depends_on:");
            for p in &prereqs {
                println!("      {} (step {})", p.id, p.metadata.source_step);
            }
        }

        println!();
    }

    // Show edges
    if !graph.edges.is_empty() {
        println!("  Dependency Edges:");
        for edge in &graph.edges {
            println!(
                "    {} → {} : {}",
                edge.prerequisite_id, edge.dependent_id, edge.reason
            );
        }
        println!();
    }

    // Persist the action graph to the trace store
    {
        let json = serde_json::to_string(&graph)?;
        conn.execute(
            "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                plan.id.as_str(),
                "actions-cli",
                Option::<String>::None,
                format!("actions.{}", plan.status),
                plan.timestamp,
                json,
            ],
        )?;
    }

    Ok(())
}

fn persist_intent_to_trace_store(
    conn: &Connection,
    intent: &agenticos_domain::Intent,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(intent)?;
    conn.execute(
        "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            intent.id.as_str(),
            "intent-cli",
            Option::<String>::None,
            format!("intents.{}", intent.intent_type),
            intent.timestamp,
            json,
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{
        ActionId, ActionKind, ActionRequest, ActionSafetyLevel, AgentId, Confidence, Decision,
        DecisionId, DecisionOutcome, DenialReason, EventEnvelope, EventPayload, Incident,
        IncidentCategory, IncidentSeverity, MetricCollection, MetricLabel, MetricSample,
        MetricValue, Observation, ObservationId, ObservationPayload, ObservationSource, Proposal,
        ProposalId, Recommendation, Topic, TraceEvent, TraceId,
    };

    fn create_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS traces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT NOT NULL,
                trace_id TEXT NOT NULL,
                causation_id TEXT,
                topic TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_traces_trace_id ON traces(trace_id);"
        ).unwrap();
        conn
    }

    fn insert_event(
        conn: &Connection,
        trace_id: &str,
        topic: &str,
        payload: EventPayload,
        timestamp: &str,
    ) {
        let env = EventEnvelope {
            id: agenticos_domain::MessageId::from_string("msg-1"),
            trace_id: TraceId::from(trace_id),
            causation_id: None,
            topic: Topic::new(topic),
            timestamp: timestamp.to_owned(),
            payload,
        };
        let payload_json = serde_json::to_string(&env.payload).unwrap();
        conn.execute(
            "INSERT INTO traces (message_id, trace_id, causation_id, topic, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                env.id.as_str(),
                env.trace_id.as_str(),
                env.causation_id.as_ref().map(|c| c.as_str()),
                env.topic.as_str(),
                env.timestamp,
                payload_json,
            ],
        ).unwrap();
    }

    fn insert_trace_event(conn: &Connection, trace_id: &str, topic: &str, message: &str, timestamp: &str) {
        insert_event(
            conn,
            trace_id,
            topic,
            EventPayload::Trace(TraceEvent { message: message.into() }),
            timestamp,
        );
    }

    fn insert_observation(conn: &Connection, trace_id: &str, timestamp: &str) {
        let obs = Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: timestamp.into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Empty,
        };
        insert_event(conn, trace_id, "observations.memory", EventPayload::Observation(obs), timestamp);
    }

    fn insert_proposal(conn: &Connection, trace_id: &str, agent: &str, timestamp: &str) {
        let prop = Proposal {
            id: ProposalId::new(),
            agent_id: AgentId::from(agent),
            created_at: timestamp.into(),
            based_on: vec![],
            requested_action: ActionRequest {
                id: ActionId::new(),
                kind: ActionKind::ObserveOnly,
                safety_level: ActionSafetyLevel::ReadOnly,
            },
            rationale: "test".into(),
            confidence: Confidence(1.0),
        };
        insert_event(
            conn,
            trace_id,
            &format!("proposals.{agent}"),
            EventPayload::Proposal(prop),
            timestamp,
        );
    }

    fn insert_decision(conn: &Connection, trace_id: &str, agent: &str, outcome: DecisionOutcome, timestamp: &str) {
        let dec = Decision {
            id: DecisionId::new(),
            proposal_id: ProposalId::new(),
            decided_at: timestamp.into(),
            decided_by: AgentId::from("policy-kernel"),
            outcome,
            explanation: "test decision".into(),
        };
        insert_event(
            conn,
            trace_id,
            &format!("decisions.{agent}"),
            EventPayload::Decision(dec),
            timestamp,
        );
    }

    fn insert_incident(conn: &Connection, trace_id: &str, category: IncidentCategory, severity: IncidentSeverity, source: &str, timestamp: &str) {
        let cat_str = category.description().to_owned();
        let inc = Incident::new(category, severity, AgentId::from(source), None, "test incident");
        insert_event(
            conn,
            trace_id,
            &format!("incidents.{cat_str}"),
            EventPayload::Incident(inc),
            timestamp,
        );
    }

    fn insert_veto_trace(conn: &Connection, trace_id: &str, msg: &str, timestamp: &str) {
        insert_trace_event(conn, trace_id, "vetoes.invalid-proposal", msg, timestamp);
    }

    fn insert_result(conn: &Connection, trace_id: &str, agent: &str, timestamp: &str) {
        let result = agenticos_domain::ActionResult {
            action_id: ActionId::new(),
            status: agenticos_domain::ActionStatus::Succeeded,
            message: "ok".into(),
            executed_at: timestamp.into(),
            duration_ms: 1,
            rollback: None,
        };
        insert_event(
            conn,
            trace_id,
            &format!("results.{agent}"),
            EventPayload::ActionResult(result),
            timestamp,
        );
    }

    fn insert_recommendation(conn: &Connection, trace_id: &str, agent: &str, summary: &str, confidence: f64, reasoning: &str, timestamp: &str) {
        let rec = Recommendation::new(
            AgentId::from(agent),
            confidence,
            summary,
            reasoning,
        );
        insert_event(
            conn,
            trace_id,
            &format!("recommendations.{agent}"),
            EventPayload::Recommendation(rec),
            timestamp,
        );
    }

    fn insert_metrics(conn: &Connection, trace_id: &str, veto_count: f64, timestamp: &str) {
        let metrics = MetricCollection {
            source: "daemon.service".into(),
            samples: vec![
                MetricSample {
                    name: "safety_veto_count".into(),
                    value: MetricValue::Gauge(veto_count),
                    labels: vec![MetricLabel {
                        name: "service".into(), value: "daemon".into(),
                    }],
                    timestamp: timestamp.into(),
                },
                MetricSample {
                    name: "decision_latency_ms".into(),
                    value: MetricValue::Histogram(vec![12.5]),
                    labels: vec![MetricLabel {
                        name: "service".into(), value: "daemon".into(),
                    }],
                    timestamp: timestamp.into(),
                },
            ],
        };
        insert_event(
            conn,
            trace_id,
            "metrics.daemon",
            EventPayload::Trace(TraceEvent {
                message: serde_json::to_string(&metrics).unwrap_or_default(),
            }),
            timestamp,
        );
    }

    // ---------------------------------------------------------------
    // Status
    // ---------------------------------------------------------------
    #[test]
    fn test_status_empty_db() {
        let conn = create_test_db();
        assert!(cmd_status(&conn).is_ok());
    }

    #[test]
    fn test_status_with_data() {
        let conn = create_test_db();
        insert_metrics(&conn, "tick-1", 0.0, "2026-06-02T00:00:01Z");
        insert_metrics(&conn, "tick-2", 1.0, "2026-06-02T00:00:02Z");
        insert_proposal(&conn, "tick-1", "memory-agent", "2026-06-02T00:00:01Z");
        assert!(cmd_status(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Agents
    // ---------------------------------------------------------------
    #[test]
    fn test_agents_empty_db() {
        let conn = create_test_db();
        assert!(cmd_agents(&conn).is_ok());
    }

    #[test]
    fn test_agents_with_proposals() {
        let conn = create_test_db();
        insert_proposal(&conn, "tick-1", "memory-agent", "ts1");
        insert_proposal(&conn, "tick-1", "process-agent", "ts2");
        insert_proposal(&conn, "tick-2", "memory-agent", "ts3");
        assert!(cmd_agents(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Incidents
    // ---------------------------------------------------------------
    #[test]
    fn test_incidents_empty_db() {
        let conn = create_test_db();
        assert!(cmd_incidents(&conn).is_ok());
    }

    #[test]
    fn test_incidents_with_data() {
        let conn = create_test_db();
        insert_incident(&conn, "trace-1", IncidentCategory::Security, IncidentSeverity::Warning, "security-agent", "ts1");
        insert_incident(&conn, "trace-1", IncidentCategory::GovernanceViolation, IncidentSeverity::Error, "safety-governor", "ts2");
        assert!(cmd_incidents(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Traces
    // ---------------------------------------------------------------
    #[test]
    fn test_traces_empty_db() {
        let conn = create_test_db();
        assert!(cmd_traces(&conn).is_ok());
    }

    #[test]
    fn test_traces_with_data() {
        let conn = create_test_db();
        insert_trace_event(&conn, "trace-1", "observations.memory", "obs", "ts1");
        insert_trace_event(&conn, "trace-1", "proposals.memory", "prop", "ts2");
        insert_trace_event(&conn, "trace-2", "observations.cpu", "obs", "ts3");
        assert!(cmd_traces(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Decisions
    // ---------------------------------------------------------------
    #[test]
    fn test_decisions_empty_db() {
        let conn = create_test_db();
        assert!(cmd_decisions(&conn).is_ok());
    }

    #[test]
    fn test_decisions_with_data() {
        let conn = create_test_db();
        insert_decision(&conn, "trace-1", "agent-1", DecisionOutcome::Approved, "ts1");
        insert_decision(&conn, "trace-1", "agent-1", DecisionOutcome::Denied { reason: DenialReason::UnsafeAction }, "ts2");
        insert_veto_trace(&conn, "trace-1", "veto proposal=ProposalId-1 reason=ConflictingProposals explanation=conflict", "ts3");
        assert!(cmd_decisions(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Metrics
    // ---------------------------------------------------------------
    #[test]
    fn test_metrics_empty_db() {
        let conn = create_test_db();
        assert!(cmd_metrics(&conn).is_ok());
    }

    #[test]
    fn test_metrics_with_data() {
        let conn = create_test_db();
        insert_proposal(&conn, "tick-1", "agent-1", "ts1");
        insert_incident(&conn, "tick-1", IncidentCategory::Security, IncidentSeverity::Warning, "security-agent", "ts2");
        insert_veto_trace(&conn, "tick-1", "veto", "ts3");
        insert_result(&conn, "tick-1", "agent-1", "ts4");
        insert_metrics(&conn, "tick-1", 1.0, "ts5");
        assert!(cmd_metrics(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Replay
    // ---------------------------------------------------------------
    #[test]
    fn test_replay_missing_trace() {
        let conn = create_test_db();
        assert!(cmd_replay(&conn, "nonexistent").is_ok());
    }

    #[test]
    fn test_replay_with_data() {
        let conn = create_test_db();
        insert_observation(&conn, "trace-replay-1", "ts1");
        insert_proposal(&conn, "trace-replay-1", "memory-agent", "ts2");
        insert_decision(&conn, "trace-replay-1", "memory-agent", DecisionOutcome::Approved, "ts3");
        insert_result(&conn, "trace-replay-1", "memory-agent", "ts4");
        assert!(cmd_replay(&conn, "trace-replay-1").is_ok());
    }

    // ---------------------------------------------------------------
    // Recommendations
    // ---------------------------------------------------------------
    #[test]
    fn test_recommendations_empty_db() {
        let conn = create_test_db();
        assert!(cmd_recommendations(&conn).is_ok());
    }

    #[test]
    fn test_recommendations_with_data() {
        let conn = create_test_db();
        insert_recommendation(&conn, "trace-1", "classifier", "Workload classified as Database", 0.92, "High CPU with database processes", "ts1");
        insert_recommendation(&conn, "trace-1", "classifier", "Workload classified as Build", 0.88, "Compiler processes detected", "ts2");
        assert!(cmd_recommendations(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Health
    // ---------------------------------------------------------------
    #[test]
    fn test_health_empty_db() {
        let conn = create_test_db();
        assert!(cmd_health(&conn).is_ok());
    }

    #[test]
    fn test_health_with_data() {
        let conn = create_test_db();
        // Security Agent should not produce proposals
        insert_proposal(&conn, "tick-1", "memory-agent", "ts1");
        // Has safety metrics
        insert_metrics(&conn, "tick-1", 0.0, "ts2");
        // Pipeline data
        insert_trace_event(&conn, "tick-1", "observations.memory", "obs", "ts3");
        insert_trace_event(&conn, "tick-1", "decisions.memory-agent", "dec", "ts4");
        assert!(cmd_health(&conn).is_ok());
    }

    // ---------------------------------------------------------------
    // Ask (Intent Parsing)
    // ---------------------------------------------------------------
    #[test]
    fn test_ask_with_launch_intent() {
        let conn = create_test_db();
        assert!(test_cmd_ask(&conn, "Open VS Code").is_ok());
        // Verify the intent was persisted to the trace store
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert!(
            count > 0,
            "expected at least one intent trace, got {}",
            count
        );
        // Verify intent type in the payload
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'intents.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(
            payload.contains("launch_application"),
            "expected launch_application in payload, got: {payload}"
        );
        assert!(
            payload.contains("vscode"),
            "expected vscode in payload, got: {payload}"
        );
    }

    #[test]
    fn test_ask_with_create_project_intent() {
        let conn = create_test_db();
        assert!(test_cmd_ask(&conn, "Create a Next.js project").is_ok());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert!(count > 0, "expected at least one intent trace");
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'intents.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(
            payload.contains("create_project"),
            "expected create_project in payload, got: {payload}"
        );
    }

    #[test]
    fn test_ask_no_tokio_panic_with_mock_parser() {
        // Explicitly verify no Tokio panic occurs when using MockIntentParser
        // (GeminiIntentParser would require #[tokio::test], but MockIntentParser
        // is synchronous and works in a regular #[test].)
        let conn = create_test_db();
        // Unset GEMINI_API_KEY to force MockIntentParser
        std::env::remove_var("GEMINI_API_KEY");
        let result = test_cmd_ask(&conn, "What time is it?");
        assert!(result.is_ok(), "cmd_ask should not panic: {result:?}");
    }

    // ---------------------------------------------------------------
    // Plan (intent_id-based)
    // ---------------------------------------------------------------

    // Test helpers that use in-memory DB for the subsidiary stores
    fn test_cmd_ask(conn: &Connection, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        cmd_ask(conn, ":memory:", text)
    }

    fn test_cmd_plan(conn: &Connection, intent_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        cmd_plan(conn, ":memory:", intent_id)
    }

    fn ask_and_get_id(conn: &Connection, text: &str) -> String {
        test_cmd_ask(conn, text).unwrap();
        conn.query_row(
            "SELECT message_id FROM traces WHERE topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn test_plan_with_launch_intent() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Open VS Code");
        assert!(test_cmd_plan(&conn, &intent_id).is_ok());
        // Verify plan persisted to trace store
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert!(count > 0, "expected at least one plan trace, got {count}");
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'plans.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(payload.contains("launch_application"), "expected launch_application step in payload");
    }

    #[test]
    fn test_plan_with_firefox_and_github() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Open Firefox and go to github.com");
        assert!(test_cmd_plan(&conn, &intent_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'plans.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        // Should have two steps
        assert!(payload.contains("launch_application"), "expected launch_application step");
        assert!(payload.contains("open_url"), "expected open_url step");
        assert!(payload.contains("firefox"), "expected firefox app");
        assert!(payload.contains("github.com"), "expected github url");
    }

    #[test]
    fn test_plan_with_create_project() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Create Next.js project called examgenius");
        assert!(test_cmd_plan(&conn, &intent_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'plans.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        // Should have two steps: create_directory + initialize_project
        assert!(payload.contains("create_directory"), "expected create_directory step");
        assert!(payload.contains("initialize_project"), "expected initialize_project step");
        assert!(payload.contains("examgenius"), "expected examgenius project name");
        assert!(payload.contains("nextjs"), "expected nextjs framework");
    }

    #[test]
    fn test_plan_unsupported_intent_returns_error() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "How is the weather?");
        let result = test_cmd_plan(&conn, &intent_id);
        assert!(result.is_err(), "unknown intent should produce plan error");
    }

    #[test]
    fn test_ask_intent_persisted_correctly() {
        let conn = create_test_db();
        test_cmd_ask(&conn, "Open Firefox").unwrap();
        // Read back the full payload to verify all fields
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'intents.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("source_text"));
        assert!(obj.contains_key("intent_type"));
        assert!(obj.contains_key("parameters"));
        assert!(obj.contains_key("confidence"));
        assert!(obj.contains_key("timestamp"));
        assert_eq!(
            obj.get("intent_type").and_then(|v| v.as_str()),
            Some("launch_application")
        );
        assert_eq!(
            obj.get("source_text").and_then(|v| v.as_str()),
            Some("Open Firefox")
        );
    }

    // ---------------------------------------------------------------
    // Ask → Plan Integration
    // ---------------------------------------------------------------
    #[test]
    fn test_ask_then_plan_open_vscode() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Open VS Code");
        // Verify intent was stored correctly
        let intent_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1",
                rusqlite::params![intent_id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(intent_json.contains("launch_application"), "intent should be launch_application");
        // Now plan from that intent
        assert!(test_cmd_plan(&conn, &intent_id).is_ok(), "plan should succeed");
        // Verify plan was created and stored
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(plan_count, 1, "should have exactly one plan trace");
        let plan_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'plans.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(plan_json.contains("launch_application"), "plan should contain launch_application step");
        assert!(plan_json.contains("vscode"), "plan should contain vscode");
    }

    #[test]
    fn test_ask_then_plan_firefox_and_github() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Open Firefox and go to github.com");
        // Verify the intent has both app and url parameters
        let intent_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1",
                rusqlite::params![intent_id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(intent_json.contains("launch_application"), "intent should be launch_application");
        // Now plan from that intent
        assert!(test_cmd_plan(&conn, &intent_id).is_ok(), "plan should succeed");
        // Verify plan has two steps
        let plan_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'plans.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        assert!(plan_json.contains("launch_application"), "plan should contain launch_application");
        assert!(plan_json.contains("open_url"), "plan should contain open_url");
        assert!(plan_json.contains("firefox"), "plan should reference firefox");
        assert!(plan_json.contains("github.com"), "plan should reference github.com");
        // Verify two steps
        let plan: agenticos_domain::TaskPlan = serde_json::from_str(&plan_json).unwrap();
        assert_eq!(plan.steps.len(), 2, "should be exactly 2 steps");
        assert_eq!(plan.steps[0].action, "launch_application");
        assert_eq!(plan.steps[1].action, "open_url");
    }

    // ---------------------------------------------------------------
    // Actions (Action Graph)
    // ---------------------------------------------------------------

    fn ask_and_plan(conn: &Connection, text: &str) -> String {
        let intent_id = ask_and_get_id(conn, text);
        test_cmd_plan(conn, &intent_id).unwrap();
        conn.query_row(
            "SELECT message_id FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn test_actions_shows_graph_for_launch_plan() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify action graph persisted to trace store
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'actions.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert!(count > 0, "expected at least one action trace, got {count}");
    }

    #[test]
    fn test_actions_with_two_step_plan() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify action graph has 2 nodes
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.node_count(), 2, "expected 2 action nodes");
        assert_eq!(graph.edge_count(), 1, "expected 1 dependency edge");
    }

    #[test]
    fn test_actions_with_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.node_count(), 2);
        // First node should be CreateDirectory
        match &graph.nodes[0].kind {
            agenticos_domain::ActionKind::CreateDirectory { path } => {
                assert_eq!(path, "examgenius");
            }
            other => panic!("expected CreateDirectory, got {:?}", other),
        }
        // Second node should be CreateProjectWorkspace
        match &graph.nodes[1].kind {
            agenticos_domain::ActionKind::CreateProjectWorkspace {
                project_name,
                framework,
            } => {
                assert_eq!(project_name, "examgenius");
                assert_eq!(framework, "nextjs");
            }
            other => panic!("expected CreateProjectWorkspace, got {:?}", other),
        }
    }

    #[test]
    fn test_actions_nonexistent_plan_returns_error() {
        let conn = create_test_db();
        let result = cmd_actions(&conn, "PlanId-999");
        assert!(result.is_err(), "should error for missing plan");
    }

    #[test]
    fn test_actions_asks_plan_actions_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Check the full chain: intent → plan → action graph
        let action_payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&action_payload).unwrap();
        assert_eq!(graph.source_plan_id.as_str(), plan_id);
        // Verify node kinds
        match &graph.nodes[0].kind {
            agenticos_domain::ActionKind::LaunchApplication { application } => {
                assert_eq!(application, "firefox");
            }
            other => panic!("expected LaunchApplication, got {:?}", other),
        }
        match &graph.nodes[1].kind {
            agenticos_domain::ActionKind::OpenUrl { url } => {
                assert_eq!(url, "https://github.com");
            }
            other => panic!("expected OpenUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_actions_ask_then_plan_then_actions_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let action_payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&action_payload).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edges.len(), 1);
        // Verify tool resolution in metadata
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
    }

    #[test]
    fn test_actions_actions_payload_is_valid_action_graph() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        cmd_actions(&conn, &plan_id).unwrap();
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Each node must have required fields
        for node in &graph.nodes {
            assert!(!node.id.as_str().is_empty(), "node id must not be empty");
            assert!(
                node.metadata.source_step > 0,
                "source_step must be > 0"
            );
        }
    }

    #[test]
    fn test_actions_then_plan_actions_integration() {
        let conn = create_test_db();
        // Open VS Code → plan → actions
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify action graph nodes have correct tool resolution
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // VS Code should resolve to tool "vscode" via StaticToolRegistry
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("vscode"));
    }

    #[test]
    fn test_actions_then_plan_two_step_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Firefox → tool "firefox", open_url → tool "browser"
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("firefox"));
        assert_eq!(graph.nodes[1].metadata.tool.as_deref(), Some("browser"));
        // Dependency edge should exist
        assert_eq!(graph.edges[0].prerequisite_id, graph.nodes[0].id);
        assert_eq!(graph.edges[0].dependent_id, graph.nodes[1].id);
    }

    #[test]
    fn test_actions_then_plan_create_project_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // create_directory → tool "filesystem", initialize_project → tool (none for project workspace)
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
        // verify params are preserved through the pipeline
        assert_eq!(
            graph.nodes[0].params.get("path").unwrap(),
            "examgenius"
        );
        assert_eq!(
            graph.nodes[1].params.get("project_name").unwrap(),
            "examgenius"
        );
        assert_eq!(
            graph.nodes[1].params.get("framework").unwrap(),
            "nextjs"
        );
    }

    #[test]
    fn test_actions_then_plan_invalid_plan_id() {
        let conn = create_test_db();
        let result = cmd_actions(&conn, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_actions_then_plan_actions_empty_plan() {
        let conn = create_test_db();
        // First create an intent
        let intent_id = ask_and_get_id(&conn, "How is the weather?");
        // This will fail because "unknown" intent type is not supported by MockPlannerAgent
        let plan_result = test_cmd_plan(&conn, &intent_id);
        assert!(plan_result.is_err(), "unknown intent should fail planning");
        // actions on a nonexistent plan should fail too
        let result = cmd_actions(&conn, "PlanId-999");
        assert!(result.is_err());
    }

    #[test]
    fn test_actions_then_plan_then_actions_then_plan() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Second plan + actions
        let plan_id2 = ask_and_plan(&conn, "Open Firefox");
        assert!(cmd_actions(&conn, &plan_id2).is_ok());
        // Both action graphs should be stored
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'actions.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 2, "expected exactly 2 action traces");
    }

    #[test]
    fn test_actions_then_plan_actions_integration_full_pipeline() {
        let conn = create_test_db();
        // Full pipeline: ask → plan → actions
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify all three traces exist
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let action_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'actions.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "expected 1 intent trace");
        assert_eq!(plan_count, 1, "expected 1 plan trace");
        assert_eq!(action_count, 1, "expected 1 action trace");
    }

    #[test]
    fn test_actions_create_project_then_plan_integration_full() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Validate all 3 traces
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let action_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'actions.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent count");
        assert_eq!(plan_count, 1, "plan count");
        assert_eq!(action_count, 1, "action count");
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
    }

    #[test]
    fn test_actions_invalid_plan_id_behavior() {
        let conn = create_test_db();
        // Empty plan ID
        let result = cmd_actions(&conn, "");
        assert!(result.is_err(), "empty plan_id should error");
        // Malformed plan ID format (but it's just a string, so it won't error on format)
        let result = cmd_actions(&conn, "not-a-plan-id");
        assert!(result.is_err(), "nonexistent plan should error");
    }

    #[test]
    fn test_actions_invalid_plan_id_format() {
        let conn = create_test_db();
        // PlanId with special characters
        let result = cmd_actions(&conn, "PlanId-!!!");
        assert!(result.is_err(), "nonexistent plan should error");
    }

    #[test]
    fn test_actions_invalid_plan_id_empty() {
        let conn = create_test_db();
        let result = cmd_actions(&conn, "");
        assert!(result.is_err());
    }

    #[test]
    fn test_actions_invalid_plan_id_special_chars() {
        let conn = create_test_db();
        let result = cmd_actions(&conn, "PlanId-@#$%");
        assert!(result.is_err());
    }

    #[test]
    fn test_actions_pipeline_intent_plan_actions() {
        let conn = create_test_db();
        // Full pipeline
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify action graph fields
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Node should have LaunchApplication kind
        assert!(matches!(
            &graph.nodes[0].kind,
            agenticos_domain::ActionKind::LaunchApplication { application }
                if application == "vscode"
        ));
        // Metadata should reference plan and intent
        assert_eq!(graph.source_plan_id.as_str(), plan_id);
        assert!(graph.source_intent_id.as_str().starts_with("IntentId-"));
    }

    #[test]
    fn test_actions_pipeline_intent_plan_actions_two_step_dependency() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Dependency edge: ActionId-1 (launch) must complete before ActionId-2 (open_url)
        assert_eq!(graph.nodes[0].metadata.source_step, 1);
        assert_eq!(graph.nodes[1].metadata.source_step, 2);
        assert_eq!(graph.edges[0].prerequisite_id, graph.nodes[0].id);
        assert_eq!(graph.edges[0].dependent_id, graph.nodes[1].id);
        assert!(graph.edges[0].reason.contains("must complete before"));
    }

    #[test]
    fn test_actions_actions_graph_metadata_fields() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Check metadata fields
        let meta = &graph.nodes[0].metadata;
        assert_eq!(meta.source_step, 1);
        assert_eq!(meta.source_plan_id.as_str(), plan_id);
        assert!(!meta.source_intent_id.as_str().is_empty());
        assert_eq!(meta.tool.as_deref(), Some("vscode"));
        assert_eq!(meta.capability.as_deref(), Some("launch_application"));
    }

    #[test]
    fn test_actions_actions_graph_create_project_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Verify all node metadata for create project
        assert_eq!(graph.nodes[0].metadata.source_step, 1);
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
        assert_eq!(
            graph.nodes[0].metadata.capability.as_deref(),
            Some("create_directory")
        );
        assert_eq!(graph.nodes[1].metadata.source_step, 2);
        assert_eq!(graph.nodes[1].metadata.capability.as_deref(), Some("initialize_project"));
    }

    #[test]
    fn test_actions_actions_graph_then_plan_actions_integration_complete() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Validate full metadata
        for node in &graph.nodes {
            assert!(node.metadata.source_step >= 1);
            assert_eq!(node.metadata.source_plan_id.as_str(), plan_id);
            assert!(node.metadata.source_intent_id.as_str().starts_with("IntentId-"));
        }
        // Validate edges
        for edge in &graph.edges {
            assert!(!edge.prerequisite_id.as_str().is_empty());
            assert!(!edge.dependent_id.as_str().is_empty());
            assert!(!edge.reason.is_empty());
        }
    }

    #[test]
    fn test_actions_actions_graph_then_plan_integration_full() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.source_plan_id.as_str(), plan_id);
        assert_eq!(graph.node_count(), 1, "VS Code plan should have 1 action");
        // Verify content
        match &graph.nodes[0].kind {
            agenticos_domain::ActionKind::LaunchApplication { application } => {
                assert_eq!(application, "vscode");
            }
            other => panic!("expected LaunchApplication, got {:?}", other),
        }
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("vscode"));
    }

    #[test]
    fn test_actions_actions_graph_empty_plan_id_then_plan_actions() {
        let conn = create_test_db();
        // Empty plan ID
        let result = cmd_actions(&conn, "");
        assert!(result.is_err());
    }

    #[test]
    fn test_actions_actions_graph_plan_id_format_then_plan_actions() {
        let conn = create_test_db();
        let result = cmd_actions(&conn, "PlanId-abc");
        assert!(result.is_err(), "nonexistent plan should error");
    }

    #[test]
    fn test_actions_actions_graph_then_plan_actions_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Check dependency chain
        assert_eq!(graph.nodes[0].metadata.source_step, 1);
        assert_eq!(graph.nodes[1].metadata.source_step, 2);
        assert_eq!(graph.edges[0].prerequisite_id, graph.nodes[0].id);
        assert_eq!(graph.edges[0].dependent_id, graph.nodes[1].id);
    }

    #[test]
    fn test_actions_actions_graph_then_plan_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        // Check edge is absent (single node)
        assert_eq!(graph.edge_count(), 0);
        // Check tool is correctly resolved
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("vscode"));
    }

    #[test]
    fn test_actions_create_project_then_plan_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
    }

    #[test]
    fn test_actions_create_project_then_plan_actions_integration_full() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'actions.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("filesystem"));
        assert_eq!(graph.nodes[0].params.get("path").unwrap(), "examgenius");
        assert_eq!(graph.nodes[1].params.get("project_name").unwrap(), "examgenius");
        assert_eq!(graph.nodes[1].params.get("framework").unwrap(), "nextjs");
    }

    #[test]
    fn test_actions_actions_graph_then_plan_actions_integration_complete_full_pipeline() {
        let conn = create_test_db();
        // Full pipeline: ask → plan → actions
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_actions(&conn, &plan_id).is_ok());
        // Verify all 3 traces
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let action_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'actions.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent");
        assert_eq!(plan_count, 1, "plan");
        assert_eq!(action_count, 1, "action");
    }

    // ---------------------------------------------------------------
    // Execute (Full Governance Pipeline)
    // ---------------------------------------------------------------

    #[test]
    fn test_execute_single_step_launch() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify execution trace was persisted
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1, "expected 1 execute trace");
    }

    #[test]
    fn test_execute_two_step_plan() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Both steps should be executed
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1);
        // Verify the execute trace payload contains action graph
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE topic LIKE 'execute.%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let graph: agenticos_domain::ActionGraph =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn test_execute_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_execute_nonexistent_plan_returns_error() {
        let conn = create_test_db();
        let result = cmd_execute(&conn, "PlanId-999");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_full_pipeline_persists_trace() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify all traces: intent, plan, action, execute
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent");
        assert_eq!(plan_count, 1, "plan");
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_then_ask_then_plan_then_execute() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Second plan + execute
        let plan_id2 = ask_and_plan(&conn, "Open Firefox");
        assert!(cmd_execute(&conn, &plan_id2).is_ok());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 2, "expected 2 execute traces");
    }

    #[test]
    fn test_execute_then_plan_then_actions_integration() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify the pipeline ran successfully
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_execute_then_plan_then_actions_integration_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify the pipeline ran successfully
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_execute_then_plan_then_actions_integration_single_step() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_execute_with_unsupported_intent() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "How is the weather?");
        // Plan will fail for unsupported intent
        let plan_result = test_cmd_plan(&conn, &intent_id);
        assert!(plan_result.is_err(), "unknown intent should fail planning");
        // Execute on nonexistent plan should fail
        let result = cmd_execute(&conn, "PlanId-999");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_empty_plan_id() {
        let conn = create_test_db();
        let result = cmd_execute(&conn, "");
        assert!(result.is_err(), "empty plan_id should error");
    }

    #[test]
    fn test_execute_invalid_plan_id() {
        let conn = create_test_db();
        let result = cmd_execute(&conn, "PlanId-!!!");
        assert!(result.is_err(), "nonexistent plan should error");
    }

    #[test]
    fn test_execute_full_integration_pipeline() {
        let conn = create_test_db();
        // Full pipeline: ask → plan → execute
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify all traces
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent");
        assert_eq!(plan_count, 1, "plan");
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_full_integration_pipeline_two_steps() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_full_integration_pipeline_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_pipeline_intent_plan_execute() {
        let conn = create_test_db();
        // Full pipeline: ask → plan → execute
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify all three traces
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent");
        assert_eq!(plan_count, 1, "plan");
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_pipeline_intent_plan_execute_two_steps() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open Firefox and go to github.com");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_pipeline_intent_plan_execute_create_project() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Create Next.js project called examgenius");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(execute_count, 1, "execute");
    }

    #[test]
    fn test_execute_pipeline_full_integration() {
        let conn = create_test_db();
        // Full pipeline: ask → plan → execute
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        // Verify all traces
        let intent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'intents.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let plan_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'plans.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let execute_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic LIKE 'execute.%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(intent_count, 1, "intent");
        assert_eq!(plan_count, 1, "plan");
        assert_eq!(execute_count, 1, "execute");
    }

    // ---------------------------------------------------------------
    // Governance Validation Tests (Task 4 — Critical Governance Fix)
    // ---------------------------------------------------------------

    /// Helper: count executor calls by checking `executor.called` traces.
    fn get_executor_call_count(conn: &Connection) -> u64 {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE topic = 'executor.called'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        count as u64
    }

    /// Check the plan's action kind from the trace store.
    fn get_plan_action_kind(conn: &Connection, plan_id: &str) -> String {
        let payload_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![plan_id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        if payload_json.is_empty() {
            return "unknown".into();
        }
        let plan: agenticos_domain::TaskPlan = serde_json::from_str(&payload_json).unwrap_or_else(|_| {
            agenticos_domain::TaskPlan::new(
                agenticos_domain::IntentId::new(),
                vec![],
                "error",
            )
        });
        if plan.steps.is_empty() {
            return "no_steps".into();
        }
        plan.steps[0].action.clone()
    }

    // Scenario A: "Run cargo build" — ALLOW through full pipeline
    #[test]
    fn test_governance_cargo_build_allowed() {
        let conn = create_test_db();
        // Step 1: Ask
        test_cmd_ask(&conn, "Run cargo build").unwrap();
        let intent_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();

        // Verify intent type
        let payload_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![intent_id],
                |row| row.get(0),
            )
            .unwrap();
        let intent: agenticos_domain::Intent = serde_json::from_str(&payload_json).unwrap();
        assert_eq!(intent.intent_type, "run_command");
        assert!(intent.parameters.get("command").unwrap().contains("cargo build"));

        // Step 2: Plan
        test_cmd_plan(&conn, &intent_id).unwrap();
        let plan_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let action = get_plan_action_kind(&conn, &plan_id);
        assert_eq!(action, "run_command");

        // Step 3: Execute
        let before = get_executor_call_count(&conn);
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let after = get_executor_call_count(&conn);

        // Verify: executor was called (noop)
        assert_eq!(after - before, 1, "cargo build: executor should be called once");
    }

    // Scenario B: "Run rm -rf /" — DENY or VETO
    #[test]
    fn test_governance_rm_denied() {
        let conn = create_test_db();
        test_cmd_ask(&conn, "Run rm -rf /").unwrap();
        let intent_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();

        let payload_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![intent_id],
                |row| row.get(0),
            )
            .unwrap();
        let intent: agenticos_domain::Intent = serde_json::from_str(&payload_json).unwrap();
        assert_eq!(intent.intent_type, "run_command");
        assert!(intent.parameters.get("command").unwrap().contains("rm -rf /"));

        test_cmd_plan(&conn, &intent_id).unwrap();
        let plan_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let action = get_plan_action_kind(&conn, &plan_id);
        assert_eq!(action, "run_command");

        let before = get_executor_call_count(&conn);
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let after = get_executor_call_count(&conn);

        // Verify: executor was NEVER called
        assert_eq!(after - before, 0, "rm -rf /: executor must NOT be called");
    }

    // Scenario C: "Run shutdown now" — DENY or VETO
    #[test]
    fn test_governance_shutdown_denied() {
        let conn = create_test_db();
        test_cmd_ask(&conn, "Run shutdown now").unwrap();
        let intent_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();

        let payload_json: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1 AND topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![intent_id],
                |row| row.get(0),
            )
            .unwrap();
        let intent: agenticos_domain::Intent = serde_json::from_str(&payload_json).unwrap();
        assert_eq!(intent.intent_type, "run_command");
        assert!(intent.parameters.get("command").unwrap().contains("shutdown"));

        test_cmd_plan(&conn, &intent_id).unwrap();
        let plan_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();

        let before = get_executor_call_count(&conn);
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let after = get_executor_call_count(&conn);

        // Verify: executor was NEVER called
        assert_eq!(after - before, 0, "shutdown: executor must NOT be called");
    }

    // Test: Plans command works
    #[test]
    fn test_plans_command_empty_db() {
        let conn = create_test_db();
        assert!(cmd_plans(&conn).is_ok());
    }

    #[test]
    fn test_plans_command_with_data() {
        let conn = create_test_db();
        let _plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_plans(&conn).is_ok());
    }

    // Test: Plan-show command works
    #[test]
    fn test_plan_show_existing() {
        let conn = create_test_db();
        let plan_id = ask_and_plan(&conn, "Open VS Code");
        assert!(cmd_plan_show(&conn, &plan_id).is_ok());
    }

    #[test]
    fn test_plan_show_nonexistent() {
        let conn = create_test_db();
        assert!(cmd_plan_show(&conn, "PlanId-999").is_err());
    }

    // Test: Execute-intent command works
    #[test]
    fn test_execute_intent_existing() {
        let conn = create_test_db();
        test_cmd_ask(&conn, "Open VS Code").unwrap();
        let intent_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'intents.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        test_cmd_plan(&conn, &intent_id).unwrap();
        assert!(cmd_execute_intent(&conn, &intent_id).is_ok());
    }

    #[test]
    fn test_execute_intent_nonexistent() {
        let conn = create_test_db();
        assert!(cmd_execute_intent(&conn, "IntentId-999").is_err());
    }

    // Test: Governance pipeline — verify execution guard assertion
    #[test]
    fn test_execution_guard_rm_not_executed() {
        let conn = create_test_db();
        test_cmd_ask(&conn, "Run rm -rf /").unwrap();
        let intent_id = ask_and_get_id(&conn, "Run rm -rf /");
        test_cmd_plan(&conn, &intent_id).unwrap();
        let plan_id = conn
            .query_row(
                "SELECT message_id FROM traces WHERE topic LIKE 'plans.%' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();

        let before = get_executor_call_count(&conn);
        assert!(cmd_execute(&conn, &plan_id).is_ok());
        let after = get_executor_call_count(&conn);

        // Execution guard: executor call count must be 0
        assert_eq!(after - before, 0, "rm -rf /: executor must NOT be called");

        // Verify the pipeline did run (execute trace exists)
        let execute_traces: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE message_id = ?1 AND topic LIKE 'execute.%'",
                rusqlite::params![plan_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(execute_traces, 1, "execute trace should exist (pipeline ran)");
    }

    // ---------------------------------------------------------------
    // Persistence tests — verify IDs survive process restarts
    // ---------------------------------------------------------------

    #[test]
    fn test_intent_ids_persist_across_calls() {
        // Simulate persistent storage across CLI invocations using a temp file
        let tmp = std::env::temp_dir().join(format!("test_intents_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp); // clean start

        // First CLI invocation
        {
            let store = agenticos_intelligence::IntentStore::new(tmp.to_str().unwrap()).unwrap();
            let params = {
                let mut m = std::collections::HashMap::new();
                m.insert("application".into(), "vscode".into());
                m
            };
            let intent1 = agenticos_domain::Intent::new("Open VS Code", "launch_application", params.clone(), 0.9);
            let mut intent1 = intent1;
            intent1.id = store.generate_id();
            assert_eq!(intent1.id.as_str(), "IntentId-1");
            store.insert(&intent1).unwrap();

            let intent2 = agenticos_domain::Intent::new("Open Firefox", "launch_application", params.clone(), 0.85);
            let mut intent2 = intent2;
            intent2.id = store.generate_id();
            assert_eq!(intent2.id.as_str(), "IntentId-2");
            store.insert(&intent2).unwrap();
        }

        // Simulate second CLI invocation (new process)
        {
            let store = agenticos_intelligence::IntentStore::new(tmp.to_str().unwrap()).unwrap();
            assert_eq!(store.len().unwrap(), 2, "persisted intents survive restart");

            let params = {
                let mut m = std::collections::HashMap::new();
                m.insert("application".into(), "code".into());
                m
            };
            let intent3 = agenticos_domain::Intent::new("Open Code", "launch_application", params, 0.9);
            let mut intent3 = intent3;
            intent3.id = store.generate_id();
            // ID continues from where we left off
            assert_eq!(intent3.id.as_str(), "IntentId-3", "IDs must persist across restarts");
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_plan_ids_persist_across_calls() {
        let tmp = std::env::temp_dir().join(format!("test_plans_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        // First CLI invocation
        {
            let store = agenticos_intelligence::PlanStore::new(tmp.to_str().unwrap()).unwrap();
            let intent_id = agenticos_domain::IntentId::new();
            let step1 = agenticos_domain::PlanStep::new(
                1,
                "launch_application",
                {
                    let mut m = std::collections::HashMap::new();
                    m.insert("application".into(), "vscode".into());
                    m
                },
            );
            let plan1 = agenticos_domain::TaskPlan::new(intent_id.clone(), vec![step1.clone()], "pending");
            let mut plan1 = plan1;
            plan1.id = store.generate_id();
            assert_eq!(plan1.id.as_str(), "PlanId-1");
            store.insert(&plan1).unwrap();

            let plan2 = agenticos_domain::TaskPlan::new(intent_id.clone(), vec![step1], "pending");
            let mut plan2 = plan2;
            plan2.id = store.generate_id();
            assert_eq!(plan2.id.as_str(), "PlanId-2");
            store.insert(&plan2).unwrap();
        }

        // Second CLI invocation
        {
            let store = agenticos_intelligence::PlanStore::new(tmp.to_str().unwrap()).unwrap();
            let plan3 = agenticos_domain::TaskPlan::new(
                agenticos_domain::IntentId::new(),
                vec![agenticos_domain::PlanStep::new(1, "test", std::collections::HashMap::new())],
                "pending",
            );
            let mut plan3 = plan3;
            plan3.id = store.generate_id();
            assert_eq!(plan3.id.as_str(), "PlanId-3", "Plan IDs must persist across restarts");
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_intent_plan_referential_integrity() {
        let conn = create_test_db();
        let intent_id = ask_and_get_id(&conn, "Open VS Code");
        let plan_id = ask_and_plan(&conn, "Open VS Code");

        // Verify plan's source_intent_id matches the intent
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM traces WHERE message_id = ?1",
                rusqlite::params![plan_id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let plan: agenticos_domain::TaskPlan = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            plan.source_intent_id.as_str(),
            intent_id,
            "plan must reference the correct intent ID"
        );
    }
}

