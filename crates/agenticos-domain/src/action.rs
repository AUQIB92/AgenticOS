use crate::{ActionId, DecisionId};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionRequest {
    pub id: ActionId,
    pub kind: ActionKind,
    pub safety_level: ActionSafetyLevel,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApprovedAction {
    pub request: ActionRequest,
    pub decision_id: DecisionId,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionResult {
    pub action_id: ActionId,
    pub status: ActionStatus,
    pub message: String,
    pub executed_at: String,
    pub duration_ms: u64,
    pub rollback: Option<RollbackToken>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActionKind {
    CgroupCreate { name: String },
    CgroupSetCpuMax { group: String, quota: String },
    CgroupSetCpuWeight { group: String, weight: u64 },
    CgroupSetMemoryMax { group: String, bytes: u64 },
    CgroupMovePid { group: String, pid: u32 },
    ProcessFreezeGroup { group: String },
    ProcessThawGroup { group: String },
    ProcessTerminateGroup { group: String },
    WorkloadClassifyRecommend { group: String, classification: String },
    ObserveOnly,
    // ── Desktop/Productivity Actions ─────────────────────────────────
    LaunchApplication { application: String },
    OpenUrl { url: String },
    RunCommand { command: String, args: String },
    CreateDirectory { path: String },
    OpenFile { path: String },
    CloneRepository { url: String, directory: String },
    CreateProjectWorkspace { project_name: String, framework: String },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActionSafetyLevel {
    ReadOnly,
    LowRisk,
    MediumRisk,
    HighRisk,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActionStatus {
    Pending,
    Proposed,
    Approved,
    Denied,
    Executing,
    Succeeded,
    Failed,
    RolledBack,
    DryRun,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RollbackToken {
    pub token: String,
}
