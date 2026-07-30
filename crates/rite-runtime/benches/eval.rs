//! Interpreter benchmarks.
//!
//! These exist because the project had performance *targets* in IMPLEMENTATION.md and
//! nothing that measured them, and because the closure/environment redesign changed the
//! cost model — closures went from a deep clone of every frame to a handful of `Arc`
//! bumps — with no way to notice a regression.
//!
//! Run: `cargo bench -p rite-runtime`
//! Compare against a baseline: `cargo bench -p rite-runtime -- --save-baseline before`,
//! then after a change `cargo bench -p rite-runtime -- --baseline before`.
//!
//! What each case is watching for:
//!   * `closure_creation` — the per-closure environment capture.
//!   * `pipeline_map_keep` — `call_value` on a closure, once per element.
//!   * `fib_recursive` — call depth and frame push/pop.
//!   * `record_build` / `record_spread` — `IndexMap` work and the merge fold.
//!   * `string_interpolation` — desugared concat.
//!   * `arithmetic_loop` — the boxed-future-per-node floor, with nothing else going on.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rite_core::{FileId, SourceFile};
use rite_runtime::RuntimeContext;
use rite_sem::{compile_to_ir, ProgramIr};

/// Compile `src` once, outside the measured loop.
///
/// The front end is measured separately (`frontend/compile`). Mixing them would mean a
/// parser regression and an interpreter regression look identical, which defeats the
/// point of having a baseline.
fn compile(src: &str) -> ProgramIr {
    let file = SourceFile::new(FileId(0), "bench.rite", src);
    let (ir, diags) = compile_to_ir(&file);
    assert!(
        !diags.has_errors(),
        "bench source does not compile:\n{:#?}",
        diags.into_vec()
    );
    ir.expect("ir")
}

/// Evaluate already-compiled IR, panicking on failure so a broken benchmark never
/// silently measures an error path.
fn run(rt: &tokio::runtime::Runtime, ir: &ProgramIr) {
    rt.block_on(async {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        rite_runtime::run_ir(ir, &mut ctx)
            .await
            .unwrap_or_else(|e| panic!("bench eval failed: {e}"));
    });
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
}

/// Compile once, then measure evaluation only.
fn bench_eval(
    g: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    id: BenchmarkId,
    src: &str,
) {
    let rt = runtime();
    let ir = compile(src);
    g.bench_function(id, |b| b.iter(|| run(&rt, &ir)));
}

fn bench_closures(c: &mut Criterion) {
    let mut g = c.benchmark_group("closures");
    for n in [100usize, 2_000] {
        let src = format!(
            "◆ adder(k) ⟦ ^ {{ |x| x + k }} ⟧\n\
             total ↢ 0\n\
             (1..{n}) → each {{ |i| total := total + adder(i)(1) }}\n\
             total\n"
        );
        bench_eval(&mut g, BenchmarkId::new("closure_creation", n), &src);
    }
    g.finish();
}

fn bench_pipelines(c: &mut Criterion) {
    let mut g = c.benchmark_group("pipelines");
    for n in [100usize, 5_000] {
        let src =
            format!("(1..{n})\n  → map {{ |x| x * 2 }}\n  → keep {{ |x| x % 3 = 0 }}\n  → sum\n");
        bench_eval(&mut g, BenchmarkId::new("pipeline_map_keep", n), &src);
    }
    g.finish();
}

fn bench_calls(c: &mut Criterion) {
    let mut g = c.benchmark_group("calls");
    for n in [15u32, 20] {
        let src =
            format!("◆ fib(n) ⟦\n  ? n < 2 ⟦ ^ n ⟧\n  ^ fib(n - 1) + fib(n - 2)\n⟧\nfib({n})\n");
        bench_eval(&mut g, BenchmarkId::new("fib_recursive", n), &src);
    }
    g.finish();
}

fn bench_values(c: &mut Criterion) {
    let mut g = c.benchmark_group("values");
    bench_eval(
        &mut g,
        BenchmarkId::new("record_build", 5),
        "cfg ← ⟨host: \"h\", port: 80, tls: false, retries: 3, name: \"x\"⟩\ncfg.port\n",
    );
    // The merge fold: `⟨..a, k: v⟩` lowers to `a + ⟨k: v⟩`.
    bench_eval(
        &mut g,
        BenchmarkId::new("record_spread", 5),
        "base ← ⟨host: \"h\", port: 80, tls: false⟩\n⟨..base, port: 443, extra: 1⟩.port\n",
    );
    bench_eval(
        &mut g,
        BenchmarkId::new("string_interpolation", 3),
        "name ← \"Aura\"\nn ← 42\n\"hi {name}, n={n}, again {name}\"\n",
    );
    g.finish();
}

fn bench_floor(c: &mut Criterion) {
    // Straight-line arithmetic: closest thing to measuring per-node overhead alone.
    let mut g = c.benchmark_group("floor");
    for n in [1_000usize, 20_000] {
        let src = format!("total ↢ 0\n(1..{n}) → each {{ |i| total := total + i * 2 }}\ntotal\n");
        bench_eval(&mut g, BenchmarkId::new("arithmetic_loop", n), &src);
    }
    g.finish();
}

/// The front end on its own, so an eval number can be read without wondering whether
/// the parser moved. Also the closest thing to the "100-300 ms" LSP responsiveness
/// target in IMPLEMENTATION.md, which nothing measured.
fn bench_frontend(c: &mut Criterion) {
    let mut g = c.benchmark_group("frontend");
    let small = "◆ sq(n) ⟦ ^ n * n ⟧\n! @console.println(str(sq(12)))\n";
    g.bench_function(BenchmarkId::new("compile", "small"), |b| {
        b.iter(|| compile(small))
    });
    // A file with some bulk: 200 functions plus calls.
    let big: String = (0..200)
        .map(|i| format!("◆ f{i}(n) ⟦ ^ n + {i} ⟧\n"))
        .chain((0..200).map(|i| format!("x{i} ← f{i}({i})\n")))
        .collect();
    g.bench_function(BenchmarkId::new("compile", "200_fns"), |b| {
        b.iter(|| compile(&big))
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_frontend,
    bench_closures,
    bench_pipelines,
    bench_calls,
    bench_values,
    bench_floor
);
criterion_main!(benches);
