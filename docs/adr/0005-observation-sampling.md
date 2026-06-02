# ADR 0005: Observation Sampling

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

The agent loop needs a steady stream of observations to react to system state changes. The question is how observations are triggered and at what rate.

## Decision

Observations are collected on a **periodic sampling timer**, not on-demand by agents.

1. **Timer-driven.** A daemon-level tokio task wakes every `sampling_interval_ms` and calls the Sampler once.

2. **Sampler owns timing.** The Sampler starts a monotonic clock before collection and records `collection_duration_ms` on every `Observation` in the batch.

3. **Fixed rate for MVP.** Default sampling interval is 1000 ms (1 Hz). Future work may add adaptive sampling or event-triggered collection.

4. **Single sample round = one batch.** All collectors (process, memory, cgroup) run in sequence within one round. The batch shares the same `observed_at` timestamp.

5. **Batch is published as individual bus events.** Each `Observation` is wrapped in an `EventEnvelope` and published to `observations.<source>` topics. This preserves traceability through `TraceStore`.

6. **Consumers are async and decoupled.** Agents subscribe to observation topics and receive observations asynchronously. The sampler does not wait for agent processing.

## Rationale

- Timer-driven sampling decouples observation frequency from agent reasoning latency.
- Fixed rate makes benchmarks reproducible and comparable.
- Publishing individual events (not batches) preserves per-observation causality and trace IDs.

## Consequences

Positive:
- Observation rate is predictable and configurable.
- Agent stalls do not backpressure the observation pipeline.
- Each observation is individually traceable and replayable.

Negative:
- Concurrent state changes between samples are invisible.
- 1 Hz sampling may miss short-lived processes (fork bombs, quick exec).
- Higher sampling rates increase bus and store load.
