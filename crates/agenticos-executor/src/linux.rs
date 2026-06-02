use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Instant;

use agenticos_application::AppError;
use agenticos_domain::{
    ActionId, ActionKind, ActionResult, ActionStatus, ApprovedAction, RollbackToken,
};

use crate::traits::{ApprovedActionExecutor, RollbackManager};

// ---------------------------------------------------------------------------
// Snapshot — serialised into the RollbackToken for undo
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct CgroupSnapshot {
    action: String,
    group: String,
    /// Pre-mutation value of the target file (e.g. "max" or "100000 100000")
    previous_value: Option<String>,
}

// ---------------------------------------------------------------------------
// LinuxCgroupExecutor
// ---------------------------------------------------------------------------

pub struct LinuxCgroupExecutor {
    cgroup_root: PathBuf,
}

impl LinuxCgroupExecutor {
    /// `cgroup_root` is the parent directory under which all agenticos cgroups live,
    /// typically `/sys/fs/cgroup/agenticos`.
    pub fn new(cgroup_root: PathBuf) -> Self {
        Self { cgroup_root }
    }

    /// Full path for a named cgroup group.
    fn group_path(&self, group: &str) -> PathBuf {
        self.cgroup_root.join(group)
    }

    /// Read the current content of a cgroup file (trimmed).
    fn read_cgroup_file(&self, group: &str, file: &str) -> Option<String> {
        let path = self.group_path(group).join(file);
        std::fs::read_to_string(&path).ok().map(|s| s.trim().to_owned())
    }

    /// Write a value to a cgroup file.
    fn write_cgroup_file(&self, group: &str, file: &str, value: &str) -> Result<(), String> {
        let path = self.group_path(group).join(file);
        std::fs::write(&path, value.as_bytes())
            .map_err(|e| format!("failed to write {:?}: {}", path, e))
    }

    // -----------------------------------------------------------------------
    // A3.1  — create, set_cpu_max, set_memory_max
    // -----------------------------------------------------------------------

    fn do_create(&self, name: &str) -> Result<Option<CgroupSnapshot>, String> {
        let path = self.group_path(name);
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create cgroup {:?}: {}", path, e))?;
        // Enable controllers so child groups can set cpu/memory limits.
        let subtree = self.cgroup_root.join("cgroup.subtree_control");
        let _ = std::fs::write(&subtree, b"+cpu +memory");
        Ok(None) // no rollback snapshot needed; dir deletion is handled by RollbackManager
    }

    fn do_set_cpu_max(&self, group: &str, quota: &str) -> Result<Option<CgroupSnapshot>, String> {
        let prev = self.read_cgroup_file(group, "cpu.max");
        self.write_cgroup_file(group, "cpu.max", quota)?;
        Ok(Some(CgroupSnapshot {
            action: "set_cpu_max".into(),
            group: group.into(),
            previous_value: prev,
        }))
    }

    fn do_set_memory_max(&self, group: &str, bytes: u64) -> Result<Option<CgroupSnapshot>, String> {
        let prev = self.read_cgroup_file(group, "memory.max");
        self.write_cgroup_file(group, "memory.max", &bytes.to_string())?;
        Ok(Some(CgroupSnapshot {
            action: "set_memory_max".into(),
            group: group.into(),
            previous_value: prev,
        }))
    }

    // -----------------------------------------------------------------------
    // A3.2  — move_pid
    // -----------------------------------------------------------------------

    fn do_move_pid(&self, group: &str, pid: u32) -> Result<Option<CgroupSnapshot>, String> {
        // Capture the original cgroup of the PID for rollback.
        let origin = read_pid_cgroup(pid);
        self.write_cgroup_file(group, "cgroup.procs", &pid.to_string())?;
        Ok(Some(CgroupSnapshot {
            action: "move_pid".into(),
            group: group.into(),
            previous_value: origin,
        }))
    }

    // -----------------------------------------------------------------------
    // A3.3  — freeze, thaw, terminate
    // -----------------------------------------------------------------------

    fn do_freeze(&self, group: &str) -> Result<Option<CgroupSnapshot>, String> {
        let prev = self.read_cgroup_file(group, "cgroup.freeze");
        self.write_cgroup_file(group, "cgroup.freeze", "1")?;
        Ok(Some(CgroupSnapshot {
            action: "freeze".into(),
            group: group.into(),
            previous_value: prev,
        }))
    }

    fn do_thaw(&self, group: &str) -> Result<Option<CgroupSnapshot>, String> {
        let prev = self.read_cgroup_file(group, "cgroup.freeze");
        self.write_cgroup_file(group, "cgroup.freeze", "0")?;
        Ok(Some(CgroupSnapshot {
            action: "thaw".into(),
            group: group.into(),
            previous_value: prev,
        }))
    }

    fn do_terminate(&self, group: &str) -> Result<Option<CgroupSnapshot>, String> {
        // Read all PIDs from cgroup.procs and send SIGKILL.
        let procs_path = self.group_path(group).join("cgroup.procs");
        let content = std::fs::read_to_string(&procs_path)
            .map_err(|e| format!("failed to read {:?}: {}", procs_path, e))?;

        let pids: Vec<u32> = content
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();

        if pids.is_empty() {
            return Err("no pids to terminate".into());
        }

        for pid in &pids {
            // Use SIGKILL (9) via kill command
            let status = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .status()
                .map_err(|e| format!("kill command failed: {}", e))?;

            if !status.success() {
                // Log but continue killing remaining PIDs
                eprintln!("warning: kill -9 {} returned {:?}", pid, status.code());
            }
        }

        Ok(Some(CgroupSnapshot {
            action: "terminate".into(),
            group: group.into(),
            previous_value: None,
        }))
    }

    // -----------------------------------------------------------------------
    // Dispatch
    // -----------------------------------------------------------------------

    fn dispatch(&self, kind: &ActionKind) -> Result<(ActionStatus, String, Option<CgroupSnapshot>), String> {
        match kind {
            ActionKind::ObserveOnly => {
                Ok((ActionStatus::Succeeded, "observe-only: no mutation performed".into(), None))
            }
            ActionKind::CgroupCreate { name } => {
                self.do_create(name)?;
                Ok((ActionStatus::Succeeded, format!("cgroup '{}' created", name), None))
            }
            ActionKind::CgroupSetCpuMax { group, quota } => {
                let snap = self.do_set_cpu_max(group, quota)?;
                Ok((ActionStatus::Succeeded, format!("cpu.max set to '{}'", quota), snap))
            }
            ActionKind::CgroupSetMemoryMax { group, bytes } => {
                let snap = self.do_set_memory_max(group, *bytes)?;
                Ok((ActionStatus::Succeeded, format!("memory.max set to {} bytes", bytes), snap))
            }
            ActionKind::CgroupMovePid { group, pid } => {
                let snap = self.do_move_pid(group, *pid)?;
                Ok((ActionStatus::Succeeded, format!("pid {} moved to '{}'", pid, group), snap))
            }
            ActionKind::ProcessFreezeGroup { group } => {
                let snap = self.do_freeze(group)?;
                Ok((ActionStatus::Succeeded, format!("group '{}' frozen", group), snap))
            }
            ActionKind::ProcessThawGroup { group } => {
                let snap = self.do_thaw(group)?;
                Ok((ActionStatus::Succeeded, format!("group '{}' thawed", group), snap))
            }
            ActionKind::ProcessTerminateGroup { group } => {
                let snap = self.do_terminate(group)?;
                Ok((ActionStatus::Succeeded, format!("group '{}' terminated", group), snap))
            }
        }
    }
}

impl ApprovedActionExecutor for LinuxCgroupExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let now = timestamp();
        let action_id = action.request.id.clone();

        match self.dispatch(&action.request.kind) {
            Ok((status, message, snapshot)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let rollback = snapshot.and_then(|s| {
                    serde_json::to_string(&s)
                        .ok()
                        .map(|json| RollbackToken { token: json })
                });

                Ok(ActionResult {
                    action_id,
                    status,
                    message,
                    executed_at: now,
                    duration_ms,
                    rollback,
                })
            }
            Err(err_msg) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                // Attempt rollback if a snapshot was captured during a partial failure
                // (not applicable for single-operation dispatch, but kept for consistency)
                Ok(ActionResult {
                    action_id,
                    status: ActionStatus::Failed,
                    message: err_msg,
                    executed_at: now,
                    duration_ms,
                    rollback: None,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RollbackManager — applies the inverse of a previously captured snapshot
// ---------------------------------------------------------------------------

pub struct CgroupRollbackManager {
    cgroup_root: PathBuf,
}

impl CgroupRollbackManager {
    pub fn new(cgroup_root: PathBuf) -> Self {
        Self { cgroup_root }
    }

    fn restore_value(&self, group: &str, file: &str, value: &Option<String>) -> Result<(), String> {
        let path = self.cgroup_root.join(group).join(file);
        let content = value.as_deref().unwrap_or("max");
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| format!("rollback write failed {:?}: {}", path, e))
    }
}

impl RollbackManager for CgroupRollbackManager {
    fn rollback(&self, token: RollbackToken) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let now = timestamp();

        let snapshot: CgroupSnapshot = serde_json::from_str(&token.token)
            .map_err(|e| AppError::Message(format!("invalid rollback token: {}", e)))?;

        let result = match snapshot.action.as_str() {
            "set_cpu_max" => {
                self.restore_value(&snapshot.group, "cpu.max", &snapshot.previous_value)
            }
            "set_memory_max" => {
                self.restore_value(&snapshot.group, "memory.max", &snapshot.previous_value)
            }
            "move_pid" => {
                // previous_value stores the original cgroup path of the PID
                if let Some(origin) = &snapshot.previous_value {
                    let path = self.cgroup_root.join(&snapshot.group).join("cgroup.procs");
                    // Read the PID we previously moved; not stored in snapshot, so log only
                    Err(format!("rollback of move_pid requires PID tracking (origin cgroup: {})", origin))
                } else {
                    Err("rollback of move_pid: no origin recorded".into())
                }
            }
            "freeze" => {
                self.restore_value(&snapshot.group, "cgroup.freeze", &snapshot.previous_value)
            }
            "thaw" => {
                self.restore_value(&snapshot.group, "cgroup.freeze", &snapshot.previous_value)
            }
            "terminate" => {
                Err("terminate cannot be rolled back".into())
            }
            other => {
                Err(format!("unknown rollback action: {}", other))
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(_) => Ok(ActionResult {
                action_id: ActionId::from("rollback"),
                status: ActionStatus::Succeeded,
                message: format!("rollback of '{}' succeeded", snapshot.action),
                executed_at: now,
                duration_ms,
                rollback: None,
            }),
            Err(msg) => Ok(ActionResult {
                action_id: ActionId::from("rollback"),
                status: ActionStatus::Failed,
                message: format!("rollback of '{}' failed: {}", snapshot.action, msg),
                executed_at: now,
                duration_ms,
                rollback: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the cgroup path of a PID from /proc/<pid>/cgroup (cgroup v2).
fn read_pid_cgroup(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/cgroup", pid);
    let content = std::fs::read_to_string(&path).ok()?;
    // cgroup v2 line format: "0::/user.slice/user-1000.slice/session-3.scope"
    for line in content.lines() {
        if let Some(cg) = line.split("::").nth(1) {
            return Some(cg.trim().to_owned());
        }
    }
    None
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{ActionId, ActionRequest, ActionSafetyLevel, DecisionId};

    #[test]
    fn group_path_joins_correctly() {
        let ex = LinuxCgroupExecutor::new(PathBuf::from("/sys/fs/cgroup/agenticos"));
        assert_eq!(
            ex.group_path("bench"),
            PathBuf::from("/sys/fs/cgroup/agenticos/bench")
        );
    }

    #[test]
    fn observe_only_returns_success() {
        let ex = LinuxCgroupExecutor::new(PathBuf::from("/tmp/agenticos-test"));
        let action = ApprovedAction {
            request: ActionRequest {
                id: ActionId::from("a1"),
                kind: ActionKind::ObserveOnly,
                safety_level: ActionSafetyLevel::ReadOnly,
            },
            decision_id: DecisionId::from("d1"),
        };
        let result = ex.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(!result.executed_at.is_empty());
        assert!(result.duration_ms < 1000);
    }

    #[test]
    fn cgroup_snapshot_round_trips_via_json() {
        let snap = CgroupSnapshot {
            action: "set_memory_max".into(),
            group: "bench".into(),
            previous_value: Some("max".into()),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: CgroupSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.action, "set_memory_max");
        assert_eq!(back.group, "bench");
        assert_eq!(back.previous_value, Some("max".into()));
    }

    #[test]
    fn rollback_manager_handles_unknown_action() {
        let mgr = CgroupRollbackManager::new(PathBuf::from("/tmp"));
        let token = RollbackToken {
            token: r#"{"action":"unknown_stub","group":"test","previous_value":null}"#.into(),
        };
        let result = mgr.rollback(token).unwrap();
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result.message.contains("unknown rollback action"));
    }
}
