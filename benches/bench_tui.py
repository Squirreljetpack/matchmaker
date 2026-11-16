#!/usr/bin/env python3
"""
PTY benchmark suite comparing mm (nucleo & frizbee backends), fzf, and sk.

Suites (all synchronous ingestion via start.sync=true / --sync):
1. select1: Startup + full ingestion + unique initial query + auto-exit
   (uses exit.first=true / -1). Reports wall time, CPU, and peak RSS.
2. heavy_ui: Interactive 6-column ANSI-colored + wrapped workload (120x40 PTY):
   - first_frame_ms: spawn -> first rendered item row
   - query_ms: broad query ('gwprobe') -> filtered rows rendered
   - nav50_ms & nav_kb: 25x Down + 25x Up cursor moves (exercises ResultsUI::row_cache)
   - switch_ms: Ctrl-U + disjoint unique query -> rendered
   - accept_ms: Enter -> exit
   - rss_mb: peak RSS via wait4 rusage
3. scale: 500k-path incremental query, Ctrl-U switch, and broad re-filter.
"""

import argparse
import fcntl
import os
import pty
import re
import select
import shutil
import statistics
import struct
import subprocess
import tempfile
import termios
import time

ANSI_RE = re.compile(
    rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b_[^\x1b]*\x1b\\|\x1b[=>]"
)


def strip_ansi(b: bytes) -> str:
    return ANSI_RE.sub(b"", b).decode("utf-8", errors="replace")


def set_winsize(fd: int, rows: int = 40, cols: int = 120) -> None:
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def spawn_pty(cmd: list[str], stdin_fd, env: dict[str, str]):
    master, slave = pty.openpty()
    set_winsize(slave, 40, 120)

    def preexec():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

    proc = subprocess.Popen(
        cmd,
        stdin=stdin_fd,
        stdout=subprocess.PIPE,
        stderr=slave,
        preexec_fn=preexec,
        env=env,
        close_fds=True,
    )
    os.close(slave)
    return proc, master


def spawn_pty_file(cmd: list[str], stdin_path: str, env: dict[str, str]):
    with open(stdin_path, "rb") as fin:
        return spawn_pty(cmd, fin, env)


def drain_until(
    master: int,
    proc: subprocess.Popen,
    predicate,
    timeout: float = 15.0,
) -> tuple[bool, bytes, float]:
    buf = bytearray()
    deadline = time.perf_counter() + timeout
    while True:
        if predicate(bytes(buf)):
            return True, bytes(buf), time.perf_counter()
        rem = deadline - time.perf_counter()
        if rem <= 0:
            return False, bytes(buf), time.perf_counter()
        r, _, _ = select.select([master], [], [], min(0.01, rem))
        if r:
            try:
                chunk = os.read(master, 65536)
                if not chunk:
                    return predicate(bytes(buf)), bytes(buf), time.perf_counter()
                buf.extend(chunk)
            except OSError:
                return predicate(bytes(buf)), bytes(buf), time.perf_counter()
        elif proc.poll() is not None:
            return predicate(bytes(buf)), bytes(buf), time.perf_counter()


def drain_idle(master: int, idle_s: float = 0.02) -> bytes:
    buf = bytearray()
    while True:
        r, _, _ = select.select([master], [], [], idle_s)
        if not r:
            break
        try:
            chunk = os.read(master, 65536)
            if not chunk:
                break
            buf.extend(chunk)
        except OSError:
            break
    return bytes(buf)


def wait_exit_rusage(
    master: int, proc: subprocess.Popen, timeout: float = 15.0
) -> tuple[bytes, int, float, float]:
    """Wait for exit and return (stdout, returncode, rss_mb, cpu_ms)."""
    deadline = time.perf_counter() + timeout
    while proc.poll() is None:
        rem = deadline - time.perf_counter()
        if rem <= 0:
            try:
                os.killpg(proc.pid, 9)
            except ProcessLookupError:
                pass
            break
        r, _, _ = select.select([master], [], [], min(0.01, rem))
        if r:
            try:
                if not os.read(master, 65536):
                    break
            except OSError:
                break
    out = proc.stdout.read() if proc.stdout else b""
    _, status, ru = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    os.close(master)
    rss_mb = ru.ru_maxrss / 1024.0
    cpu_ms = (ru.ru_utime + ru.ru_stime) * 1000.0
    return out, proc.returncode, rss_mb, cpu_ms


def make_plain_dataset(path: str, n: int) -> str:
    with open(path, "w") as f:
        for i in range(n - 1):
            f.write(f"src/pkg_{i % 200}/mod_{i % 50}/item_{i:06d}.rs\n")
        f.write("src/pkg_target/mod_unique/zxq_needle_999999.rs\n")
    return "zxq_needle_999999"


def make_heavy_ansi_dataset(path: str, n: int) -> tuple[str, str]:
    """
    6-column ANSI-colored dataset (>130 chars/line to exercise wrapping + column sizing):
    - Background rows at head & tail: crates/core_... (never contain 'gwprobe' or 'tenant_alpha').
    - Middle slice [mid, mid+119): 'gwprobe/...' rows for broad filtering + 50x cursor nav.
    - Single row at mid+119: disjoint 'services/auth/deep/tenant_alpha/zxq_validator_9.rs'.
    """
    mid = max(100, n // 2)
    with open(path, "w") as f:
        for i in range(n):
            if mid <= i < mid + 119:
                j = i - mid
                f.write(
                    f"\x1b[36m{i:06d}\x1b[0m\t"
                    f"\x1b[32mINFO\x1b[0m\t"
                    f"\x1b[35mshard-{j % 8}\x1b[0m\t"
                    f"gwprobe/w{j % 9}/h_{j:03d}.rs\t"
                    f"\x1b[90m{12 + (j % 40)}ms\x1b[0m\t"
                    f"processed batch {j} with cached layout metadata and trailing context for line wrapping\n"
                )
            elif i == mid + 119:
                f.write(
                    f"\x1b[36m{i:06d}\x1b[0m\t"
                    f"\x1b[1;31mCRIT\x1b[0m\t"
                    f"\x1b[35mshard-0\x1b[0m\t"
                    f"services/auth/deep/tenant_alpha/zxq_validator_9.rs\t"
                    f"\x1b[90m99ms\x1b[0m\t"
                    f"sig verify failed\n"
                )
            else:
                tag = "INFO" if i % 3 else "WARN"
                color = "32" if i % 3 else "33"
                f.write(
                    f"\x1b[36m{i:06d}\x1b[0m\t"
                    f"\x1b[{color}m{tag}\x1b[0m\t"
                    f"\x1b[35mshard-{i % 8}\x1b[0m\t"
                    f"crates/core_{i % 40}/sub_{i % 25}/node_{i:06d}.rs\t"
                    f"\x1b[90m{10 + (i % 90)}ms\x1b[0m\t"
                    f"standard event payload {i} across pipeline stage {i % 7} with extended diagnostics\n"
                )
    return "gwprobe", "zxq_validator_9"


def run_select1(
    cmd: list[str], stdin_path: str, env: dict[str, str]
) -> tuple[float, bytes, int, float, float]:
    t0 = time.perf_counter()
    proc, master = spawn_pty_file(cmd, stdin_path, env)
    out, rc, rss_mb, cpu_ms = wait_exit_rusage(master, proc, timeout=20.0)
    dt_ms = (time.perf_counter() - t0) * 1000.0
    return dt_ms, out, rc, rss_mb, cpu_ms


def run_heavy_ui(
    cmd: list[str],
    stdin_path: str,
    env: dict[str, str],
    broad_query: str,
    unique_query: str,
    steps: int = 25,
) -> dict[str, float]:
    t0 = time.perf_counter()
    proc, master = spawn_pty_file(cmd, stdin_path, env)

    # 1. First rendered frame ('shard-' is short enough never to wrap)
    ok, _, t_frame = drain_until(
        master,
        proc,
        lambda b: "shard-" in strip_ansi(b),
        timeout=20.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for first frame: {cmd}")
    first_frame_ms = (t_frame - t0) * 1000.0
    drain_idle(master, 0.02)

    # 2. Broad query
    t_q0 = time.perf_counter()
    os.write(master, broad_query.encode())
    ok, _, t_q1 = drain_until(
        master,
        proc,
        lambda b: "h_0" in strip_ansi(b) and "gwprobe" in strip_ansi(b),
        timeout=10.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for broad query match: {cmd}")
    query_ms = (t_q1 - t_q0) * 1000.0
    drain_idle(master, 0.015)

    # 3. 25x Down + 25x Up cursor burst (measures row_cache + PTY byte volume)
    nav_bytes = 0
    t_s0 = time.perf_counter()
    for seq in ([b"\x1b[B"] * steps) + ([b"\x1b[A"] * steps):
        os.write(master, seq)
        r, _, _ = select.select([master], [], [], 0.015)
        if r:
            try:
                nav_bytes += len(os.read(master, 65536))
            except OSError:
                pass
    nav_ms = (time.perf_counter() - t_s0) * 1000.0
    nav_bytes += len(drain_idle(master, 0.01))

    # 4. Clear (Ctrl-U) + disjoint unique query
    t_n0 = time.perf_counter()
    os.write(master, b"\x15" + unique_query.encode())
    ok, _, t_n1 = drain_until(
        master,
        proc,
        lambda b: "tenant_alpha" in strip_ansi(b),
        timeout=10.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for unique match: {cmd}")
    switch_ms = (t_n1 - t_n0) * 1000.0

    # 5. Accept
    t_a0 = time.perf_counter()
    os.write(master, b"\r")
    out, rc, rss_mb, cpu_ms = wait_exit_rusage(master, proc, timeout=5.0)
    accept_ms = (time.perf_counter() - t_a0) * 1000.0

    if rc != 0 or b"zxq_validator_9" not in out:
        raise RuntimeError(f"Unexpected exit rc={rc} out={out!r} for {cmd}")

    return {
        "first_frame_ms": first_frame_ms,
        "query_ms": query_ms,
        "nav_ms": nav_ms,
        "nav_kb": nav_bytes / 1024.0,
        "switch_ms": switch_ms,
        "accept_ms": accept_ms,
        "rss_mb": rss_mb,
        "cpu_ms": cpu_ms,
    }


def make_scale_dataset(path: str, n: int) -> None:
    # Enforce strict length hierarchy for empty-query / tie-break ordering:
    #   crates/pkg_.. (28 chars) < gwprobe/gwitem_.. (33 chars) < tenant_alpha/zxq_validator_9 (61 chars)
    # so 'gwitem_' never appears on frame 1 and 'tenant_alpha' never appears before narrowing.
    j = 0
    with open(path, "w") as f:
        for i in range(n):
            if i == n - 1:
                f.write("services/auth/deep/subsystem/tenant_alpha/zxq_validator_9.rs\n")
            elif i >= n // 5 and (i - n // 5) % max(1, (n * 3 // 5) // 500) == 0 and j < 499:
                f.write(f"services/gwprobe/w{j % 17}/gwitem_{j:03d}.rs\n")
                j += 1
            else:
                f.write(f"crates/pkg_{i % 40:02d}/s_{i % 25:02d}/n_{i:06d}.rs\n")


def run_scale_query(
    cmd: list[str], stdin_path: str, env: dict[str, str]
) -> dict[str, float]:
    t0 = time.perf_counter()
    proc, master = spawn_pty_file(cmd, stdin_path, env)

    ok, _, t_frame = drain_until(
        master,
        proc,
        lambda b: "crates/pkg_" in strip_ansi(b),
        timeout=30.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for scale first frame: {cmd}")
    frame_ms = (t_frame - t0) * 1000.0
    drain_idle(master, 0.02)

    # Incremental query: 'g' -> 'wp' -> 'robe'
    t_q0 = time.perf_counter()
    for part in (b"g", b"wp", b"robe"):
        os.write(master, part)
        time.sleep(0.005)
    ok, _, t_q1 = drain_until(
        master,
        proc,
        lambda b: "gwitem_" in strip_ansi(b),
        timeout=15.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for scale broad query: {cmd}")
    inc_ms = (t_q1 - t_q0) * 1000.0
    drain_idle(master, 0.015)

    # Switch via Ctrl-U to unique match (disjoint substring check: 'tenant_alpha')
    t_n0 = time.perf_counter()
    os.write(master, b"\x15zxq_validator_9")
    ok, _, t_n1 = drain_until(
        master,
        proc,
        lambda b: "tenant_alpha" in strip_ansi(b),
        timeout=15.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for scale switch query: {cmd}")
    narrow_ms = (t_n1 - t_n0) * 1000.0
    drain_idle(master, 0.015)

    # Clear back to 'gwprobe' (forces full re-match of broadened query)
    t_b0 = time.perf_counter()
    os.write(master, b"\x15gwprobe")
    ok, _, t_b1 = drain_until(
        master,
        proc,
        lambda b: "gwitem_" in strip_ansi(b),
        timeout=15.0,
    )
    if not ok:
        wait_exit_rusage(master, proc, timeout=0.5)
        raise RuntimeError(f"Timed out waiting for scale backspace re-filter: {cmd}")
    back_ms = (t_b1 - t_b0) * 1000.0

    # Accept
    os.write(master, b"\r")
    out, rc, rss_mb, _ = wait_exit_rusage(master, proc, timeout=5.0)
    if rc != 0 or b"gwprobe/" not in out:
        raise RuntimeError(f"Unexpected scale exit rc={rc} out={out!r} for {cmd}")

    return {
        "frame_ms": frame_ms,
        "inc_ms": inc_ms,
        "narrow_ms": narrow_ms,
        "back_ms": back_ms,
        "rss_mb": rss_mb,
    }


def med(vals: list[float]) -> float:
    return statistics.median(vals)


def main():
    ap = argparse.ArgumentParser(description="Benchmark mm against fzf, sk, and tv in a PTY.")
    ap.add_argument("--mm", default="./target/dist/mm", help="Path to mm binary (defaults to dist fat-LTO build)")
    ap.add_argument("--items", type=int, default=200_000, help="Dataset row count for select1/heavy")
    ap.add_argument("--scale-items", type=int, default=500_000, help="Dataset row count for scale suite")
    ap.add_argument("--runs", type=int, default=5, help="Measured runs per tool")
    ap.add_argument("--warmup", type=int, default=1, help="Warmup runs per tool")
    ap.add_argument(
        "--suite",
        choices=["all", "select1", "heavy", "scale"],
        default="all",
        help="Which benchmark suite(s) to run",
    )
    args = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        cfg_nucleo = os.path.join(tmp, "nucleo.toml")
        with open(cfg_nucleo, "w") as f:
            f.write("[tui]\nsleep_ms = 0\n[matcher]\nbackend = \"nucleo\"\n")

        cfg_frizbee = os.path.join(tmp, "frizbee.toml")
        with open(cfg_frizbee, "w") as f:
            f.write("[tui]\nsleep_ms = 0\n[matcher]\nbackend = \"frizbee\"\n")

        cfg_heavy_nucleo = os.path.join(tmp, "heavy_nucleo.toml")
        with open(cfg_heavy_nucleo, "w") as f:
            f.write(
                "[tui]\nsleep_ms = 0\n"
                "[matcher]\nbackend = \"nucleo\"\nansi = true\n"
                "[results]\nwrap = true\nmax_height = 4\n"
            )

        cfg_heavy_frizbee = os.path.join(tmp, "heavy_frizbee.toml")
        with open(cfg_heavy_frizbee, "w") as f:
            f.write(
                "[tui]\nsleep_ms = 0\n"
                "[matcher]\nbackend = \"frizbee\"\nansi = true\n"
                "[results]\nwrap = true\nmax_height = 4\n"
            )

        base_env = {**os.environ, "FZF_DEFAULT_OPTS": "", "NO_COLOR": "0"}

        if args.suite in ("all", "select1"):
            plain_path = os.path.join(tmp, "plain.txt")
            needle = make_plain_dataset(plain_path, args.items)

            tools_s1 = [
                ("mm (nucleo)", [args.mm, "--config", cfg_nucleo, "start.sync=true", "exit.first=true", f"query.initial={needle}"]),
                ("mm (frizbee)", [args.mm, "--config", cfg_frizbee, "start.sync=true", "exit.first=true", f"query.initial={needle}"]),
            ]
            if shutil.which("fzf"):
                tools_s1.append(("fzf", ["fzf", "--sync", "-1", "-q", needle]))
            if shutil.which("sk"):
                tools_s1.append(("sk", ["sk", "--sync", "-1", "-q", needle]))

            print(f"=== 1. select-1 / exit.first ({args.items:,} plain paths, {args.runs} runs, {args.warmup} warmup) ===")
            for name, cmd in tools_s1:
                agg: dict[str, list[float]] = {}
                ok = True
                for r in range(args.warmup + args.runs):
                    ms, out, rc, rss_mb, cpu_ms = run_select1(cmd, plain_path, base_env)
                    if rc != 0 or needle.encode() not in out:
                        print(f"  {name:16s} FAILED rc={rc}")
                        ok = False
                        break
                    if r >= args.warmup:
                        agg.setdefault("ms", []).append(ms)
                        agg.setdefault("rss", []).append(rss_mb)
                        agg.setdefault("cpu", []).append(cpu_ms)
                if ok:
                    print(
                        f"  {name:16s} wall={med(agg['ms']):6.1f}ms "
                        f"(min {min(agg['ms']):5.1f}, max {max(agg['ms']):5.1f}) | "
                        f"cpu={med(agg['cpu']):6.1f}ms | rss={med(agg['rss']):5.1f}MB"
                    )

        if args.suite in ("all", "heavy"):
            heavy_path = os.path.join(tmp, "heavy.txt")
            broad_q, uniq_q = make_heavy_ansi_dataset(heavy_path, args.items)

            tools_heavy = [
                ("mm (nucleo)", [args.mm, "--config", cfg_heavy_nucleo, "start.sync=true"]),
                ("mm (frizbee)", [args.mm, "--config", cfg_heavy_frizbee, "start.sync=true"]),
            ]
            if shutil.which("fzf"):
                tools_heavy.append(("fzf (--ansi)", ["fzf", "--sync", "--ansi"]))
            if shutil.which("sk"):
                tools_heavy.append(("sk (--ansi)", ["sk", "--sync", "--ansi"]))

            print(f"\n=== 2. Heavy 6-Col ANSI + Wrapped + 50x Nav ({args.items:,} rows, {args.runs} runs) ===")
            for name, cmd in tools_heavy:
                agg = {}
                ok = True
                for r in range(args.warmup + args.runs):
                    try:
                        res = run_heavy_ui(cmd, heavy_path, base_env, broad_q, uniq_q)
                    except Exception as e:
                        print(f"  {name:16s} FAILED: {e}")
                        ok = False
                        break
                    if r >= args.warmup:
                        for k, v in res.items():
                            agg.setdefault(k, []).append(v)
                if ok:
                    print(
                        f"  {name:16s} "
                        f"frame={med(agg['first_frame_ms']):6.1f}ms | "
                        f"query={med(agg['query_ms']):5.1f}ms | "
                        f"nav50={med(agg['nav_ms']):6.1f}ms ({med(agg['nav_kb']):5.1f}KB) | "
                        f"switch={med(agg['switch_ms']):5.1f}ms | "
                        f"accept={med(agg['accept_ms']):5.1f}ms | "
                        f"rss={med(agg['rss_mb']):5.1f}MB"
                    )

        if args.suite in ("all", "scale"):
            scale_path = os.path.join(tmp, "scale.txt")
            make_scale_dataset(scale_path, args.scale_items)

            tools_scale = [
                ("mm (nucleo)", [args.mm, "--config", cfg_nucleo, "start.sync=true"]),
                ("mm (frizbee)", [args.mm, "--config", cfg_frizbee, "start.sync=true"]),
            ]
            if shutil.which("fzf"):
                tools_scale.append(("fzf", ["fzf", "--sync"]))
            if shutil.which("sk"):
                tools_scale.append(("sk", ["sk", "--sync"]))

            print(f"\n=== 3. Scale & Query-Shape ({args.scale_items:,} paths, incremental + narrow + backspace, {args.runs} runs) ===")
            for name, cmd in tools_scale:
                agg = {}
                ok = True
                for r in range(args.warmup + args.runs):
                    try:
                        res = run_scale_query(cmd, scale_path, base_env)
                    except Exception as e:
                        print(f"  {name:16s} FAILED: {e}")
                        ok = False
                        break
                    if r >= args.warmup:
                        for k, v in res.items():
                            agg.setdefault(k, []).append(v)
                if ok:
                    print(
                        f"  {name:16s} "
                        f"frame={med(agg['frame_ms']):6.1f}ms | "
                        f"inc={med(agg['inc_ms']):5.1f}ms | "
                        f"narrow={med(agg['narrow_ms']):5.1f}ms | "
                        f"back={med(agg['back_ms']):5.1f}ms | "
                        f"rss={med(agg['rss_mb']):5.1f}MB"
                    )


if __name__ == "__main__":
    main()
