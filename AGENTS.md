# Repository Guidelines

## Project Structure & Module Organization
- `src/` contains the Rust CLI and MCP server.
- `src/engine/` holds core tool logic (`search.rs`, `edit.rs`, `graph.rs`, `analysis.rs`, `api.rs`, `nav.rs`).
- `src/lang/` contains language-specific analyzers (Rust, Go, Python, TypeScript, etc.).
- `tests/` contains integration fixtures (`tests/fixtures/sample_project/...`).
- `evals/` contains Python eval harnesses (`run.py`, `run_semantic.py`, `write_run.py`) and task specs in `evals/tasks/`.
- `docs/` and `reports/` are documentation and benchmark artifacts.

## Build, Test, and Development Commands
- `cargo build` builds the debug binary.
- `cargo build --release` builds optimized artifacts (used by release workflow).
- `cargo test` runs the test suite (this is what CI runs on Linux and macOS).
- `cargo run -- --help` checks CLI wiring locally.
- `python3 evals/run.py --tasks evals/tasks/tokio.json` runs structural evals.
- `python3 evals/run_semantic.py --tasks evals/tasks/tokio_semantic.json` runs semantic evals.
- `python3 evals/write_run.py --tasks evals/tasks/ripgrep_write.json` runs write-operation evals.

## Coding Style & Naming Conventions
- Use Rust 2021 idioms and 4-space indentation.
- Prefer small, focused modules in `src/engine/`; keep tool behavior explicit over clever abstractions.
- File/module names are `snake_case` (`blast_radius`, `architecture_map`); types/traits are `UpperCamelCase`.
- Keep CLI/tool output JSON stable and schema-consistent.

## Testing Guidelines
- Add/adjust tests for behavior changes; do not rely on manual inspection only.
- Place parser/index fixtures under `tests/fixtures/`.
- Name tests by expected behavior (example: `test_graph_delete_blocks_active_callers`).
- Run `cargo test` before opening a PR; run targeted eval scripts when changing search/ranking/write logic.

## Commit & Pull Request Guidelines
- Follow existing commit style: `<type>: <summary>` (for example, `fix: ...`, `feat: ...`, `docs: ...`, `chore: ...`).
- Keep commits scoped and reviewable; reference issues with `closes #N` when applicable.
- PRs should include: problem statement, approach, test/eval evidence, and any docs updates.
- Include screenshots only for UI/docs rendering changes (for example, `docs/index.html`).

## Security & Configuration Tips
- Never commit API keys or local machine paths.
- Prefer reproducible commands and checked-in task specs over ad-hoc scripts.
