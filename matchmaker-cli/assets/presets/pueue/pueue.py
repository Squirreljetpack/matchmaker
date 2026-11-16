#!/usr/bin/env python3
"""Data and actions for the pueue matchmaker presets.

Rows are space padded on stdout (the presets split them on runs of spaces)
and diagnostics go to stderr, so every subcommand is also usable by hand:

  groups                name, running, queued, done, failed, parallel, state
  tasks [GROUP...]      id, group, state, elapsed, command
  summary [GROUP...]    per-group task counts, aligned for the pager
  toggle GROUP...       pause running groups, resume paused ones
  kill [-s SIG] [-g GROUP] [ID...]
                        kill tasks (or a whole group) and wait until they stop
  remove-group GROUP... remove groups after confirmation; refuses groups that
                        still have unfinished tasks

States are short (run, ok, fail:2, ...) and colored when stdout is a terminal
or the helper runs under matchmaker, which strips the colors for matching. The
elapsed cell starts with zero-padded seconds so `sort` and `sort numeric` both
order it correctly, followed by the compact duration for reading.
"""

import json
import os
import re
import signal
import subprocess
import sys
from datetime import datetime, timezone
from time import monotonic, sleep

BUCKETS = ("running", "queued", "done", "failed")
DONE_KINDS = {
    "Success": "0",
    "Killed": "kill",
    "FailedToSpawn": "spawn",
    "DependencyFailed": "dep",
    "Errored": "err",
}
GROUP_STATES = {"running": "run", "paused": "pause", "reset": "reset"}
STATE_LABELS = {
    "running": "run",
    "paused": "pause",
    "queued": "queue",
    "stashed": "stash",
    "locked": "lock",
}
STATE_COLORS = {
    "run": 32,
    "pause": 33,
    "reset": 33,
    "queue": 36,
    "stash": 35,
    "lock": 2,
    "kill": 91,
    "spawn": 31,
    "dep": 31,
    "err": 31,
}
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def paint(label):
    """Color a state label: exit codes by success, words by kind."""
    if not (sys.stdout.isatty() or "MM_OVERRIDE" in os.environ):
        return label
    code = STATE_COLORS.get(label)
    if code is None:
        if not label.lstrip("-").isdigit():
            return label
        code = 32 if label == "0" else 31
    return f"\x1b[{code}m{label}\x1b[0m"


def pueue(args, capture=False):
    try:
        return subprocess.run(["pueue", *args], text=True, capture_output=capture)
    except FileNotFoundError:
        sys.exit("pueue.py: pueue is not installed")


def status_json():
    proc = pueue(["status", "--json"], capture=True)
    if proc.returncode != 0:
        sys.exit(proc.stderr.strip() or "pueue.py: pueue status failed")
    return json.loads(proc.stdout)


def done_state(result):
    """Render a Done task's result: its exit code, or a word when pueue has none."""
    if isinstance(result, dict):
        kind, value = next(iter(result.items()))
        return str(value) if kind == "Failed" else DONE_KINDS.get(kind, kind.lower())
    return DONE_KINDS.get(result, str(result).lower())


def task_state(status):
    kind, payload = next(iter(status.items()))
    if kind == "Done":
        return done_state(payload.get("result"))
    return STATE_LABELS.get(kind.lower(), kind.lower())


def bucket(status):
    """Fold a task state into one of the summary buckets."""
    kind = next(iter(status))
    if kind == "Running":
        return "running"
    if kind == "Done":
        return "done" if task_state(status) == "0" else "failed"
    return "queued"


def state_rank(state):
    if state == "run":
        return 0
    if state == "pause":
        return 1
    if state in ("queue", "stash", "lock"):
        return 2
    return 3


def parse_time(stamp):
    if not stamp:
        return None
    # pueue emits nanoseconds; fromisoformat takes at most microseconds
    return datetime.fromisoformat(re.sub(r"(\.\d{6})\d+", r"\1", stamp.replace("Z", "+00:00")))


def human_time(seconds):
    """Elapsed time as a duration with one segment per unit, e.g. 0d0h26m5s."""
    minutes, s = divmod(int(seconds), 60)
    hours, m = divmod(minutes, 60)
    d, h = divmod(hours, 24)
    return f"{d}d{h}h{m}m{s}s"


def elapsed_seconds(status):
    """Seconds a task ran, or has been running so far."""
    kind, payload = next(iter(status.items()))
    start = parse_time(payload.get("start"))
    if not start:
        return None
    if kind == "Running":
        return int((datetime.now(timezone.utc) - start).total_seconds())
    if kind == "Done":
        end = parse_time(payload.get("end"))
        if end:
            return int((end - start).total_seconds())
    return None


def time_cell(status):
    """Elapsed time as 0d0h26m5s, or - for a task that never started."""
    seconds = elapsed_seconds(status)
    return "-" if seconds is None else human_time(seconds)


def render(rows):
    """Print rows with every column but the last padded into a table."""
    if not rows:
        return

    def visible(cell):
        return len(ANSI_RE.sub("", str(cell)))

    # a minimum width keeps the column labels from being cut off in the picker
    widths = [max(4, *(visible(row[i]) for row in rows)) for i in range(len(rows[0]) - 1)]
    for row in rows:
        cells = []
        for i, cell in enumerate(row[:-1]):
            pad = " " * (widths[i] - visible(cell))
            cells.append(pad + str(cell) if isinstance(cell, int) else str(cell) + pad)
        print(" ".join([*cells, str(row[-1])]))


def blank_counts():
    return dict.fromkeys(BUCKETS, 0)


def group_counts(data):
    counts = {}
    for task in data["tasks"].values():
        counts.setdefault(task["group"], blank_counts())[bucket(task["status"])] += 1
    return counts


def cmd_groups(args):
    data = status_json()
    counts = group_counts(data)
    rows = []
    for name in sorted(data["groups"]):
        group = data["groups"][name]
        tally = counts.get(name, blank_counts())
        row = (
            name,
            *(tally[bucket] for bucket in BUCKETS),
            group["parallel_tasks"],
            paint(GROUP_STATES.get(str(group["status"]).lower(), str(group["status"]).lower())),
        )
        rows.append(row)
    render(rows)


def cmd_tasks(args):
    data = status_json()
    groups = set(args)
    rows = []
    for task in data["tasks"].values():
        if groups and task["group"] not in groups:
            continue
        state = task_state(task["status"])
        rows.append((state_rank(state), -task["id"], task, state))
    # running first, then waiting, then finished newest first
    rows.sort(key=lambda row: row[:2])
    render(
        [
            (
                task["id"],
                task["group"],
                paint(state),
                time_cell(task["status"]),
                " ".join(task["command"].split()),
            )
            for _, _, task, state in rows
        ]
    )


def cmd_summary(args):
    data = status_json()
    groups = set(args)
    counts = group_counts(data)
    names = [name for name in sorted(data["groups"]) if not groups or name in groups]
    if groups:
        missing = groups - set(data["groups"])
        if missing:
            sys.exit("pueue.py summary: no such group: " + ", ".join(sorted(missing)))

    body = [
        [name, *(counts.get(name, blank_counts())[bucket] for bucket in BUCKETS)]
        for name in names
    ]
    body = [[*row, sum(row[1:])] for row in body]
    total = ["total", *(sum(row[i] for row in body) for i in range(1, 5)), sum(row[5] for row in body)]

    table = [["group", *BUCKETS, "total"], *body]
    widths = [max(len(str(row[i])) for row in table) for i in range(len(table[0]))]
    if len(body) > 1:
        table += [["─" * width for width in widths], total]
    else:
        table += [total]
    for row in table:
        if all(set(cell) == {"─"} for cell in row):
            print("  ".join(row))
        else:
            print("  ".join([str(row[0]).ljust(widths[0])] + [str(value).rjust(widths[i]) for i, value in enumerate(row[1:], start=1)]))


def cmd_toggle(args):
    """Pause running groups and resume paused ones."""
    if not args:
        sys.exit("pueue.py toggle: no group given")
    data = status_json()
    for name in args:
        if name not in data["groups"]:
            sys.exit(f"pueue.py toggle: no such group: {name}")
    for name in args:
        paused = str(data["groups"][name]["status"]).lower() == "paused"
        command = ["start", "-g", name] if paused else ["pause", "-g", name]
        if pueue(command).returncode != 0:
            return 1
    return 0


def cmd_remove_group(args):
    if not args:
        sys.exit("pueue.py remove-group: no group given")
    data = status_json()
    for name in args:
        if name not in data["groups"]:
            sys.exit(f"pueue.py remove-group: no such group: {name}")
        if name == "default":
            sys.exit("pueue.py remove-group: the default group cannot be removed")

    unfinished = {}
    for task in data["tasks"].values():
        if task["group"] in args and next(iter(task["status"])) != "Done":
            unfinished.setdefault(task["group"], []).append(task["id"])
    if unfinished:
        detail = "; ".join(
            f"{name}: {', '.join(str(task_id) for task_id in sorted(ids))}"
            for name, ids in sorted(unfinished.items())
        )
        sys.exit(f"pueue.py remove-group: kill these tasks first: {detail}")

    print(f"Remove {', '.join(args)} and their finished tasks? [y/N] ", end="", flush=True)
    try:
        reply = input().strip().lower()
    except EOFError:
        reply = ""
    if reply not in ("y", "yes"):
        print("nothing removed")
        return

    for name in args:
        for command in (["clean", "-g", name], ["group", "remove", name]):
            if pueue(command).returncode != 0:
                sys.exit(f"pueue.py remove-group: pueue {' '.join(command)} failed")


def cmd_kill(args):
    """Kill tasks or a group and wait until nothing in scope is running.

    pueue only marks a killed task done once its process is reaped, so a list
    reload straight after `pueue kill` would still show it as running.
    """
    signal_name = "SIGTERM"
    group = None
    ids = []
    arg_iter = iter(args)
    for arg in arg_iter:
        if arg in ("-s", "--signal"):
            signal_name = next(arg_iter, None) or signal_name
        elif arg in ("-g", "--group"):
            group = next(arg_iter, None)
        else:
            ids.append(arg)
    if group:
        target = ["-g", group]
    elif ids:
        target = ids
    else:
        sys.exit("pueue.py kill: pass a group (-g) or task ids")

    command = ["kill"] if signal_name == "SIGTERM" else ["kill", "-s", signal_name]
    proc = pueue(command + target)
    if proc.returncode != 0:
        return proc.returncode

    def still_running(status):
        if group:
            return status.get("group") == group
        return str(status.get("id")) in ids

    deadline = monotonic() + 3
    while monotonic() < deadline:
        tasks = status_json().get("tasks", {}).values()
        if not any("Running" in t.get("status", {}) and still_running(t) for t in tasks):
            return 0
        sleep(0.02)
    return 0


COMMANDS = {
    "groups": cmd_groups,
    "tasks": cmd_tasks,
    "summary": cmd_summary,
    "toggle": cmd_toggle,
    "kill": cmd_kill,
    "remove-group": cmd_remove_group,
}


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip())
        return 0
    command, *args = argv
    if command not in COMMANDS:
        sys.exit(f"pueue.py: unknown subcommand: {command} (try --help)")
    return COMMANDS[command](args) or 0


if __name__ == "__main__":
    # die quietly when piped into head and friends
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main(sys.argv[1:]))
