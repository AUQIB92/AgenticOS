# AgenticOS Architecture (Alpha-1)

**Date:** 2026-06-02  
**Version:** Alpha-1 (Phase 0 scaffold)

---

## Component Architecture

```mermaid
graph TB
    subgraph "Layer 4: Entry Points"
        DAEMON["agenticos-daemon<br/>tokio main, config, bootstrap, service loop"]
        CLI["agenticos-cli<br/>(scaffold)"]
    end

    subgraph "Layer 3: Infrastructure"
        OBSERVE["agenticos-observe<br/>SystemSampler, ProcfsCollectors,<br/>NoopCollectors"]
        EXECUTOR["agenticos-executor<br/>DryRunExecutor, LinuxCgroupExecutor,<br/>CgroupRollbackManager"]
        BUS["agenticos-bus<br/>InMemoryEventBus, InMemoryTraceStore,<br/>SqliteTraceStore"]
        AGENTS["agenticos-agents<br/>MemoryAgent, DummyAgentA/B,<br/>ProcessAgent (stub), SecurityAgent (stub)"]
        DASHBOARD["agenticos-dashboard<br/>(scaffold)"]
    end

    subgraph "Layer 2: Application Ports"
        POLICY["agenticos-policy<br/>DeterministicPolicyKernel,<br/>DefaultPolicyKernel"]
        RUNTIME["agenticos-runtime<br/>InMemoryAgentRuntime,<br/>AgentLifecycle"]
        APPLICATION["agenticos-application<br/>EventBus, PolicyKernelPort,<br/>ObserverPort, ActionExecutorPort"]
    end

    subgraph "Layer 0: Domain"
        DOMAIN["agenticos-domain<br/>Observation, Proposal, Decision, Action,<br/>Agent trait, EventEnvelope, Metrics, IDs"]
    end

    DOMAIN --> APPLICATION
    APPLICATION --> POLICY
    APPLICATION --> RUNTIME
    APPLICATION --> BUS
    POLICY --> OBSERVE
    POLICY --> EXECUTOR
    OBSERVE --> DAEMON
    EXECUTOR --> DAEMON
    BUS --> DAEMON
    AGENTS --> DAEMON
    POLICY --> DAEMON
    RUNTIME --> DAEMON
    CLI --> DAEMON
    DASHBOARD --> DAEMON
```

---

## Event Flow

```mermaid
sequenceDiagram
    participant O as Observer
    participant EB as Event Bus
    participant TS as Trace Store
    participant AR as Agent Runtime
    participant PK as Policy Kernel
    participant E as Executor

    loop Every 1 second
        O->>O: observe()
        O->>EB: publish(Observation)
        O->>TS: append(Observation)

        AR->>AR: collect_proposals(observations)
        AR->>EB: publish(Proposal)
        AR->>TS: append(Proposal)

        loop For each proposal
            PK->>PK: evaluate(proposal)
            PK->>EB: publish(Decision)
            PK->>TS: append(Decision)

            alt Decision == Approved
                PK->>E: ApprovedAction
                E->>E: execute(action)
                E->>EB: publish(ActionResult)
                E->>TS: append(ActionResult)
            else Decision == Denied
                PK-->>AR: denial logged
            end
        end

        O->>EB: publish(Metrics)
        O->>TS: append(Metrics)
    end
```

---

## Multi-Agent Coordination

```mermaid
graph TB
    subgraph "Daemon Tick (1 Hz)"
        direction TB
        O[Observer] --> OBS[Observations]
        OBS --> AR{Agent Runtime}
        
        subgraph "Agent Registry (ordered)"
            direction LR
            A1["Agent A<br/>(DummyAgentA)"]
            A2["Agent B<br/>(DummyAgentB)"]
            AX["Agent N<br/>(...)"]
        end
        
        AR --> A1
        AR --> A2
        AR --> AX
        
        A1 --> P1[Proposal A1]
        A2 --> P2[Proposal B1]
        AX --> PN[Proposal N1]
        
        P1 --> PQ[Proposal Queue<br/>ordered vec]
        P2 --> PQ
        PN --> PQ
        
        PQ --> PK[Policy Kernel]
        
        PK --> D1[Decision A]
        PK --> D2[Decision B]
        PK --> DN[Decision N]
        
        D1 --> EX{Executor}
        D2 --> EX
        DN --> EX
        
        EX --> R1[ActionResult A]
        EX --> R2[ActionResult B]
        EX --> RN[ActionResult N]
    end

    subgraph "Persistence"
        EB[Event Bus] --> TS[Trace Store]
        OBS --> EB
        P1 --> EB
        P2 --> EB
        D1 --> EB
        D2 --> EB
        R1 --> EB
        R2 --> EB
    end
```

### Coordination Properties

| Property | Implementation |
|----------|---------------|
| **Ordering** | Agents registered in `Vec<AgentId>` → proposals collected in insertion order |
| **Concurrency** | Single-threaded tick loop; no parallel proposal processing |
| **Determinism** | Same observations + same agents + same policy → same decisions |
| **Auditability** | Every event published to bus + persisted to trace store |
| **Isolation** | Agents cannot observe each other's proposals within the same tick |
| **Fairness** | First registered = first processed (no priority inversion) |

---

## Proposal Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Created: Agent.propose()
    Created --> Queued: Runtime.collect_proposals()
    Queued --> Evaluating: Policy.evaluate()
    
    Evaluating --> Approved: safety check passed
    Evaluating --> Denied: safety check failed
    Evaluating --> RequiresApproval: policy config
    
    Approved --> Executing: ApprovedAction created
    Executing --> Succeeded: executor returns Ok
    Executing --> Failed: executor returns Err
    
    Denied --> [*]: logged + traced
    RequiresApproval --> [*]: deferred (not implemented)
    Succeeded --> [*]: traced
    Failed --> [*]: incident published
```

### Proposal Fields

```rust
Proposal {
    id: ProposalId,              // Unique, auto-generated
    agent_id: AgentId,           // Originating agent
    created_at: String,          // ISO-8601 timestamp
    based_on: Vec<ObservationId>, // Observations that triggered this proposal
    requested_action: ActionRequest,  // What to do
    rationale: String,           // Why (human-readable)
    confidence: Confidence(f32),  // 0.0 – 1.0
}
```

---

## Decision Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Received: policy.evaluate(proposal)
    Received --> Checked: confidence in [0,1]
    Checked --> ConfidenceFailed: below minimum
    Checked --> CapabilityChecked: confidence OK
    
    CapabilityChecked --> MissingCapability: action not allowed
    CapabilityChecked --> SafetyChecked: action allowed
    
    SafetyChecked --> Approved: ReadOnly | LowRisk
    SafetyChecked --> MediumRiskCheck: MediumRisk
    SafetyChecked --> HighRiskCheck: HighRisk
    
    MediumRiskCheck --> Approved: allow_medium_risk
    MediumRiskCheck --> UnsafeAction: medium risk denied
    
    HighRiskCheck --> Approved: allow_high_risk
    HighRiskCheck --> UnsafeAction: high risk denied
    
    ConfidenceFailed --> [*]: Denied
    MissingCapability --> [*]: Denied
    UnsafeAction --> [*]: Denied
    Approved --> ValidateAction: validate_action()
    ValidateAction --> [*]: ApprovedAction or None
```

### Decision Fields

```rust
Decision {
    id: DecisionId,
    proposal_id: ProposalId,
    decided_at: String,
    decided_by: AgentId,         // Always "policy-kernel"
    outcome: DecisionOutcome,    // Approved | Denied { reason } | RequiresApproval
    explanation: String,
}
```

---

## Crate Dependency Graph

```mermaid
graph LR
    subgraph "External"
        SERDE[serde]
        SERDE_JSON[serde_json]
        RUSQLITE[rusqlite]
        TOML[toml]
        TOKIO[tokio]
    end

    subgraph "Internal"
        DOMAIN[agenticos-domain]
        APP[agenticos-application]
        BUS[agenticos-bus]
        POLICY[agenticos-policy]
        RUNTIME[agenticos-runtime]
        OBSERVE[agenticos-observe]
        EXECUTOR[agenticos-executor]
        AGENTS[agenticos-agents]
        DAEMON[agenticos-daemon]
        CLI[agenticos-cli]
        DASH[agenticos-dashboard]
    end

    DOMAIN --> SERDE
    APP --> DOMAIN
    BUS --> APP
    BUS --> DOMAIN
    BUS --> RUSQLITE
    BUS --> SERDE_JSON
    POLICY --> APP
    POLICY --> DOMAIN
    RUNTIME --> APP
    RUNTIME --> DOMAIN
    OBSERVE --> APP
    OBSERVE --> DOMAIN
    EXECUTOR --> APP
    EXECUTOR --> DOMAIN
    EXECUTOR --> SERDE
    EXECUTOR --> SERDE_JSON
    AGENTS --> DOMAIN
    DAEMON --> AGENTS
    DAEMON --> APP
    DAEMON --> BUS
    DAEMON --> DOMAIN
    DAEMON --> EXECUTOR
    DAEMON --> OBSERVE
    DAEMON --> POLICY
    DAEMON --> RUNTIME
    DAEMON --> SERDE
    DAEMON --> SERDE_JSON
    DAEMON --> TOML
    DAEMON --> TOKIO
    DASH --> DOMAIN
```

---

## Configuration Schema

```toml
[agenticos]
mode = "development"           # safe-local | development | benchmark
event_store = "sqlite"         # sqlite | memory
db_path = "data/agenticos.db"  # SQLite file path
policy = "policies/default.toml"

[safety]
privileged_execution = false   # Future: allow privileged mode
llm_enabled = false            # Future: LLM-based agent reasoning
```
