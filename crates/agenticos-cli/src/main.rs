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
    #[arg(long)]
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
}

fn main() {
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

    let mut rows = vec![
        MetricsRow { metric: "proposal_count".into(), value: proposal_count.to_string() },
        MetricsRow { metric: "incident_count".into(), value: incident_count.to_string() },
        MetricsRow { metric: "veto_count".into(), value: veto_count.to_string() },
        MetricsRow { metric: "approved_count".into(), value: approved_count.to_string() },
        MetricsRow { metric: "denied_count".into(), value: denied_count.to_string() },
        MetricsRow { metric: "executor_count".into(), value: result_count.to_string() },
        MetricsRow { metric: "decision_latency".into(), value: decision_latency },
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
            Some(RecommendationRow {
                timestamp,
                agent,
                classification,
                confidence,
                summary,
            })
        })
        .collect();

    if rows.is_empty() {
        println!("No recommendations found.");
        return Ok(());
    }

    print_output(&rows, &["TIMESTAMP", "AGENT", "CLASSIFICATION", "CONFIDENCE", "SUMMARY"],
        |r| vec![
            r.timestamp.clone(),
            r.agent.clone(),
            r.classification.clone(),
            format!("{:.2}", r.confidence),
            r.summary.clone(),
        ],
    );

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
}

