#!/usr/bin/env python3
"""
Generate benchmark comparison Markdown with Mermaid charts.

Reads summary.json from each mode directory and creates a comparison report.
Modes are auto-detected from subdirectories containing summary.json.

Usage:
    # Default: build/benchmarks/
    python3 generate_benchmark_report.py

    # Custom directory (e.g., after matrix benchmark):
    python3 generate_benchmark_report.py --dir build/benchmarks_d2s
    python3 generate_benchmark_report.py --dir build/benchmarks_d8s
"""

import argparse
import json
import os
from pathlib import Path
from datetime import datetime

# Default benchmarks directory
DEFAULT_BENCHMARKS_DIR = Path(__file__).parent.parent.parent / "build" / "benchmarks"

# Will be set by main() based on args
BENCHMARKS_DIR: Path = DEFAULT_BENCHMARKS_DIR

# Preferred order for display (modes not in this list appear at the end alphabetically)
MODE_ORDER = ["dpdk", "tokio", "tokio-local", "kimojio", "kimojio-poll"]

# Color palette for charts
CHART_COLORS = ["#3366cc", "#ff9900", "#33cc33", "#9933ff", "#cc3366", "#33cccc", "#cc6633"]
COLOR_NAMES = ["blue", "orange", "green", "purple", "pink", "cyan", "brown"]


def discover_modes() -> list[str]:
    """Discover available modes by scanning for directories with summary.json."""
    if not BENCHMARKS_DIR.exists():
        return []
    
    modes = []
    for entry in BENCHMARKS_DIR.iterdir():
        if entry.is_dir() and (entry / "summary.json").exists():
            modes.append(entry.name)
    
    # Sort: known modes first in preferred order, then unknown modes alphabetically
    def sort_key(mode):
        if mode in MODE_ORDER:
            return (0, MODE_ORDER.index(mode))
        return (1, mode)
    
    return sorted(modes, key=sort_key)


def load_summary(mode: str) -> dict | None:
    """Load summary.json for a given mode."""
    summary_path = BENCHMARKS_DIR / mode / "summary.json"
    if not summary_path.exists():
        return None
    with open(summary_path) as f:
        return json.load(f)


def get_results_by_connections(summary: dict) -> dict:
    """Index results by connection count."""
    return {r["connections"]: r for r in summary["results"]}


def get_chart_colors(num_modes: int) -> str:
    """Get comma-separated color palette for the given number of modes."""
    return ", ".join(CHART_COLORS[:num_modes])


def add_legend(modes: list[str]) -> list[str]:
    """Generate legend lines for the given modes."""
    legend_items = [f"{m} ({COLOR_NAMES[i % len(COLOR_NAMES)]})" for i, m in enumerate(modes)]
    return ["", "**Legend:** " + " | ".join(legend_items), ""]


def generate_markdown() -> str:
    """Generate the comparison Markdown content."""
    # Discover and load all summaries
    modes = discover_modes()
    if not modes:
        return "# Benchmark Comparison\n\nNo benchmark data found.\n"
    
    summaries = {}
    for mode in modes:
        summary = load_summary(mode)
        if summary:
            summaries[mode] = get_results_by_connections(summary)

    if not summaries:
        return "# Benchmark Comparison\n\nNo benchmark data found.\n"

    # Get all connection counts (sorted)
    all_connections = sorted(
        set(c for results in summaries.values() for c in results.keys())
    )

    chart_colors = get_chart_colors(len(modes))

    # Build markdown
    lines = [
        "# Benchmark Comparison",
        "",
        f"Generated: {datetime.now().isoformat()}",
        "",
        f"Modes tested: {', '.join(modes)}",
        "",
        "## Summary",
        "",
        "| Mode | Connections | Requests/sec | MB/sec | p50 (μs) | p99 (μs) | Errors |",
        "|------|-------------|--------------|--------|----------|----------|--------|",
    ]

    for mode in modes:
        if mode not in summaries:
            continue
        for conn in all_connections:
            if conn not in summaries[mode]:
                continue
            r = summaries[mode][conn]
            lat = r.get("latency", {})
            lines.append(
                f"| {mode} | {conn} | {r['requests_per_sec']:.0f} | {r['mb_per_sec']:.1f} | "
                f"{lat.get('p50_us', 'N/A')} | {lat.get('p99_us', 'N/A')} | {r['errors']} |"
            )

    # Throughput chart
    lines.extend([
        "",
        "## Throughput Comparison",
        "",
        "```mermaid",
        "---",
        "config:",
        "    themeVariables:",
        "        xyChart:",
        f'            plotColorPalette: "{chart_colors}"',
        "---",
        "xychart-beta",
        '    title "Requests per Second by Connection Count"',
        f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
    ])

    # Find max for y-axis
    max_rps = max(
        r["requests_per_sec"]
        for results in summaries.values()
        for r in results.values()
    )
    y_max = int(max_rps * 1.1)
    lines.append(f'    y-axis "Requests/sec" 0 --> {y_max}')

    for mode in modes:
        if mode not in summaries:
            continue
        values = [
            str(int(summaries[mode].get(c, {}).get("requests_per_sec", 0)))
            for c in all_connections
        ]
        lines.append(f'    line "{mode}" [{", ".join(values)}]')

    lines.append("```")
    lines.extend(add_legend(modes))

    # Bandwidth chart (MB/sec)
    lines.extend([
        "",
        "## Bandwidth Comparison",
        "",
        "```mermaid",
        "---",
        "config:",
        "    themeVariables:",
        "        xyChart:",
        f'            plotColorPalette: "{chart_colors}"',
        "---",
        "xychart-beta",
        '    title "MB per Second by Connection Count"',
        f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
    ])

    max_mbps = max(
        r["mb_per_sec"]
        for results in summaries.values()
        for r in results.values()
    )
    y_max_mbps = int(max_mbps * 1.1)
    lines.append(f'    y-axis "MB/sec" 0 --> {y_max_mbps}')

    for mode in modes:
        if mode not in summaries:
            continue
        values = [
            str(int(summaries[mode].get(c, {}).get("mb_per_sec", 0)))
            for c in all_connections
        ]
        lines.append(f'    line "{mode}" [{", ".join(values)}]')

    lines.append("```")
    lines.extend(add_legend(modes))

    # DPDK improvement percentage helper
    def calc_improvement(dpdk_val, other_val):
        """Calculate percentage improvement of DPDK over other mode."""
        if other_val == 0:
            return 0
        return ((dpdk_val - other_val) / other_val) * 100

    # Throughput improvement chart (DPDK vs others)
    if "dpdk" in summaries:
        other_modes = [m for m in modes if m != "dpdk" and m in summaries]
        if other_modes:
            improvement_colors = get_chart_colors(len(other_modes))
            lines.extend([
                "",
                "## DPDK Throughput Improvement",
                "",
                "Percentage improvement of DPDK over other modes (positive = DPDK is faster).",
                "",
                "```mermaid",
                "---",
                "config:",
                "    themeVariables:",
                "        xyChart:",
                f'            plotColorPalette: "{improvement_colors}"',
                "---",
                "xychart-beta",
                '    title "DPDK Throughput Improvement (%)"',
                f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
            ])

            # Calculate min/max for y-axis
            all_improvements = []
            for other_mode in other_modes:
                for c in all_connections:
                    dpdk_val = summaries["dpdk"].get(c, {}).get("requests_per_sec", 0)
                    other_val = summaries[other_mode].get(c, {}).get("requests_per_sec", 0)
                    all_improvements.append(calc_improvement(dpdk_val, other_val))

            y_min = int(min(all_improvements) - 10)
            y_max = int(max(all_improvements) + 10)
            lines.append(f'    y-axis "Improvement (%)" {y_min} --> {y_max}')

            for other_mode in other_modes:
                values = []
                for c in all_connections:
                    dpdk_val = summaries["dpdk"].get(c, {}).get("requests_per_sec", 0)
                    other_val = summaries[other_mode].get(c, {}).get("requests_per_sec", 0)
                    improvement = calc_improvement(dpdk_val, other_val)
                    values.append(str(int(improvement)))
                lines.append(f'    line "vs {other_mode}" [{", ".join(values)}]')

            lines.append("```")
            # Dynamic legend for improvement chart
            improvement_legend_items = [
                f"vs {m} ({COLOR_NAMES[i % len(COLOR_NAMES)]})"
                for i, m in enumerate(other_modes)
            ]
            lines.extend([
                "",
                "**Legend:** " + " | ".join(improvement_legend_items),
                "",
            ])

    # Latency p50 chart
    lines.extend([
        "",
        "## Latency Comparison (p50)",
        "",
        "```mermaid",
        "---",
        "config:",
        "    themeVariables:",
        "        xyChart:",
        f'            plotColorPalette: "{chart_colors}"',
        "---",
        "xychart-beta",
        '    title "p50 Latency by Connection Count"',
        f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
    ])

    max_p50 = max(
        r.get("latency", {}).get("p50_us", 0)
        for results in summaries.values()
        for r in results.values()
    )
    y_max_lat = int(max_p50 * 1.2)
    lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_lat}')

    for mode in modes:
        if mode not in summaries:
            continue
        values = [
            str(summaries[mode].get(c, {}).get("latency", {}).get("p50_us", 0))
            for c in all_connections
        ]
        lines.append(f'    line "{mode}" [{", ".join(values)}]')

    lines.append("```")
    lines.extend(add_legend(modes))

    # Latency p50 chart (low connections - first 4 points)
    low_connections = all_connections[:4]
    if len(low_connections) > 1:
        lines.extend([
            "",
            "### p50 Latency (Low Connections)",
            "",
            "```mermaid",
            "---",
            "config:",
            "    themeVariables:",
            "        xyChart:",
            f'            plotColorPalette: "{chart_colors}"',
            "---",
            "xychart-beta",
            '    title "p50 Latency (Low Connection Counts)"',
            f'    x-axis "Connections" [{", ".join(str(c) for c in low_connections)}]',
        ])

        max_p50_low = max(
            summaries[mode].get(c, {}).get("latency", {}).get("p50_us", 0)
            for mode in modes if mode in summaries
            for c in low_connections
        )
        y_max_lat_low = int(max_p50_low * 1.2) if max_p50_low > 0 else 100
        lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_lat_low}')

        for mode in modes:
            if mode not in summaries:
                continue
            values = [
                str(summaries[mode].get(c, {}).get("latency", {}).get("p50_us", 0))
                for c in low_connections
            ]
            lines.append(f'    line "{mode}" [{", ".join(values)}]')

        lines.append("```")
        lines.extend(add_legend(modes))

        # Calculate DPDK latency improvement at last low-connection point
        if "dpdk" in summaries and len(low_connections) > 0:
            last_conn = low_connections[-1]
            dpdk_lat = summaries["dpdk"].get(last_conn, {}).get("latency", {}).get("p50_us", 0)
            other_lats = [
                summaries[m].get(last_conn, {}).get("latency", {}).get("p50_us", 0)
                for m in modes if m != "dpdk" and m in summaries
            ]
            if other_lats and dpdk_lat > 0:
                best_other = min(lat for lat in other_lats if lat > 0) if any(lat > 0 for lat in other_lats) else 0
                if best_other > 0:
                    improvement = ((best_other - dpdk_lat) / best_other) * 100
                    lines.append("")
                    lines.append(f"**DPDK p50 latency improvement at {last_conn} connections: {improvement:+.1f}%** (positive = DPDK is faster)")

    # Latency p90 chart
    lines.extend([
        "",
        "## Latency Comparison (p90)",
        "",
        "```mermaid",
        "---",
        "config:",
        "    themeVariables:",
        "        xyChart:",
        f'            plotColorPalette: "{chart_colors}"',
        "---",
        "xychart-beta",
        '    title "p90 Latency by Connection Count"',
        f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
    ])

    max_p90 = max(
        r.get("latency", {}).get("p90_us", 0)
        for results in summaries.values()
        for r in results.values()
    )
    y_max_p90 = int(max_p90 * 1.2)
    lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_p90}')

    for mode in modes:
        if mode not in summaries:
            continue
        values = [
            str(summaries[mode].get(c, {}).get("latency", {}).get("p90_us", 0))
            for c in all_connections
        ]
        lines.append(f'    line "{mode}" [{", ".join(values)}]')

    lines.append("```")
    lines.extend(add_legend(modes))

    # Latency p90 chart (low connections - first 4 points)
    if len(low_connections) > 1:
        lines.extend([
            "",
            "### p90 Latency (Low Connections)",
            "",
            "```mermaid",
            "---",
            "config:",
            "    themeVariables:",
            "        xyChart:",
            f'            plotColorPalette: "{chart_colors}"',
            "---",
            "xychart-beta",
            '    title "p90 Latency (Low Connection Counts)"',
            f'    x-axis "Connections" [{", ".join(str(c) for c in low_connections)}]',
        ])

        max_p90_low = max(
            summaries[mode].get(c, {}).get("latency", {}).get("p90_us", 0)
            for mode in modes if mode in summaries
            for c in low_connections
        )
        y_max_p90_low = int(max_p90_low * 1.2) if max_p90_low > 0 else 100
        lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_p90_low}')

        for mode in modes:
            if mode not in summaries:
                continue
            values = [
                str(summaries[mode].get(c, {}).get("latency", {}).get("p90_us", 0))
                for c in low_connections
            ]
            lines.append(f'    line "{mode}" [{", ".join(values)}]')

        lines.append("```")
        lines.extend(add_legend(modes))

        # Calculate DPDK latency improvement at last low-connection point
        if "dpdk" in summaries and len(low_connections) > 0:
            last_conn = low_connections[-1]
            dpdk_lat = summaries["dpdk"].get(last_conn, {}).get("latency", {}).get("p90_us", 0)
            other_lats = [
                summaries[m].get(last_conn, {}).get("latency", {}).get("p90_us", 0)
                for m in modes if m != "dpdk" and m in summaries
            ]
            if other_lats and dpdk_lat > 0:
                best_other = min(lat for lat in other_lats if lat > 0) if any(lat > 0 for lat in other_lats) else 0
                if best_other > 0:
                    improvement = ((best_other - dpdk_lat) / best_other) * 100
                    lines.append("")
                    lines.append(f"**DPDK p90 latency improvement at {last_conn} connections: {improvement:+.1f}%** (positive = DPDK is faster)")

    # Latency p99 chart
    lines.extend([
        "",
        "## Latency Comparison (p99)",
        "",
        "```mermaid",
        "---",
        "config:",
        "    themeVariables:",
        "        xyChart:",
        f'            plotColorPalette: "{chart_colors}"',
        "---",
        "xychart-beta",
        '    title "p99 Latency by Connection Count"',
        f'    x-axis "Connections" [{", ".join(str(c) for c in all_connections)}]',
    ])

    max_p99 = max(
        r.get("latency", {}).get("p99_us", 0)
        for results in summaries.values()
        for r in results.values()
    )
    y_max_p99 = int(max_p99 * 1.2)
    lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_p99}')

    for mode in modes:
        if mode not in summaries:
            continue
        values = [
            str(summaries[mode].get(c, {}).get("latency", {}).get("p99_us", 0))
            for c in all_connections
        ]
        lines.append(f'    line "{mode}" [{", ".join(values)}]')

    lines.append("```")
    lines.extend(add_legend(modes))

    # Latency p99 chart (low connections - first 4 points)
    if len(low_connections) > 1:
        lines.extend([
            "",
            "### p99 Latency (Low Connections)",
            "",
            "```mermaid",
            "---",
            "config:",
            "    themeVariables:",
            "        xyChart:",
            f'            plotColorPalette: "{chart_colors}"',
            "---",
            "xychart-beta",
            '    title "p99 Latency (Low Connection Counts)"',
            f'    x-axis "Connections" [{", ".join(str(c) for c in low_connections)}]',
        ])

        max_p99_low = max(
            summaries[mode].get(c, {}).get("latency", {}).get("p99_us", 0)
            for mode in modes if mode in summaries
            for c in low_connections
        )
        y_max_p99_low = int(max_p99_low * 1.2) if max_p99_low > 0 else 100
        lines.append(f'    y-axis "Latency (μs)" 0 --> {y_max_p99_low}')

        for mode in modes:
            if mode not in summaries:
                continue
            values = [
                str(summaries[mode].get(c, {}).get("latency", {}).get("p99_us", 0))
                for c in low_connections
            ]
            lines.append(f'    line "{mode}" [{", ".join(values)}]')

        lines.append("```")
        lines.extend(add_legend(modes))

        # Calculate DPDK latency improvement at last low-connection point
        if "dpdk" in summaries and len(low_connections) > 0:
            last_conn = low_connections[-1]
            dpdk_lat = summaries["dpdk"].get(last_conn, {}).get("latency", {}).get("p99_us", 0)
            other_lats = [
                summaries[m].get(last_conn, {}).get("latency", {}).get("p99_us", 0)
                for m in modes if m != "dpdk" and m in summaries
            ]
            if other_lats and dpdk_lat > 0:
                best_other = min(lat for lat in other_lats if lat > 0) if any(lat > 0 for lat in other_lats) else 0
                if best_other > 0:
                    improvement = ((best_other - dpdk_lat) / best_other) * 100
                    lines.append("")
                    lines.append(f"**DPDK p99 latency improvement at {last_conn} connections: {improvement:+.1f}%** (positive = DPDK is faster)")

    # Raw data section
    lines.extend([
        "",
        "## Raw Data",
        "",
    ])

    for mode in modes:
        if mode not in summaries:
            continue
        lines.append(f"### {mode}")
        lines.append("")
        lines.append("<details>")
        lines.append("<summary>Click to expand</summary>")
        lines.append("")
        lines.append("```json")
        summary = load_summary(mode)
        lines.append(json.dumps(summary, indent=2))
        lines.append("```")
        lines.append("")
        lines.append("</details>")
        lines.append("")

    return "\n".join(lines)


def main():
    """Main entry point."""
    global BENCHMARKS_DIR
    
    parser = argparse.ArgumentParser(
        description="Generate benchmark comparison Markdown with Mermaid charts."
    )
    parser.add_argument(
        "--dir", "-d",
        type=Path,
        default=DEFAULT_BENCHMARKS_DIR,
        help="Benchmarks directory containing mode subdirectories (default: build/benchmarks/)"
    )
    parser.add_argument(
        "--output", "-o",
        type=Path,
        default=None,
        help="Output file path (default: <dir>/BENCHMARK_<dirname>.md)"
    )
    args = parser.parse_args()
    
    BENCHMARKS_DIR = args.dir.resolve()
    
    # Default output filename based on directory name
    if args.output:
        output_file = args.output
    else:
        dir_name = BENCHMARKS_DIR.name
        # Extract suffix like "d2s" from "benchmarks_d2s", otherwise use full name
        if dir_name.startswith("benchmarks_"):
            suffix = dir_name[len("benchmarks_"):]
            output_file = BENCHMARKS_DIR / f"BENCHMARK_{suffix}.md"
        elif dir_name == "benchmarks":
            output_file = BENCHMARKS_DIR / "BENCHMARK_COMPARISON.md"
        else:
            output_file = BENCHMARKS_DIR / f"BENCHMARK_{dir_name}.md"
    
    if not BENCHMARKS_DIR.exists():
        print(f"Error: Benchmarks directory not found: {BENCHMARKS_DIR}")
        return 1

    content = generate_markdown()

    with open(output_file, "w") as f:
        f.write(content)

    print(f"Generated: {output_file}")
    return 0


if __name__ == "__main__":
    exit(main())
