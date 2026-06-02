# AgenticOS Architecture

AgenticOS is organized as a clean-architecture Rust workspace. The domain model defines OS-policy concepts, the application layer defines ports, and infrastructure crates will later provide event bus, policy, runtime, observation, execution, and dashboard adapters.

Agents are advisory components. The deterministic Policy Kernel is the only component that can approve actions. The Linux substrate is the only component that executes mechanisms.
