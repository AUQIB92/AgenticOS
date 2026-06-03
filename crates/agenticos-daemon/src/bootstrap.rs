use std::path::PathBuf;
use std::sync::Arc;

use agenticos_agents::{DummyAgentA, DummyAgentB, SecurityAgent};
use agenticos_application::{AppError, EventBus, ObserverPort};
use agenticos_bus::{InMemoryEventBus, SqliteTraceStore, TraceStore};
use agenticos_domain::AgentId;
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
#[cfg(target_os = "linux")]
use agenticos_executor::LinuxCgroupExecutor;
use agenticos_observe::SystemSampler;
use agenticos_policy::DefaultPolicyKernel;
use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};
use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig};

use crate::config::DaemonConfig;

pub struct DaemonContext {
    pub config: DaemonConfig,
    pub observer: Box<dyn ObserverPort>,
    pub event_bus: Box<dyn EventBus>,
    pub trace_store: Box<dyn TraceStore>,
    pub policy_kernel: Arc<DefaultPolicyKernel>,
    pub safety_governor: DefaultSafetyGovernor,
    pub executor: Box<dyn ApprovedActionExecutor>,
    pub agent_runtime: InMemoryAgentRuntime,
}

impl DaemonContext {
    pub fn from_config(config: DaemonConfig) -> Result<Self, AppError> {
        let event_store_type = config.event_store();
        let mode = config.mode();

        let observer: Box<dyn ObserverPort> = Box::new(SystemSampler::new(Some(
            PathBuf::from("/sys/fs/cgroup"),
        )));

        let event_bus: Box<dyn EventBus> = Box::new(InMemoryEventBus::new());

        let trace_store: Box<dyn TraceStore> = match event_store_type {
            "sqlite" => {
                let path = config.db_path();
                Box::new(
                    SqliteTraceStore::new(path)
                        .map_err(|e| AppError::Message(format!("sqlite init: {e}")))?,
                )
            }
            _ => Box::new(agenticos_bus::InMemoryTraceStore::new()),
        };

        let policy_kernel: Arc<DefaultPolicyKernel> = match mode {
            "safe-local" => Arc::new(DefaultPolicyKernel::safe_local()),
            "benchmark" | "development" => Arc::new(DefaultPolicyKernel::benchmark()),
            _ => {
                let allowed_actions = vec![agenticos_policy::ActionKindClass::ObserveOnly];
                Arc::new(DefaultPolicyKernel::new(agenticos_policy::PolicyKernelConfig {
                    kernel_agent_id: AgentId::from("policy-kernel"),
                    allowed_actions,
                    allow_medium_risk: false,
                    allow_high_risk: false,
                    minimum_confidence: 0.0,
                }))
            }
        };

        let executor: Box<dyn ApprovedActionExecutor> = {
            #[cfg(target_os = "linux")]
            {
                if mode == "benchmark" || mode == "development" {
                    Box::new(
                        LinuxCgroupExecutor::new("/sys/fs/cgroup".into()),
                    )
                } else {
                    Box::new(DryRunExecutor::new())
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = mode;
                Box::new(DryRunExecutor::new())
            }
        };

        let agent_runtime = InMemoryAgentRuntime::new();

        let safety_governor = DefaultSafetyGovernor::new(SafetyConfig {
            max_cpu_weight: match mode {
                "benchmark" | "development" => 10000,
                _ => 1000,
            },
            max_memory_bytes: match mode {
                "benchmark" | "development" => Some(64u64 * 1024 * 1024 * 1024),
                _ => Some(16u64 * 1024 * 1024 * 1024),
            },
            veto_on_security_incidents: true,
        });

        let mut ctx = Self {
            config,
            observer,
            event_bus,
            trace_store,
            policy_kernel,
            safety_governor,
            executor,
            agent_runtime,
        };

        ctx.register_default_agents()?;

        Ok(ctx)
    }

    fn register_default_agents(&mut self) -> Result<(), AppError> {
        self.agent_runtime
            .register(Box::new(DummyAgentA::new(AgentId::from("dummy-a"))))?;
        self.agent_runtime
            .register(Box::new(DummyAgentB::new(AgentId::from("dummy-b"))))?;
        self.agent_runtime
            .register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))?;
        self.agent_runtime
            .start(AgentId::from("dummy-a"))?;
        self.agent_runtime
            .start(AgentId::from("dummy-b"))?;
        self.agent_runtime
            .start(AgentId::from("security-agent"))?;
        Ok(())
    }
}
