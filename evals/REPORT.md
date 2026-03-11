# tokenwise Eval Report

Canonical accuracy report for tokenwise's structural and semantic tools against real-world Rust codebases.

Last updated: 2026-03-07 — v0.20.4

---

## Codebases

| Codebase | Description | Functions | Files |
|---|---|---|---|
| [tokio](https://github.com/tokio-rs/tokio) | Async runtime library | 6,174 | 841 |
| [ripgrep](https://github.com/BurntSushi/ripgrep) | CLI search binary | 2,925 | ~200 |
| [axum](https://github.com/tokio-rs/axum) | Web framework | 855 | ~150 |
| **Total** | | **9,954** | |

---

## Results

### Structural tasks

Tests: definition location, visibility, module path, fan-out (calls count), caller count, complexity rank, dead code count, god functions.

| Codebase | Score | Tasks |
|---|---|---|
| tokio | 21/21 — 100% | `evals/tasks/tokio.json` |
| ripgrep | 22/22 — 100% | `evals/tasks/ripgrep.json` |
| axum | 20/20 — 100% | `evals/tasks/axum.json` |
| **Combined** | **63/63 — 100%** | |

### Semantic tasks

Tests: top-3 embedding recall for natural language queries (fastembed AllMiniLML6V2, local ONNX).

| Codebase | Score | Tasks |
|---|---|---|
| tokio | 8/8 — 100% | `evals/tasks/tokio_semantic.json` |
| ripgrep | 5/5 — 100% | `evals/tasks/ripgrep_semantic.json` |
| axum | 5/5 — 100% | `evals/tasks/axum_semantic.json` |
| **Combined** | **18/18 — 100%** | |

### Latency benchmark

Fixture benchmark (repeatable local run) for core read-indexed commands and semantic search.

- Runner: `evals/bench_latency_semantic.py`
- Fixture: `tests/fixtures/sample_project`
- Query set: `evals/tasks/tokenwise_semantic_benchmark.json`
- Baseline artifact: `evals/benchmarks/phase5-baseline.json`

| Command | Median | p95 |
|---|---:|---:|
| `symbol` | 6.48ms | 8.20ms |
| `context` | 6.12ms | 6.40ms |
| `change-impact` | 6.03ms | 6.37ms |
| `cfg` | 6.13ms | 6.76ms |
| `dfg` | 6.08ms | 6.56ms |
| `program-slice` | 6.04ms | 6.16ms |
| `semantic-search` | 143.55ms | 147.70ms |

Semantic top-3 recall on the fixture task set: **5/5 (100%)**.

### Combined

| Type | tokenwise | Claude Code (no index) |
|---|---|---|
| Structural (63 tasks) | **63/63 — 100%** | 20/63 — 32% |
| Semantic (18 tasks) | **18/18 — 100%** | 0/18 — 0% |
| **All (81 tasks)** | **81/81 — 100%** | **20/81 — 25%** |

---

## Question-level comparison (tokio)

| Question | Claude Code | tokenwise |
|---|---|---|
| Where is `poll_acquire` defined? | ✗ grep scan — multiple hits, manual filter | ✓ `batch_semaphore.rs:397` (exact) |
| Is `poll_acquire` public/private/crate? | ✗ Must infer from raw text | ✓ `visibility: private` |
| What module does `spawn_blocking` belong to? | ✗ No tool | ✓ `tokio::runtime::blocking` |
| What does `poll_acquire` call? | ✗ Can't isolate calls by function | ✓ 14 call sites (exact array) |
| Who calls `poll_acquire`? | ✗ grep — includes comments/docs | ✓ 121 distinct callers |
| Who calls `spawn_blocking`? | ✗ grep — includes comments/docs | ✓ 135 distinct callers |
| Most complex function in tokio? | ✗ No tool | ✓ `test_combination` score=957 |
| Dead code in tokio? | ✗ No tool | ✓ 126 unused symbols (public API excluded) |
| Find "semaphore acquisition" | ✗ No semantic search | ✓ `acquire`, `poll_acquire` in top-3 |
| Find "spawn blocking task" | ✗ No semantic search | ✓ `spawn_blocking`, `create_blocking_pool` in top-3 |
| Find "async runtime builder" | ✗ No semantic search | ✓ `build` in top-3 |
| Find "channel sender" | ✗ No semantic search | ✓ `channel`, `send` in top-3 |
| Rename `poll_acquire` safely? | ✗ Corrupts partial matches | ✓ Word-boundary safe |
| Delete `spawn_blocking` — safe? | ✗ Deletes blindly | ✓ BLOCKED — 135 active callers |
| Patch `poll_acquire` by name? | ~ 2 steps (grep line, then edit) | ✓ 1 step (`patch_by_symbol`) |

---

## Running evals

```bash
# Structural
python3 evals/run.py --tasks evals/tasks/tokio.json

# Semantic (requires bake + embed first)
tokenwise bake --path /path/to/tokio
python3 evals/run_semantic.py --tasks evals/tasks/tokio_semantic.json --path /path/to/tokio

# Write ops
python3 evals/write_run.py --tasks evals/tasks/ripgrep_write.json
```

Results are written to `evals/results/` as timestamped JSON files.

---

## Key fixes that reached 100%

| Version | Fix |
|---|---|
| v0.20.0 | `semantic_search` — fastembed ONNX embeddings, test functions excluded, TF-IDF fallback |
| v0.20.1 | `blast_radius` total_callers undercount — unlimited second-pass BFS |
| v0.20.2 | `health` dead_code false positives — `Visibility::Public` excluded |
| v0.20.2 | Stale embeddings DB — delete before rebuild to clear test contamination |
| v0.20.4 | Rust workspace `module_path` — `src` segment stripped, crate name preserved |

---

## Known gaps (open issues)

- **#58** — Rust macro call sites invisible (`tokio::spawn!`, `select!`) — not captured in `calls` array
- **#31** — `graph_move` sibling type visibility breaks compilation after move
- **#5** — `supersearch` context/pattern flags unreliable in some cases
