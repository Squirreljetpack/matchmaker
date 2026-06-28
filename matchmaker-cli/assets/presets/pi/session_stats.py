#!/usr/bin/env python3
"""
pi_stats.py — aggregate usage statistics from pi session JSONL files, bucketed by (model, provider).

Usage:
    python3 pi_stats.py [--price] [--format=auto|table|yaml|json] <file.jsonl> ...
"""

import json
import os
import shutil
import sys
from collections import defaultdict


def parse_file(path):
    events = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return events


def classify_events(events, path):
    model_changes = [e for e in events if e.get("type") == "model_change"]
    default_model    = model_changes[0].get("modelId",  "unknown") if model_changes else "unknown"
    default_provider = model_changes[0].get("provider", "unknown") if model_changes else "unknown"

    model_at_time = {}
    current_model    = default_model
    current_provider = default_provider
    for e in events:
        if e.get("type") == "model_change":
            current_model    = e.get("modelId",  current_model)
            current_provider = e.get("provider", current_provider)
        model_at_time[e.get("id")] = (current_model, current_provider)

    def new_stat():
        return {
            "sessions": set(),
            "user_turns": 0,
            "assistant_turns": 0,
            "tool_calls_ok": 0,
            "tool_calls_fail": 0,
            "input": 0,
            "cache_read": 0,
            "cache_write": 0,
            "output": 0,
            "input_cost": 0.0,
            "output_cost": 0.0,
            "cost": 0.0,
            "thinking_tokens": 0,
        }

    stats = defaultdict(new_stat)
    session_id = next((e.get("id") for e in events if e.get("type") == "session"), path)

    for e in events:
        if e.get("type") != "message":
            continue

        msg  = e.get("message", {})
        role = msg.get("role")

        model_name = msg.get("model") or model_at_time.get(e.get("id"), (default_model, default_provider))[0]
        provider   = model_at_time.get(e.get("id"), (model_name, default_provider))[1]
        
        combo_key = (model_name, provider)
        s = stats[combo_key]
        s["sessions"].add(session_id)

        if role == "user":
            s["user_turns"] += 1

        elif role == "assistant":
            s["assistant_turns"] += 1

            usage = msg.get("usage", {})
            s["input"]       += usage.get("input",      0)
            s["cache_read"]  += usage.get("cacheRead",  0)
            s["cache_write"] += usage.get("cacheWrite", 0)
            s["output"]      += usage.get("output",     0)

            cost = usage.get("cost") or {}
            if isinstance(cost, dict):
                s["input_cost"]  += cost.get("input", 0) + cost.get("cacheRead", 0) + cost.get("cacheWrite", 0)
                s["output_cost"] += cost.get("output", 0)
                s["cost"]        += cost.get("total",  0)
            elif isinstance(cost, (int, float)):
                s["cost"] += cost

            for block in msg.get("content", []):
                if block.get("type") == "thinking":
                    s["thinking_tokens"] += len(block.get("thinking", "")) // 4
                elif block.get("type") == "toolCall":
                    s["tool_calls_ok"] += 1

        elif role == "toolResult":
            model_name2, provider2 = model_at_time.get(e.get("id"), (default_model, default_provider))
            combo_key2 = (model_name2, provider2)
            if msg.get("isError", False):
                s2 = stats[combo_key2]
                s2["tool_calls_fail"] += 1
                s2["tool_calls_ok"]    = max(0, s2["tool_calls_ok"] - 1)

    return stats


def merge_stats(all_stats):
    def new_stat():
        return {
            "sessions": set(),
            "user_turns": 0,
            "assistant_turns": 0,
            "tool_calls_ok": 0,
            "tool_calls_fail": 0,
            "input": 0,
            "cache_read": 0,
            "cache_write": 0,
            "output": 0,
            "input_cost": 0.0,
            "output_cost": 0.0,
            "cost": 0.0,
            "thinking_tokens": 0,
        }

    merged = defaultdict(new_stat)
    for stats in all_stats:
        for combo_key, s in stats.items():
            m = merged[combo_key]
            m["sessions"]        |= s["sessions"]
            m["user_turns"]      += s["user_turns"]
            m["assistant_turns"] += s["assistant_turns"]
            m["tool_calls_ok"]   += s["tool_calls_ok"]
            m["tool_calls_fail"] += s["tool_calls_fail"]
            m["input"]           += s["input"]
            m["cache_read"]      += s["cache_read"]
            m["cache_write"]     += s["cache_write"]
            m["output"]          += s["output"]
            m["input_cost"]      += s["input_cost"]
            m["output_cost"]     += s["output_cost"]
            m["cost"]            += s["cost"]
            m["thinking_tokens"] += s["thinking_tokens"]
    return merged


def fmt_int(n):
    return f"{n:,}"

def fmt_pct(num, denom):
    if denom == 0:
        return "—"
    return f"{num * 100 / denom:.1f}%"

def fmt_cost(c):
    if c == 0:
        return "—"
    return f"{c:.2f}"

def fmt_k(n):
    if n >= 1_000_000:
        return f"{n/1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n/1_000:.1f}k"
    return str(n)


def build_columns(show_price):
    def total_input(s):
        return s["input"] + s["cache_read"] + s["cache_write"]

    base = [
        ("#S",      lambda m, s: fmt_int(len(s["sessions"]))),
        ("#U",       lambda m, s: fmt_int(s["user_turns"])),
        ("#A",       lambda m, s: fmt_int(s["assistant_turns"])),
        ("✓",          lambda m, s: fmt_int(s["tool_calls_ok"])),
        ("✗",          lambda m, s: fmt_int(s["tool_calls_fail"])),
        ("Input",      lambda m, s: fmt_k(total_input(s))),
        ("C R%",   lambda m, s: fmt_pct(s["cache_read"],  total_input(s))),
        ("C W%",   lambda m, s: fmt_pct(s["cache_write"], total_input(s))),
        ("Output",     lambda m, s: fmt_k(s["output"])),
        ("Think",     lambda m, s: fmt_k(s["thinking_tokens"]) if s["thinking_tokens"] else "—"),
    ]
    
    if show_price:
        # Rate per 1M tokens: (Cost / Tokens) * 1,000,000
        base += [
            ("$/in",  lambda m, s: fmt_cost((s["input_cost"] / total_input(s)) * 1_000_000) if total_input(s) else "—"),
            ("$/out", lambda m, s: fmt_cost((s["output_cost"] / s["output"]) * 1_000_000) if s["output"] else "—"),
            ("Cost",    lambda m, s: fmt_cost(s["cost"])),
        ]
    else:
        # Absolute total spending columns
        base += [
            ("in $",  lambda m, s: fmt_cost(s["input_cost"])),
            ("out $", lambda m, s: fmt_cost(s["output_cost"])),
            ("Cost",    lambda m, s: fmt_cost(s["cost"])),
        ]
        
    return base


def build_total(merged):
    return {
        "sessions":        set().union(*(s["sessions"] for s in merged.values())),
        "user_turns":      sum(s["user_turns"]      for s in merged.values()),
        "assistant_turns": sum(s["assistant_turns"] for s in merged.values()),
        "tool_calls_ok":   sum(s["tool_calls_ok"]   for s in merged.values()),
        "tool_calls_fail": sum(s["tool_calls_fail"] for s in merged.values()),
        "input":           sum(s["input"]           for s in merged.values()),
        "cache_read":      sum(s["cache_read"]      for s in merged.values()),
        "cache_write":     sum(s["cache_write"]     for s in merged.values()),
        "output":          sum(s["output"]          for s in merged.values()),
        "input_cost":      sum(s["input_cost"]      for s in merged.values()),
        "output_cost":     sum(s["output_cost"]     for s in merged.values()),
        "cost":            sum(s["cost"]            for s in merged.values()),
        "thinking_tokens": sum(s["thinking_tokens"] for s in merged.values()),
    }


def render_table(merged, show_price=False):
    columns = build_columns(show_price)
    show_total = len(merged) > 1

    model_rows = sorted(merged.items())
    data_rows = [(mk, s) for mk, s in model_rows]
    if show_total:
        data_rows.append(("TOTAL", build_total(merged)))

    all_cols = [("Model", lambda m, s: f"{m[0]} ({m[1]})" if isinstance(m, tuple) else m)] + columns
    all_headers = [h for h, _ in all_cols]

    rows = [[fn(mk, s) for _, fn in all_cols] for mk, s in data_rows]

    widths = [len(h) for h in all_headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))

    def bar(left, mid, right, fill="─"):
        return left + mid.join(fill * (w + 2) for w in widths) + right

    def row_str(cells):
        parts = [f" {cell:<{w}} " for cell, w in zip(cells, widths)]
        return "│" + "│".join(parts) + "│"

    top     = bar("┌", "┬", "┐")
    headsep = bar("├", "┼", "┤")
    midsep  = bar("├", "┼", "┤")
    botsep  = bar("└", "┴", "┘")

    lines = [top, row_str(all_headers), headsep]
    for i, (row, (mk, _)) in enumerate(zip(rows, data_rows)):
        if show_total and i == len(rows) - 1:
            lines.append(midsep)
        lines.append(row_str(row))
    lines.append(botsep)

    return "\n".join(lines)


def build_output_list(merged, show_price=False):
    columns = build_columns(show_price)
    show_total = len(merged) > 1

    rows = sorted(merged.items())
    
    out_list = []
    for mk, s in rows:
        model_name, provider = mk
        metrics = {"Provider": provider}
        for header, fn in columns:
            metrics[header] = fn(mk, s)
        out_list.append({model_name: metrics})

    if show_total:
        total_metrics = {"Provider": ""}
        for header, fn in columns:
            total_metrics[header] = fn("TOTAL", build_total(merged))
        out_list.append({"TOTAL": total_metrics})

    return out_list


def render_json(merged, show_price=False):
    return json.dumps(
        build_output_list(merged, show_price),
        indent=2,
        ensure_ascii=False,
    )


def render_yaml(merged, show_price=False):
    out = []
    for entry in build_output_list(merged, show_price):
        for model, values in entry.items():
            first = True
            for key, value in values.items():
                if value == "":
                    value = '""'
                if first:
                    out.append(f"- {model}:")
                    first = False
                out.append(f"    {key}: {value}")
    return "\n".join(out)


def terminal_columns():
    try:
        return int(os.environ["COLUMNS"])
    except (KeyError, ValueError):
        return shutil.get_terminal_size((80, 24)).columns


def main():
    import argparse
    parser = argparse.ArgumentParser(
        prog="pi_stats.py",
        description="Aggregate usage statistics from pi session JSONL files, bucketed by model and provider.",
    )
    parser.add_argument("files", nargs="+", metavar="FILE", help="Session JSONL files to process")
    parser.add_argument("--price", action="store_true", help="Show price instead of cost ($ / in, $ / out)")
    parser.add_argument(
        "--format",
        choices=["auto", "table", "yaml", "json"],
        default="auto",
        metavar="auto|table|yaml|json",
        help="Output format (default: auto — falls back to YAML if the table exceeds terminal width)",
    )
    args = parser.parse_args()
    show_price = args.price
    fmt        = args.format
    paths      = args.files

    all_stats = []
    errors = []
    for path in paths:
        try:
            events = parse_file(path)
            all_stats.append(classify_events(events, path))
        except Exception as e:
            errors.append(f"  {path}: {e}")

    if errors:
        print("Skipped files:", file=sys.stderr)
        for e in errors:
            print(e, file=sys.stderr)

    if not all_stats:
        print("No valid files processed.", file=sys.stderr)
        sys.exit(1)

    merged = merge_stats(all_stats)

    if fmt == "json":
        print(render_json(merged, show_price=show_price))
    elif fmt == "yaml":
        print(render_yaml(merged, show_price=show_price))
    elif fmt == "table":
        print(render_table(merged, show_price=show_price))
    else:  # auto
        table = render_table(merged, show_price=show_price)
        width = max((len(line) for line in table.splitlines()), default=0)
        if width > terminal_columns():
            print(render_yaml(merged, show_price=show_price))
        else:
            print(table)

    print(f"\n{len(paths) - len(errors)} file(s) processed, {len(errors)} skipped.", file=sys.stderr)


if __name__ == "__main__":
    main()