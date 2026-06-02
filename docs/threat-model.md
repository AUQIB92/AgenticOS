# Threat Model

AgenticOS treats agents, LLM outputs, observations from untrusted workloads, and tool outputs as untrusted inputs.

Core assumptions:

- Agents can fail, hallucinate, stall, or become compromised.
- Prompt text is data, not authority.
- Privileged actions require deterministic validation.
- The system must degrade toward ordinary Linux behavior.

Out of scope for this scaffold:

- Kernel exploit resistance
- Hardware attacks
- Full formal verification
- Production multi-tenant hardening
