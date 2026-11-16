# Matchmaker Benchmarks

PTY-driven comparison harness and internal `matchmaker-lib` microbenchmarks.

> [!NOTE]
> 
> Do not take these as authoritative: neither are they robust to variance, nor are they representative across different machines / normal use cases.
> Mostly, they were added (generated) out of curiosity.

## Prerequisites

Build `mm` with the `dist` profile (`codegen-units = 1`, `lto = "fat"`):

```sh
cargo build --profile dist -p matchmaker-cli
```

Optional comparison tools (automatically skipped if not on `PATH`): `fzf`, `sk`.

## PTY comparison suites

```sh
python3 benches/bench_tui.py --mm ./target/dist/mm --items 200000 --scale-items 500000 --runs 5 --warmup 1
```

Run a single suite with `--suite select1`, `--suite heavy`, or `--suite scale`:

1. **`select1`**: Startup + full ingestion (`start.sync=true` / `--sync`) + unique initial query + auto-exit (`exit.first=true` / `-1`). Reports wall time, CPU time, and peak RSS.
2. **`heavy`**: 6-column ANSI-colored dataset (`ansi = true`, `results.wrap = true`, `max_height = 4` for `mm`) in a `120x40` PTY. Measures first frame, broad query, 50 cursor moves (`25x Down` + `25x Up`) + PTY bytes emitted, query switch (`Ctrl-U`), accept latency, and peak RSS.
3. **`scale`**: 500k-path incremental query (`g` → `wp` → `robe`), query switch (`Ctrl-U` + `zxq_validator_9`), and broad re-filter (`Ctrl-U` + `gwprobe`), reporting latencies and peak RSS.

## Internal `matchmaker-lib` microbenchmarks

Measures `ResultsUI::update_table` (`cold_miss` vs `row_cache` vs `is_clean`) and pure `nucleo` vs `frizbee` (exact and `max_typos = 1`) matching throughput without PTY overhead:

```sh
cargo bench -p matchmaker-lib --bench ui_and_matcher
```
