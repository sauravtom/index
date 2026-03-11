#!/usr/bin/env python3
"""
tokenwise benchmark runner: latency + semantic top-3 recall.

Usage:
  python3 evals/bench_latency_semantic.py
  python3 evals/bench_latency_semantic.py --project tests/fixtures/sample_project \
      --tasks evals/tasks/tokenwise_semantic_benchmark.json --runs 7 \
      --out evals/benchmarks/phase5-baseline.json
"""

import argparse
import json
import shutil
import statistics
import subprocess
import time
from datetime import datetime
from pathlib import Path


def resolve_tokenwise_bin() -> str:
    env_bin = None
    try:
        import os
        env_bin = os.environ.get("TOKENWISE_BIN")
    except Exception:
        env_bin = None

    if env_bin:
        return env_bin

    which = shutil.which("tokenwise")
    if which:
        return which

    local = Path("target/debug/tokenwise")
    if local.exists():
        return str(local)

    raise RuntimeError(
        "Could not find tokenwise binary. Set TOKENWISE_BIN or build with `cargo build`."
    )


def run(cmd: list[str], timeout: int = 60) -> tuple[int, str, str]:
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


def ms_stats(samples: list[float]) -> dict:
    if not samples:
        return {"runs": 0, "min_ms": 0.0, "mean_ms": 0.0, "median_ms": 0.0, "p95_ms": 0.0}
    ordered = sorted(samples)
    idx = max(0, min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1)))))
    return {
        "runs": len(samples),
        "min_ms": round(min(samples), 2),
        "mean_ms": round(statistics.mean(samples), 2),
        "median_ms": round(statistics.median(samples), 2),
        "p95_ms": round(ordered[idx], 2),
    }


def benchmark_latency(tokenwise_bin: str, project: str, runs: int) -> list[dict]:
    commands = [
        ("symbol", ["symbol", "--path", project, "--name", "clamp"]),
        ("context", ["context", "--path", project, "--name", "clamp"]),
        (
            "change-impact",
            ["change-impact", "--path", project, "--files", "src/utils.rs", "--depth", "2"],
        ),
        ("cfg", ["cfg", "--path", project, "--file", "src/utils.rs", "--function", "clamp"]),
        ("dfg", ["dfg", "--path", project, "--file", "src/utils.rs", "--function", "clamp"]),
        (
            "program-slice",
            [
                "program-slice",
                "--path",
                project,
                "--file",
                "src/utils.rs",
                "--function",
                "clamp",
                "--line",
                "10",
            ],
        ),
        (
            "semantic-search",
            [
                "semantic-search",
                "--path",
                project,
                "--query",
                "limit value between min and max",
                "--limit",
                "5",
            ],
        ),
    ]

    rows = []
    for name, args in commands:
        samples = []
        for _ in range(runs):
            start = time.perf_counter()
            rc, out, err = run([tokenwise_bin, *args], timeout=60)
            elapsed_ms = (time.perf_counter() - start) * 1000.0
            if rc != 0:
                raise RuntimeError(
                    f"Latency benchmark command failed ({name}):\n"
                    f"cmd={tokenwise_bin} {' '.join(args)}\nstdout={out}\nstderr={err}"
                )
            samples.append(elapsed_ms)

        stats = ms_stats(samples)
        stats["command"] = name
        rows.append(stats)
    return rows


def semantic_top3(tokenwise_bin: str, project: str, query: str, limit: int = 5) -> list[str]:
    rc, out, err = run(
        [tokenwise_bin, "semantic-search", "--path", project, "--query", query, "--limit", str(limit)],
        timeout=60,
    )
    if rc != 0:
        raise RuntimeError(
            f"semantic-search failed for query={query!r}\nstdout={out}\nstderr={err}"
        )
    data = json.loads(out)
    return [r.get("name", "").lower() for r in data.get("results", [])[:3]]


def benchmark_semantic_recall(tokenwise_bin: str, project: str, tasks_path: str) -> dict:
    spec = json.loads(Path(tasks_path).read_text())
    tasks = spec.get("queries", [])
    if not tasks:
        raise RuntimeError(f"No queries found in semantic task file: {tasks_path}")

    per_query = []
    passed = 0
    for q in tasks:
        expected = [e.lower() for e in q.get("expected_in_top3", [])]
        top3 = semantic_top3(tokenwise_bin, project, q["query"], limit=5)
        hit = any(e in top3 for e in expected)
        if hit:
            passed += 1
        per_query.append(
            {
                "id": q["id"],
                "query": q["query"],
                "expected_in_top3": q["expected_in_top3"],
                "top3": top3,
                "pass": hit,
            }
        )

    total = len(tasks)
    pct = (100.0 * passed / total) if total else 0.0
    return {
        "top3_passed": passed,
        "top3_total": total,
        "top3_pct": round(pct, 2),
        "queries": per_query,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="tokenwise latency + semantic recall benchmark")
    parser.add_argument(
        "--project",
        default="tests/fixtures/sample_project",
        help="Project path to benchmark",
    )
    parser.add_argument(
        "--tasks",
        default="evals/tasks/tokenwise_semantic_benchmark.json",
        help="Semantic recall task file",
    )
    parser.add_argument("--runs", type=int, default=5, help="Latency runs per command")
    parser.add_argument(
        "--out",
        default="evals/results/benchmark-latency-semantic.json",
        help="Output JSON path",
    )
    args = parser.parse_args()

    tokenwise_bin = resolve_tokenwise_bin()

    # Ensure bake + embeddings exist.
    rc, out, err = run([tokenwise_bin, "warm", "--path", args.project, "--no-daemon"], timeout=120)
    if rc != 0:
        raise RuntimeError(f"warm failed:\nstdout={out}\nstderr={err}")

    latency = benchmark_latency(tokenwise_bin, args.project, max(1, args.runs))
    semantic = benchmark_semantic_recall(tokenwise_bin, args.project, args.tasks)

    payload = {
        "eval": "latency_semantic_benchmark",
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "tokenwise_bin": tokenwise_bin,
        "project": args.project,
        "runs_per_command": max(1, args.runs),
        "latency_ms": latency,
        "semantic_recall_top3": semantic,
    }

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(payload, indent=2))
    print(f"Wrote benchmark: {out_path}")


if __name__ == "__main__":
    main()
