# AgenticOS Experiments

This directory stores experiment data, results, and reports.

## Structure

```
experiments/
├── raw/         # Raw experiment output (JSON, CSV traces)
├── processed/   # Cleaned/aggregated data for plotting
└── reports/     # Generated comparison reports
```

## Workflow

1. Define scenario in `benchmarks/<scenario>.toml`.
2. Run baseline: `agenticos bench run <scenario> --mode baseline`.
3. Run agenticos: `agenticos bench run <scenario> --mode agenticos`.
4. Compare: `agenticos report generate <experiment-id>`.
5. Output lands in `experiments/`.

## Reproducibility

Every experiment has a config file and a trace ID. The trace ID can be replayed with `agenticos trace replay <id>` to verify deterministic execution.
