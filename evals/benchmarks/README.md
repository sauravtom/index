# Latency + Semantic Recall Benchmarks

This folder tracks lightweight, repeatable performance checks for tokenwise query latency and semantic top-3 recall.

## Runner

Use:

```bash
python3 evals/bench_latency_semantic.py \
  --project tests/fixtures/sample_project \
  --tasks evals/tasks/tokenwise_semantic_benchmark.json \
  --runs 5 \
  --out evals/benchmarks/phase5-baseline.json
```

Notes:
- The runner auto-calls `tokenwise warm --no-daemon` for index freshness.
- It measures command latency for:
  - `symbol`, `context`, `change-impact`, `cfg`, `dfg`, `program-slice`, `semantic-search`
- It also evaluates semantic recall with a fixture query set (top-3 hit rate).

## Latest Baseline (2026-03-11)

Source: `evals/benchmarks/phase5-baseline.json`

- Semantic recall (top-3): `5/5` (`100.0%`)
- Median latency:
  - `symbol`: `6.48ms`
  - `context`: `6.12ms`
  - `change-impact`: `6.03ms`
  - `cfg`: `6.13ms`
  - `dfg`: `6.08ms`
  - `program-slice`: `6.04ms`
  - `semantic-search`: `143.55ms`

This benchmark is intentionally small and deterministic; use it as a regression guard for local changes.
