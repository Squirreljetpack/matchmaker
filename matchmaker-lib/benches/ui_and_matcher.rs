//! Microbenchmarks for `matchmaker-lib` UI row caching and matcher backends.
//!
//! Run with:
//!   cargo bench -p matchmaker-lib --bench ui_and_matcher

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use ansi_to_tui::IntoText;
use matchmaker::{
    Selector,
    config::{MatcherConfig, ResultsConfig},
    matcher::{matcher, set_matcher_config},
    nucleo::{
        ColumnIndexable, Worker, new_snapshot,
        nucleo::{self, MatcherBackend},
    },
    ui::{Rect, ResultsUI},
};
use ratatui::text::Text;

#[derive(Clone)]
struct HeavyRow {
    cols: [String; 6],
}

impl ColumnIndexable for HeavyRow {
    fn get_str(&self, i: usize) -> std::borrow::Cow<'_, str> {
        self.cols.get(i).map(|s| s.as_str()).unwrap_or("").into()
    }

    fn get_text(&self, i: usize) -> Text<'_> {
        self.cols
            .get(i)
            .and_then(|s| s.as_bytes().into_text().ok())
            .unwrap_or_default()
    }
}

fn make_heavy_rows(n: usize) -> Vec<HeavyRow> {
    (0..n)
        .map(|i| {
            let tag = if i % 3 == 0 {
                "\x1b[33mWARN\x1b[0m"
            } else {
                "\x1b[32mINFO\x1b[0m"
            };
            HeavyRow {
                cols: [
                    format!("\x1b[36m{i:06}\x1b[0m"),
                    tag.to_string(),
                    format!("\x1b[35mshard-{}\x1b[0m", i % 8),
                    format!("crates/core_{}/sub_{}/node_{i:06}.rs", i % 40, i % 25),
                    format!("\x1b[90m{}ms\x1b[0m", 10 + (i % 90)),
                    format!(
                        "standard event payload {i} across pipeline stage {} with extended diagnostics",
                        i % 7
                    ),
                ],
            }
        })
        .collect()
}

fn settle<T: matchmaker::SSS, D: 'static>(worker: &mut Worker<T, D>) -> u32 {
    loop {
        let (_, status) = new_snapshot(&mut worker.nucleo);
        if !status.running {
            return status.matched_count;
        }
        std::thread::yield_now();
    }
}

fn stats_us(samples: &mut [Duration]) -> (f64, f64, f64) {
    samples.sort_unstable();
    let med = samples[samples.len() / 2].as_secs_f64() * 1e6;
    let min = samples[0].as_secs_f64() * 1e6;
    let p95 = samples[(samples.len() * 95) / 100].as_secs_f64() * 1e6;
    (med, min, p95)
}

fn bench_results_ui() {
    let n = 10_000;
    let rows = make_heavy_rows(n);
    let mut worker: Worker<HeavyRow, ()> =
        Worker::new_indexable(["c0", "c1", "c2", "c3", "c4", "c5"], None);
    worker.append(rows);
    settle(&mut worker);

    let cfg = ResultsConfig {
        wrap: true,
        max_height: 4,
        ..Default::default()
    };
    let mut ui = ResultsUI::new(cfg);
    ui.init(&mut worker);
    ui.update_dimensions(Rect::new(0, 0, 120, 40));

    let selector = Selector::new();
    let mut m = matcher();

    // Warm once to populate preferred widths and row_cache
    ui.update_table(&mut worker, &selector, &mut m);

    let iters = 200;

    // 1. Cold cache miss (set_dirty forces full row render + ANSI/wrap/width pass)
    let mut cold = Vec::with_capacity(iters);
    for _ in 0..iters {
        ui.set_dirty();
        let t0 = Instant::now();
        ui.update_table(&mut worker, &selector, &mut m);
        cold.push(t0.elapsed());
        black_box(&ui.table);
    }

    // 2. Cursor navigation within cached viewport (row_cache hit)
    let mut cached_nav = Vec::with_capacity(iters);
    for i in 0..iters {
        if i % 10 < 5 {
            ui.cursor_next();
        } else {
            ui.cursor_prev();
        }
        let t0 = Instant::now();
        ui.update_table(&mut worker, &selector, &mut m);
        cached_nav.push(t0.elapsed());
        black_box(&ui.table);
    }

    // 3. Clean no-op tick (is_clean() short-circuit)
    let mut clean_tick = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        ui.update_table(&mut worker, &selector, &mut m);
        clean_tick.push(t0.elapsed());
        black_box(&ui.table);
    }

    let (c_med, c_min, c_p95) = stats_us(&mut cold);
    let (n_med, n_min, n_p95) = stats_us(&mut cached_nav);
    let (k_med, k_min, k_p95) = stats_us(&mut clean_tick);

    println!("=== ResultsUI::update_table (120x40, 6-col ANSI, wrap=true) ===");
    println!("  cold_miss   median={c_med:8.1} us  (min {c_min:7.1}, p95 {c_p95:8.1})");
    println!(
        "  row_cache   median={n_med:8.1} us  (min {n_min:7.1}, p95 {n_p95:8.1})  [{:.1}x vs cold]",
        c_med / n_med
    );
    println!(
        "  is_clean    median={k_med:8.1} us  (min {k_min:7.1}, p95 {k_p95:8.1})  [{:.1}x vs cold]",
        c_med / k_med
    );
}

fn bench_matcher_backends(n: usize) {
    let items: Vec<String> = (0..n)
        .map(|i| {
            if i == n / 2 {
                "services/gwprobe/deep/tenant_alpha/zxq_validator_9.rs".to_string()
            } else if i % 1000 == 0 {
                format!("services/gwprobe/worker_{}/handler_{i:06}.rs", i % 17)
            } else {
                format!("crates/core_{}/sub_{}/node_{i:06}.rs", i % 40, i % 25)
            }
        })
        .collect();

    let queries = [
        ("1-char ('g')", "g"),
        ("3-char ('gwp')", "gwp"),
        ("7-char ('gwprobe')", "gwprobe"),
        ("15-char ('gwprobe zxq_val')", "gwprobe zxq_val"),
        ("backspace ('gwp')", "gwp"),
    ];

    println!("\n=== Matcher Backends ({n} items, query-shape sequence) ===");
    for (label, backend) in [
        ("nucleo", MatcherBackend::Nucleo(nucleo::Config::DEFAULT)),
        (
            "frizbee",
            MatcherBackend::Frizbee(nucleo::frizbee::Config::default()),
        ),
        (
            "frizbee (max_typos=1)",
            MatcherBackend::Frizbee(nucleo::frizbee::Config {
                max_typos: Some(1),
                ..Default::default()
            }),
        ),
    ] {
        set_matcher_config(MatcherConfig::default());
        let mut worker = Worker::new_single_column();
        worker.set_config(backend.clone());
        let t_ing = Instant::now();
        worker.append(items.iter().cloned());
        settle(&mut worker);
        let ing_ms = t_ing.elapsed().as_secs_f64() * 1e3;

        let mut parts = Vec::new();
        for (qname, q) in queries {
            let mut runs = Vec::with_capacity(5);
            let mut count = 0;
            for _ in 0..5 {
                worker.find("");
                settle(&mut worker);
                let t0 = Instant::now();
                worker.find(q);
                count = settle(&mut worker);
                runs.push(t0.elapsed());
            }
            let (med, _, _) = stats_us(&mut runs);
            parts.push(format!("{qname}={:.2}ms ({count})", med / 1000.0));
        }

        // Non-subsequence 1-char substitution typo test ('x' instead of 'o')
        worker.find("");
        settle(&mut worker);
        let t0 = Instant::now();
        worker.find("zxq_validatxr_9");
        let typo_cnt = settle(&mut worker);
        let typo_ms = t0.elapsed().as_secs_f64() * 1e3;

        println!(
            "  {label:22} ingest={ing_ms:6.1}ms | {} | typo('zxq_validatxr_9')={typo_ms:.2}ms (hits={typo_cnt})",
            parts.join(" | ")
        );
    }
}

fn main() {
    bench_results_ui();
    bench_matcher_backends(200_000);
    bench_matcher_backends(500_000);
}
