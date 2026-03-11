# tokenwise 🪀

tokenwise is a code intelligence MCP server. It gives your AI agent a full toolset to read and edit any codebase — grounded in the AST, not model memory.

**Built for agents.** Drop it into Claude Code, Cursor, or any MCP-compatible agent. The agent calls the tools. You get better answers.

**99% eval accuracy** across 7 real codebases (120 tasks) vs 26% baseline (Claude Code without tokenwise). No API keys. No SaaS. No telemetry.

---

## How it works for your agent

```
you run:   tokenwise warm --path /your/project
agent gets: tools for search, read, write, rename, trace, analyze
agent uses: supersearch / symbol / flow / patch — not grep, not cat
result:     answers from facts, not memory. no hallucinated file paths.
```

---

## Setup (4 steps)

### 1. Install

**Homebrew — direct formula install (recommended)**
```bash
brew install --formula https://raw.githubusercontent.com/sauravtom/tokenwise/main/Formula/tokenwise.rb
```

This does not require a separate tap repo.

**macOS — manual (Apple Silicon)**
```bash
curl -L https://github.com/sauravtom/tokenwise/releases/latest/download/tokenwise-aarch64-apple-darwin.tar.gz | tar xz
sudo mv tokenwise-aarch64-apple-darwin /usr/local/bin/tokenwise
# Required: sign the binary or macOS Gatekeeper will kill it silently (exit 137)
codesign --force --deep --sign - /usr/local/bin/tokenwise
```

**Linux (x86_64)**
```bash
curl -L https://github.com/sauravtom/tokenwise/releases/latest/download/tokenwise-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv tokenwise-x86_64-unknown-linux-gnu /usr/local/bin/tokenwise
```

Verify:
```bash
tokenwise --version
```

> **Why `/usr/local/bin`?** The MCP server must be on a path accessible to all tools and shells. Install here once — it works everywhere.
>
> **No sudo?** Install to `~/.local/bin/tokenwise` instead, but update the `command` path in the MCP config (step 2) to match.

---

### 2. Add to your agent's MCP config

**Claude Code** — add the `tokenwise` block inside `mcpServers` in `~/.claude/settings.json`:
```json
{
  "mcpServers": {
    "tokenwise": {
      "type": "stdio",
      "command": "/usr/local/bin/tokenwise",
      "args": ["--mcp-server"]
    }
  }
}
```

> If `~/.claude/settings.json` already has other MCP servers, just add the `"tokenwise": { ... }` block alongside them. Don't replace the whole file.

> If you installed without `sudo` and the binary is at `~/.local/bin/tokenwise`, use that path instead.

**Cursor** — add the same block to your Cursor MCP config file.

**Codex CLI** — add tokenwise as an MCP server from your terminal:
```bash
codex mcp add tokenwise -- /usr/local/bin/tokenwise --mcp-server
```
If you installed to `~/.local/bin/tokenwise`, use that path in the command.

Then reconnect your agent client so it picks up the new server (for Claude Code, run `/mcp` or restart the app).

---

### 3. Index your project

Run this once per project to build indexes and start background refresh:
```bash
tokenwise warm --path /path/to/your/project
tokenwise daemon status --path /path/to/your/project
```

Notify daemon after edits (for hooks/editor integration):
```bash
tokenwise daemon notify --path /path/to/your/project --file src/auth.rs
```

LLM-first quick checks before editing:
```bash
tokenwise context --path /path/to/your/project --name login
tokenwise change-impact --path /path/to/your/project --files src/auth.rs
tokenwise cfg --path /path/to/your/project --file src/auth.rs --function login
tokenwise dfg --path /path/to/your/project --file src/auth.rs --function login
tokenwise program-slice --path /path/to/your/project --file src/auth.rs --function login --line 42
```

Daemon config is auto-created on first `warm` at `.tokenwise/config.json`:
```json
{
  "daemon": {
    "auto_reindex_threshold": 20,
    "poll_ms": 1000,
    "idle_flush_secs": 10
  },
  "semantic": {
    "enabled": true
  }
}
```

Automate notify for continuous freshness:
```bash
# Git hook (post-commit)
git diff --name-only HEAD~1 | xargs -I{} tokenwise daemon notify --path . --file {}

# Editor on-save hook
tokenwise daemon notify --path . --file "$FILE"
```

---

### 4. Add the hook (Claude Code only — strongly recommended)

Without this, Claude sees tokenwise but won't prefer it over grep/cat. Add to your project's `.claude/settings.local.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo '[tokenwise] Use mcp__tokenwise__supersearch instead of Grep. Use mcp__tokenwise__symbol+include_source instead of Read. Use mcp__tokenwise__slice for line ranges.'"
          }
        ]
      }
    ]
  }
}
```

This injects a reminder on every prompt so Claude actively uses tokenwise tools instead of falling back to file reads and grep.

---

You're set. Open Claude Code, Cursor, or Codex CLI, start a session, and ask about your code. The agent calls `llm_instructions` automatically on first contact and picks up the full tool catalog.

---

## Spec-Driven Development (SDD)

tokenwise also supports lightweight SDD workflows via slash-style commands:

```bash
tokenwise /tw:propose add-dark-mode
tokenwise /tw:apply
tokenwise /tw:archive
tokenwise /tw:status
```

What each command does:
- `/tw:propose <name>` creates `tokenwise/changes/<name>/` with `proposal.md`, `design.md`, `tasks.md`, and `specs/requirements.md`.
- `/tw:apply [name]` marks pending checklist items in `tasks.md` as complete and prints task progress.
- `/tw:archive [name]` moves the change to `tokenwise/changes/archive/<date>-<name>/` and syncs specs into `tokenwise/specs/`.
- `/tw:status` (or `/tw:show`) prints summary stats and per-change progress bars.

If `[name]` is omitted for apply/archive, tokenwise uses the first active change in `tokenwise/changes/`.
Legacy `/yoyo:*` aliases are still accepted temporarily and emit a deprecation warning.

---

## Tools

### Bootstrap
| Tool | What it does |
|---|---|
| `warm` | Build index + start daemon for incremental refresh. |
| `bake` | Parse the project, write the AST index. Run first. |
| `daemon` | Start/stop/status/notify for background refresh. |
| `shake` | Language breakdown, file count, top-complexity functions. |
| `llm_instructions` | Agent bootstrap: tool list, workflows, prime directives. |

### Read
| Tool | What it does |
|---|---|
| `symbol` | Find a function by name — file, line range, optionally full body. |
| `context` | Compact function bundle: metadata, callers, outgoing calls, endpoints, snippet. |
| `slice` | Read any line range from any file. |
| `supersearch` | AST-aware search across all files. Replaces grep. |
| `semantic_search` | Find functions by intent. Local ONNX embeddings, no API key. |
| `file_functions` | Every function in a file with complexity scores. |
| `find_docs` | Locate README, .env, Dockerfile, config files. |

### Understand
| Tool | What it does |
|---|---|
| `cfg` | Initial control-flow graph for a function (Rust + Go first). |
| `dfg` | Initial data-flow graph for a function (variable defs → uses, Rust + Go first). |
| `program_slice` (`program-slice` in CLI) | Dependency-aware backward slice for a target line in a function. |
| `blast_radius` | All transitive callers of a symbol + affected files. |
| `change_impact` | Changed files → impacted functions and likely tests to run. |
| `flow` | Endpoint → handler → call chain in one call. |
| `trace_down` | BFS call chain to db/http/queue boundary. Rust + Go. |
| `health` | Dead code, god functions, duplicate names. |
| `architecture_map` | Directory tree with inferred roles. |
| `package_summary` | Functions, endpoints, complexity for a module path. |
| `api_surface` | Exported functions grouped by module. |
| `suggest_placement` | Ranked files to place a new function. |
| `all_endpoints` | All detected HTTP routes. |
| `api_trace` | Route path + method → handler function. |
| `crud_operations` | CRUD matrix inferred from routes. |

### Write
| Tool | What it does |
|---|---|
| `patch` | Write by symbol name, line range, or string match. Auto-reindexes. |
| `patch_bytes` | Write at exact byte offsets. |
| `multi_patch` | N edits across M files in one call. |
| `graph_rename` | Rename a symbol at definition + every call site, atomically. |
| `graph_create` | Create a new file with an initial function scaffold. |
| `graph_add` | Insert a function scaffold into an existing file. |
| `graph_move` | Move a function between files. |
| `graph_delete` | Remove a function by name. Checks blast radius first. |

**Languages:** TypeScript, JavaScript, Rust, Python, Go, C, C++, C#, Java, Kotlin, PHP, Ruby, Swift, Bash

---

## Benchmarking

Run latency + semantic recall benchmark on the local fixture:

```bash
python3 evals/bench_latency_semantic.py \
  --project tests/fixtures/sample_project \
  --tasks evals/tasks/tokenwise_semantic_benchmark.json \
  --runs 5 \
  --out evals/benchmarks/phase5-baseline.json
```

See benchmark details in [`evals/benchmarks/README.md`](./evals/benchmarks/README.md).

---

Full documentation: [`docs/README.md`](./docs/README.md) · [Eval report](./evals/REPORT.md) · [Changelog](./CHANGELOG.md) · MIT
