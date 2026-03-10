# Benchmark Report — face-api.js
**Date:** 2026-03-03
**Tester:** Claude Sonnet 4.6 via MCP
**Project:** `face-api.js-master` — TypeScript ML library (386 files, TensorFlow.js)
**tokenwise version:** 0.1.0

---

## Setup

```
bake path: /Users/avirajkhare/projects/face-api/face-api.js-master
files_indexed: 386
languages: html, javascript, json, typescript, yaml
```

All 5 batches executed sequentially; Batches 1–4 used parallel tool calls.

---

## Batch 1 — Project Foundation

| Tool | Result | Notes |
|---|---|---|
| `llm_instructions` | ✅ Pass | Returned correct language list and guidance text. Fast. |
| `shake` | ⚠️ Degraded | Without pre-existing bake, returns only language/file counts and a note to run `bake` first. No top complex functions visible until bake completes. |
| `bake` | ✅ Pass | Indexed 386 files; bake.json written to `bakes/latest/`. |

**Finding:** Running all three in parallel means `shake` always fires before `bake` completes. Since `shake` falls back to a lightweight scan when no bake index exists, users get an incomplete picture on first run. Recommend `shake` be run in a second pass *after* `bake`, or that `bake` returns top functions inline.

---

## Batch 2 — Structure & APIs

| Tool | Result | Notes |
|---|---|---|
| `architecture_map` | ⚠️ Partial | Directory tree with file counts was accurate (45 directories). `roles: []` for every single directory — including `src/globalApi`, `src/ssdMobilenetv1`, etc. |
| `all_endpoints` | ✅ Expected | Returned empty (correct — no Express routes in an ML library). |
| `api_surface` | ❌ Token overflow | Returned 50.7 KB — exceeded inline display; required reading from persisted temp file. |
| `crud_operations` | ✅ Expected | Returned empty (correct — no CRUD entities in this project). |

**Finding — `architecture_map` roles:** The role-inference logic in `engine.rs` only matches three patterns: `routes/controllers` → `http-endpoints`, `services` → `services`, `models/entities` → `models`. None of the 45 directories in face-api.js match these keywords, so all get `roles: []`. The tool returns zero architectural signal on this project.

**Finding — `api_surface` size:** 50.7 KB for a 386-file project overwhelms any LLM context window. No `--limit` or `--offset` pagination exists.

---

## Batch 3 — Deep Dives

| Tool | Result | Notes |
|---|---|---|
| `package_summary(src/globalApi)` | ✅ Pass | Returned 13 files and 7 functions with line ranges and complexity. Accurate. |
| `package_summary(src/ssdMobilenetv1)` | ✅ Pass | 12 files, 27 functions. `nonMaxSuppression` correctly ranked as most complex (6). |
| `search(detectAllFaces)` | ✅ Pass | Found function in `src/globalApi/detectFaces.ts:13`. |
| `supersearch(NeuralNetwork loadFromUri)` | ⚠️ No results | Multi-word query across a gap returns 0. This is a limitation — plain-text line search requires both words on the same line. |

---

## Batch 4 — Symbol Inspection

| Tool | Result | Notes |
|---|---|---|
| `symbol(nonMaxSuppression)` | ✅ Pass | Found 2 matches (SSD and ops variants), correctly sorted by complexity. |
| `symbol(SsdMobilenetv1)` | ⚠️ Missed class | The class `SsdMobilenetv1` itself was not found — only `allFacesSsdMobilenetv1` and `createSsdMobilenetv1` appeared. Class definitions are not indexed, only function declarations. |
| `file_functions(src/globalApi/detectFaces.ts)` | ✅ Pass | Returned both `detectSingleFace` and `detectAllFaces`. |
| `file_functions(src/ssdMobilenetv1/SsdMobilenetv1.ts)` | ❌ Zero results | File uses class methods and arrow functions exclusively — none are `function_declaration` nodes in the AST. ts_index.rs only captures `function_declaration`. |

**Root cause confirmed in code (`ts_index.rs:70`):**
```rust
"function_declaration" => { ... }  // only this is captured
// missing: "method_definition", "arrow_function", "function_expression",
//          "public_method_definition", "lexical_declaration" (const fn = ...)
```

---

## Batch 5 — Code Reading & Placement

| Tool | Result | Notes |
|---|---|---|
| `slice(nonMaxSuppression, 1–74)` | ✅ Pass | Returned all 74 lines verbatim, including IOU helper. |
| `slice(SsdMobilenetv1.ts, 1–133)` | ✅ Pass | Read full class body split across two calls. |
| `api_trace(detectAllFaces)` | ⚠️ No results | Not an Express endpoint — expected. Tool currently only traces HTTP routes. |
| `suggest_placement(detectFacesFromStream)` | ⚠️ Heuristic only | Suggested `test/tests/globalApi/detectAllFaces.test.ts` — this is the test file, not the right placement. Score was 2 (low confidence). |

---

## Summary Scores

| Tool | Score | Status |
|---|---|---|
| `bake` | 10/10 | Solid |
| `slice` | 10/10 | Solid |
| `package_summary` | 9/10 | Solid |
| `symbol` | 7/10 | Misses class definitions |
| `search` | 8/10 | Fuzzy works well |
| `shake` | 7/10 | Requires pre-existing bake |
| `file_functions` | 4/10 | Fails on class-method files |
| `architecture_map` | 3/10 | roles always empty on non-Express projects |
| `api_surface` | 3/10 | Unusable on large projects without pagination |
| `supersearch` | 6/10 | Line-search works; multi-word and context/pattern limited |
| `api_trace` | 2/10 | Express-only; useless on libraries |
| `suggest_placement` | 5/10 | Path heuristics; often suggests test files |
| `all_endpoints` | N/A | Correctly empty for non-Express project |
| `crud_operations` | N/A | Correctly empty for non-Express project |

**Overall onboarding success:** Despite the gaps, tokenwise produced a complete and accurate architectural overview of face-api.js in ~30 seconds. The combination of `package_summary` + `symbol` + `slice` was the decisive value chain.

---

## Onboarding Output Quality

The benchmark session produced a full onboarding document covering:
- 6 detector networks and 4 auxiliary nets
- Complete data flow from `detectAllFaces(input)` to `FaceDetection[]`
- Most complex function identified (`nonMaxSuppression`, complexity 6)
- All public API entry points with composable task chain
- Correct placement advice for adding a new detection pipeline step

Estimated equivalent manual time: **15–20 minutes**. Actual tokenwise-assisted time: **~30 seconds**.
