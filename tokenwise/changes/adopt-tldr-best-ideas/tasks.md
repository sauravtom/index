# Tasks

## 1. Phase 1: Daemon + Warm
- [x] 1.1 Add `src/daemon.rs` with start/stop/status/notify and worker loop.
- [x] 1.2 Add CLI command surface: `warm`, `daemon`, hidden `daemon-run`.
- [x] 1.3 Add MCP tools: `warm`, `daemon_start`, `daemon_stop`, `daemon_status`, `daemon_notify`.
- [x] 1.4 Update README setup path to use `warm` and daemon notify examples.

## 2. Phase 2: Freshness + Config
- [x] 2.1 Add `.tokenwise/config.json` defaults and documented schema.
- [x] 2.2 Add smarter incremental embeddings update (avoid full rebuild on every flush).
- [x] 2.3 Add file-change hook examples (git + editor).

## 3. Phase 3: LLM Context APIs
- [x] 3.1 Add `context` command/tool for compact function bundles.
- [x] 3.2 Add `change-impact` command/tool to map changed files to impacted tests/functions.

## 4. Phase 4: Deep Analysis Layers
- [x] 4.1 Add initial CFG command for Rust/Go.
- [x] 4.2 Add initial DFG command for Rust/Go.
- [x] 4.3 Add dependency-aware program slice command.

## 5. Validation
- [x] 5.1 Add unit/integration tests for daemon lifecycle and queue handling.
- [x] 5.2 Add latency + semantic recall benchmarks to `evals`.
- [x] 5.3 Update docs and release notes with new workflow.
