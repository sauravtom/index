# Assessment — What's Good & What Needs Work
**Date:** 2026-03-03
**Based on:** source code review (`src/*.rs`, 3331 lines) + live benchmark on face-api.js (386 files)

---

## What's Good

### Architecture & code quality

**Clean module separation.** The four-file layout (`engine.rs` / `cli.rs` / `mcp.rs` / `ts_index.rs`) is excellent. Engine functions are pure — they take a path, return `Result<String>`, and have no I/O side effects beyond reading the bake index. CLI and MCP are thin wrappers that parse arguments and forward to the engine. This makes the engine trivially testable in isolation.

**Tree-sitter integration is correct.** Using a proper AST parser rather than regex means `function_declaration` extraction is accurate — no false positives from string literals or comments. The `estimate_complexity` function walks the AST recursively and counts branching nodes (if/for/while/do/switch/ternary), giving a genuine cyclomatic approximation. Scores matched expectations in the benchmark (`nonMaxSuppression` = 6, most `convLayer` helpers = 1).

**MCP server is spec-compliant.** The JSON-RPC 2.0 implementation in `mcp.rs` correctly handles `initialize`, `list_tools`, and `call_tool`. Tool schemas use proper JSON Schema with typed parameters. The stdio transport is correctly handled. This means tokenwise works out of the box with any MCP-compliant client (Claude Code, Cursor, etc.) without configuration beyond pointing at the binary.

**Bake index schema is clean.** `BakeIndex` contains `files` (path + language), `ts_functions` (name + file + line range + complexity), `express_endpoints` (method + path + file + handler), and `languages`. This is a minimal, stable schema that all downstream tools can rely on. Adding new index data is straightforward without breaking existing consumers.

**Error handling is idiomatic.** `anyhow` with `.with_context()` at every file I/O boundary means errors carry actionable messages ("Failed to read file X (resolved to Y)"). No panics in the hot path.

**`supersearch` AST walk is actually implemented.** Despite the "best-effort" label in CLI help strings, `walk_ts_supersearch` in `engine.rs:590` does real AST-aware filtering: it propagates `in_call`, `in_assign`, `in_return` state down the tree and applies `context_ok`/`pattern_ok` guards correctly. The implementation is structurally sound — the main issue is that it's bypassed when both context and pattern are "all" (the defaults).

---

### Tool effectiveness (confirmed working well)

| Tool | Why it works well |
|---|---|
| `bake` | Fast, deterministic, writes a stable JSON schema. The foundation everything else builds on. |
| `slice` | Dead-simple: read the file, return lines[start..end]. Always works, always accurate. |
| `package_summary` | Filters `ts_functions` and `files` by path substring — simple, correct, returns the right shape. |
| `search` | Lowercased substring match on function names + file paths. Fast, predictable. |
| `symbol` | Exact-before-partial ranking on function names. Reliable for looking up known function names. |
| `file_functions` | Accurate for files using only `function_declaration` (many TS util/factory files). |

---

## What Needs Work

### Critical: ts_index.rs only captures one AST node type

`walk_ts()` in `ts_index.rs:70` handles exactly one case:
```rust
"function_declaration" => { ... }
```

Modern TypeScript rarely uses bare `function_declaration`. Almost all real-world TS code uses:
- Class methods: `method_definition` → `SsdMobilenetv1.locateFaces`, `forwardInput`
- Arrow functions: `const fn = () => {}` → most React components, utility lambdas
- Function expressions: `const fn = function() {}`

**Impact:** `file_functions` returned **zero results** for `SsdMobilenetv1.ts` (133 lines, 5 public methods), `FaceRecognitionNet.ts`, and all class-based files. This is the most impactful single fix — it would immediately make `file_functions`, `symbol`, `api_surface`, and complexity rankings dramatically more accurate.

---

### High: `architecture_map` role inference is three keywords wide

The entire role-inference system is:
```rust
if contains("routes") || contains("controllers") → "http-endpoints"
if contains("services")                          → "services"
if contains("models") || contains("entities")    → "models"
```

This covers roughly 10% of real directory naming conventions. face-api.js has directories like `globalApi`, `faceProcessor`, `draw`, `dom`, `ops`, `env`, `factories` — none of which get any role. The tool's stated purpose is "project structure and placement hints", but it delivers no hints when names don't match the three patterns.

**The intent parameter is accepted but not used.** `architecture_map(intent="computer vision library")` is documented to influence suggestions, but `intent` isn't wired to any logic in the engine.

---

### High: Output size is unbounded for two tools

`api_surface` and `find_docs` collect results with no cap. On a 386-file project:
- `api_surface` → 50.7 KB
- `find_docs` → ~298 KB (previous benchmark)

Both exceed what any LLM can process inline. There's no `--limit`, `--offset`, or `"truncated"` flag. This makes them unusable as MCP tools on any project with >100 TypeScript files.

---

### Medium: supersearch defaults bypass AST filtering

When `context=all` and `pattern=all` (the defaults), the engine takes the plain `str.contains()` branch — the Tree-sitter walk is never entered. The AST branch only activates when the user explicitly passes a non-default value. This means most supersearch calls in practice use dumb line search regardless of file type.

A secondary issue: the AST walk may emit duplicate entries for the same line when multiple matching nodes (e.g., an identifier in both a `call_expression` and its enclosing `variable_declarator`) are visited on the same source line. Results should be deduplicated by `(file, line)`.

---

### Medium: Suggest placement returns test files as top candidates

The placement heuristic scores files by path keywords (`handler`, `service`, `repository`, `model`, `util`, `test`) and by proximity to the `related_to` symbol. When the closest file containing the related symbol is a test file, that test file wins. The benchmark returned `test/tests/globalApi/detectAllFaces.test.ts` for a `service`-type placement — the opposite of correct.

---

## Priority Matrix

| Area | Current State | Target State | Effort |
|---|---|---|---|
| Arrow/method indexing | Misses ~70% of TS code | All function types indexed | Medium |
| `architecture_map` roles | 3 hardcoded keywords | 20+ patterns + intent-aware | Small |
| Output pagination | Unbounded | Max 50 items + truncated flag | Small |
| `supersearch` defaults | Plain text | Always AST for TS | Small |
| `suggest_placement` | Includes test files | Source-only candidates | Small |
| `symbol` class support | Functions only | Classes + methods | Small |
| Language support | TS/JS only | + Python, Go | Large |
| Call graph in bake | Not persisted | `calls`/`called_by` edges | Large |
| Tests & CI | Zero tests | Unit + integration | Medium |
| `api_trace` beyond Express | Express-only | + NestJS, Fastify | Medium |

---

## Verdict

tokenwise's core pipeline (`bake` → `symbol` → `slice` → `package_summary`) delivers real value today for TypeScript/Express projects. The architecture is sound and the MCP integration is production-ready. The 30× onboarding speedup demonstrated in the benchmark is genuine.

The three highest-leverage fixes — indexing arrow functions/class methods, expanding role inference keywords, and adding output pagination — are all small changes in `ts_index.rs` and `engine.rs` that would immediately make the tool reliable on the full range of real TypeScript codebases.
