#!/usr/bin/env python3
"""benchmarks/report/aggregate.py

Walks a results/<run-id>/ tree and produces:
    results/<run-id>/summary.md   — markdown report (hypervisors as columns)
    results/<run-id>/plots/*.png  — bar chart per metric (optional)

Run:
    python3 aggregate.py results/<run-id>            # writes summary.md
    python3 aggregate.py results/<run-id> --plots    # also writes PNGs

`--plots` requires matplotlib; without it, only the markdown report is
produced and no extra dependencies are needed.
"""
from __future__ import annotations

import argparse
import csv
import json
import math
import sys
from pathlib import Path
from typing import Dict, List, Tuple

# ----- helpers -----

def read_summary(path: Path) -> dict:
    with path.open(newline="") as f:
        rdr = csv.DictReader(f)
        row = next(rdr, None)
    if not row:
        return {}
    return {k: float(v) for k, v in row.items()}


def discover(run_dir: Path) -> Tuple[List[str], List[str], Dict[Tuple[str, str, str], dict], Dict[str, str]]:
    """Return (hypervisors, workloads, data, units)."""
    hvs: set[str] = set()
    wls: set[str] = set()
    data: Dict[Tuple[str, str, str], dict] = {}  # (hv, wl, metric) -> summary
    units: Dict[str, str] = {}
    for hv_dir in sorted(p for p in run_dir.iterdir() if p.is_dir() and not p.name.startswith(".")):
        if hv_dir.name in {"plots"}:
            continue
        hvs.add(hv_dir.name)
        for wl_dir in sorted(p for p in hv_dir.iterdir() if p.is_dir()):
            wls.add(wl_dir.name)
            for csv_path in sorted(wl_dir.glob("*.summary.csv")):
                metric = csv_path.name[: -len(".summary.csv")]
                data[(hv_dir.name, wl_dir.name, metric)] = read_summary(csv_path)
                # unit (one-off, written by collect_sample)
                unit_path = wl_dir / "raw" / f"{metric}.unit"
                if unit_path.exists():
                    units[metric] = unit_path.read_text().strip()
    return sorted(hvs), sorted(wls), data, units


def fmt(x: float) -> str:
    if x == 0 or math.isnan(x):
        return "—"
    if abs(x) >= 1000:
        return f"{x:,.0f}"
    if abs(x) >= 1:
        return f"{x:,.2f}"
    return f"{x:.4f}"


# Higher-is-better metric heuristic.
HIB_PATTERNS = (
    "iops", "bps", "gbps", "mibps", "events_per_sec", "rate_tps", "trans/s",
    "throughput", "_per_sec",
)

LIB_PATTERNS = (
    "_ns", "_us", "_ms", "_seconds", "_latency", "rss_kib", "rss_mib",
    "_kib", "_mib", "_cold", "_warm",
)


def direction(metric: str) -> str:
    m = metric.lower()
    if any(p in m for p in HIB_PATTERNS):
        return "↑"
    if any(p in m for p in LIB_PATTERNS):
        return "↓"
    return "·"


# ----- report -----

def render_markdown(run_dir: Path, hvs: list[str], wls: list[str],
                    data: dict, units: dict) -> str:
    env_path = run_dir / "env.json"
    env = {}
    if env_path.exists():
        try:
            env = json.loads(env_path.read_text())
        except Exception:
            pass

    lines: list[str] = []
    lines.append(f"# Benchmark report: `{run_dir.name}`\n")
    if env:
        lines.append("## Environment\n")
        for k in ("uname", "kernel", "cpu", "memtotal_kib", "python"):
            if k in env:
                v = str(env[k]).splitlines()[0]
                lines.append(f"- **{k}**: `{v}`")
        if "tools" in env:
            lines.append("\n### Tool versions\n")
            for t, v in (env.get("tools") or {}).items():
                lines.append(f"- `{t}` — {v or '*not installed*'}")
        lines.append("")

    lines.append("## Results\n")
    lines.append("Legend: ↑ higher is better, ↓ lower is better, · neutral. "
                 "Values are **median** across samples; bracketed value is stdev.\n")

    for wl in wls:
        lines.append(f"### {wl}\n")
        # Gather metrics for this workload.
        metrics = sorted({m for (h, w, m) in data if w == wl})
        if not metrics:
            lines.append("_no data_\n")
            continue
        header = ["metric", "dir", "unit"] + hvs
        lines.append("| " + " | ".join(header) + " |")
        lines.append("|" + "|".join(["---"] * len(header)) + "|")
        for metric in metrics:
            row = [f"`{metric}`", direction(metric), units.get(metric, "")]
            for hv in hvs:
                s = data.get((hv, wl, metric))
                if not s:
                    row.append("—")
                else:
                    row.append(f"{fmt(s['p50'])}  ({fmt(s['stdev'])})")
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")

    return "\n".join(lines)


def render_plots(run_dir: Path, hvs: list[str], wls: list[str], data: dict) -> int:
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not available; skipping plots", file=sys.stderr)
        return 0

    plots_dir = run_dir / "plots"
    plots_dir.mkdir(exist_ok=True)
    count = 0
    metrics_by_wl: dict[str, set[str]] = {}
    for (h, w, m) in data:
        metrics_by_wl.setdefault(w, set()).add(m)
    for wl, metrics in metrics_by_wl.items():
        for metric in sorted(metrics):
            xs = hvs
            ys = [data.get((hv, wl, metric), {}).get("p50", 0.0) for hv in xs]
            errs = [data.get((hv, wl, metric), {}).get("stdev", 0.0) for hv in xs]
            fig, ax = plt.subplots(figsize=(max(4, 0.9 * len(xs) + 2), 3.5))
            ax.bar(xs, ys, yerr=errs, capsize=4)
            ax.set_title(f"{wl} — {metric}")
            ax.set_ylabel(metric)
            fig.autofmt_xdate(rotation=30)
            fig.tight_layout()
            out = plots_dir / f"{wl}__{metric}.png"
            fig.savefig(out, dpi=120)
            plt.close(fig)
            count += 1
    return count


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir", type=Path)
    ap.add_argument("--plots", action="store_true")
    args = ap.parse_args()

    if not args.run_dir.is_dir():
        print(f"not a directory: {args.run_dir}", file=sys.stderr)
        return 2

    hvs, wls, data, units = discover(args.run_dir)
    if not hvs:
        print("no hypervisor directories found", file=sys.stderr)
        return 1

    md = render_markdown(args.run_dir, hvs, wls, data, units)
    out_md = args.run_dir / "summary.md"
    out_md.write_text(md)
    print(f"wrote {out_md}")

    if args.plots:
        n = render_plots(args.run_dir, hvs, wls, data)
        print(f"wrote {n} plot(s) under {args.run_dir/'plots'}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
