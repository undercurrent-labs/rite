//! The interpreter is normative; `rite build` claims behavioural parity with it.
//!
//! That claim is the one this file checks, across the language surface rather than on
//! the handful of shapes the conformance fixtures happen to cover. Each program is run
//! twice in-process — through the tree-walking interpreter and through `run_ir`, which
//! is the path a compiled binary takes — and the value, stdout and stderr must agree.
//!
//! No `cargo build` is involved, so this runs on every `cargo test` in milliseconds.
//! The expensive end-to-end builds live behind `--ignored` in `rite-compiler`.

use rite_test::differential_source;

/// A Rite call level costs ~256 KB of debug stack (see `rite_runtime::budget`), and the
/// thread behind `#[tokio::test]` gets 2 MiB — eight levels. The recursion below is
/// modest, but sizing the thread explicitly keeps a parity test from failing for a
/// reason that has nothing to do with parity.
const STACK: usize = 64 * 1024 * 1024;

/// Assert parity for every program in a group, reporting all mismatches at once —
/// a lowering bug usually breaks a family of shapes, and seeing the family is the
/// difference between "fix this case" and "fix this construct".
fn parity(group: &'static str, cases: &'static [&'static str]) {
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            rt.block_on(async {
                let mut failures = Vec::new();
                for (i, src) in cases.iter().enumerate() {
                    if let Err(e) = differential_source(&format!("{group}-{i}.rite"), src).await {
                        failures.push(format!("\n--- {group}[{i}]\n{src}\n  => {e}"));
                    }
                }
                failures
            })
        })
        .expect("spawn");
    let failures = handle.join().expect("parity thread panicked");
    assert!(
        failures.is_empty(),
        "{} of {} {group} programs diverged:{}",
        failures.len(),
        cases.len(),
        failures.join("")
    );
}

#[test]
fn arithmetic_and_numbers() {
    parity(
        "arithmetic",
        &[
            "1 + 2 * 3",
            "(1 + 2) * 3",
            "7 / 2",
            "7 % 3",
            "-5 + 3",
            "2 * -3",
            "1.5 + 2.5",
            "10 / 4.0",
            "0 - 0",
            "1 + 2 = 3",
            "0xff + 0b101",
            "1_000 + 1",
            "100000 * 100000",
            "3 < 4 and 4 <= 4",
            "not (1 = 2)",
        ],
    );
}

#[test]
fn strings_and_interpolation() {
    parity(
        "strings",
        &[
            r#""a" + "b" + "c""#,
            r#"name ← "Aura"
"hi {name}""#,
            r#"n ← 3
"n={n} and {n}""#,
            r#"u ← ⟨name: "deep"⟩
"{u.name}""#,
            r#""literal \{name}""#,
            r#""{{ mustache }}""#,
            r#"r"\d+ {raw}""#,
            r#"str(99) + "!""#,
            r#""tab\there\nnewline""#,
            r#""\u{1F600}" + "ok""#,
            r#""日本語" + "x""#,
        ],
    );
}

#[test]
fn lists_and_records() {
    parity(
        "collections",
        &[
            "[1, 2, 3]",
            "[]",
            "[ [1, 2], [3] ]",
            "⟨a: 1, b: 2⟩",
            "⟨⟩",
            "⟨a: ⟨b: ⟨c: 1⟩⟩⟩",
            "⟨a: 1⟩.a",
            "⟨a: 1⟩.missing",
            "⟨a: 1⟩ + ⟨b: 2⟩",
            "⟨a: 1⟩ + ⟨a: 2⟩",
            "base ← ⟨host: \"h\", port: 80⟩
⟨..base, port: 443⟩",
            "base ← ⟨port: 80⟩
⟨port: 1, ..base⟩",
            "[1, 2, 3] → sum",
            "[1, 2, 3, 4] → keep { |n| n % 2 = 0 } → map { |n| n * n } → sum",
            "[3, 1, 2] → count",
            "[] → count",
            "2 ∈ [1, 2, 3]",
            "9 ∉ [1, 2, 3]",
        ],
    );
}

#[test]
fn functions_closures_and_recursion() {
    parity(
        "functions",
        &[
            "◆ f(x) ⟦ ^ x + 1 ⟧
f(41)",
            "◆ add(a, b) ⟦ ^ a + b ⟧
add(2, 3)",
            "◆ outer(x) ⟦
  ◆ inner(y) ⟦ ^ y * 2 ⟧
  ^ inner(x) + 1
⟧
outer(5)",
            "◆ fact(n) ⟦ ^ ? n <= 1 ⟦ 1 ⟧ : ⟦ n * fact(n - 1) ⟧ ⟧
fact(10)",
            "◆ fib(n) ⟦ ^ ? n < 2 ⟦ n ⟧ : ⟦ fib(n - 1) + fib(n - 2) ⟧ ⟧
fib(15)",
            "◆ mk(n) ⟦ ^ { |x| x + n } ⟧
f ← mk(10)
f(5)",
            "◆ early(x) ⟦
  ? x > 0 ⟦ ^ #positive ⟧
  ^ #other
⟧
early(1)",
            "◆ last(x) ⟦ x * 2 ⟧
last(21)",
            "f ← { |a, b| a - b }
f(10, 4)",
        ],
    );
}

#[test]
fn pattern_matching() {
    parity(
        "matching",
        &[
            "~ #ok ⟦\n  #ok → 1\n  _ → 2\n⟧",
            "~ 5 ⟦\n  1 → #one\n  5 → #five\n  _ → #other\n⟧",
            "~ [1, 2, 3] ⟦\n  [a, b, c] → a + b + c\n  _ → 0\n⟧",
            "~ [1, 2, 3] ⟦\n  [head, ..rest] → rest → count\n  _ → 0\n⟧",
            "~ ⟨a: 1⟩ ⟦\n  ⟨a: v⟩ → v\n  _ → 0\n⟧",
            "~ ok(7) ⟦\n  ok v → v\n  err e → 0\n⟧",
            "~ err(#nope) ⟦\n  ok v → v\n  err e → e\n⟧",
            "~ \"s\" ⟦\n  \"s\" → #matched\n  _ → #no\n⟧",
            "~ [] ⟦\n  [] → #empty\n  _ → #full\n⟧",
        ],
    );
}

#[test]
fn results_and_error_propagation() {
    parity(
        "results",
        &[
            "ok(1)",
            "err(#bad)",
            "ok(41)?  + 1",
            "◆ f() ⟦ ^ ok(1)? ⟧
f()",
            "◆ f() ⟦
  v ← err(#bad)?
  ^ #unreachable
⟧
~ f() ⟦\n  err e → e\n  _ → #no\n⟧",
            "none ?? 42",
            "1 ?? 42",
            "⟨a: 1⟩.missing ?? #fallback",
        ],
    );
}

#[test]
fn bindings_mutation_and_scope() {
    parity(
        "bindings",
        &[
            "x ← 1
x",
            "n ↢ 0
n := n + 1
n := n + 1
n",
            "total ↢ 0
[1, 2, 3] → each { |i| total := total + i }
total",
            "x ← 1
◆ f() ⟦ ^ x + 1 ⟧
f()",
            "x ← 1
y ← ? true ⟦ x + 1 ⟧ : ⟦ 0 ⟧
y",
            "a ← 1
a2 ← a + 1
a2 * 2",
        ],
    );
}

#[test]
fn conditionals_and_truthiness() {
    parity(
        "conditionals",
        &[
            "? true ⟦ 1 ⟧ : ⟦ 2 ⟧",
            "? false ⟦ 1 ⟧ : ⟦ 2 ⟧",
            "? none ⟦ 1 ⟧ : ⟦ 2 ⟧",
            "? 0 ⟦ #truthy ⟧ : ⟦ #falsey ⟧",
            "? \"\" ⟦ #truthy ⟧ : ⟦ #falsey ⟧",
            "? [] ⟦ #truthy ⟧ : ⟦ #falsey ⟧",
            "? ⟨⟩ ⟦ #truthy ⟧ : ⟦ #falsey ⟧",
            "true and false",
            "false or true",
            "not none",
        ],
    );
}

#[test]
fn effects_reach_the_same_output() {
    // The value is `none` for all of these, so a value-only comparison would pass them
    // even if one path printed nothing at all.
    parity(
        "effects",
        &[
            r#"! @console.println("one")"#,
            r#"! @console.println("one")
! @console.println("two")"#,
            r#"[1, 2, 3] → each { |n| ! @console.println(str(n)) }"#,
            r#"◆! greet(who) ⟦ ! @console.println("hi " + who) ⟧
! greet("world")"#,
            r#"! @console.println(str(⟨a: 1⟩))"#,
            r#"! @console.println("a")
x ← 1 + 1
! @console.println(str(x))"#,
        ],
    );
}

#[test]
fn pipelines() {
    parity(
        "pipelines",
        &[
            "[1, 2, 3] → map { |n| n + 1 } → sum",
            "[1, 2, 3] → first",
            "[1, 2, 3] → last",
            "[] → first",
            "[\"a\", \"b\"] → join(\", \")",
            "5 → str",
            "([1, 2, 3] → count) > 2",
            "([1, 2] → sum) + 1",
            "1 + 2 → str",
            "[⟨name: \"a\"⟩, ⟨name: \"b\"⟩] → map .name → join(\"-\")",
        ],
    );
}

#[test]
fn atoms_and_equality() {
    parity(
        "atoms",
        &[
            "#ok",
            "#ok = #ok",
            "#ok != #err",
            "[#a, #b] → count",
            "⟨status: #ok⟩.status",
            "\"a\" = \"a\"",
            "[1, 2] = [1, 2]",
            "⟨a: 1⟩ = ⟨a: 1⟩",
            "1 = 1.0",
            // Atoms reaching a string must carry their name, not their interner index.
            "str(#ok)",
            "\"status={#ready}\"",
            "[#a, #b] → join(\", \")",
            "str([#a, ⟨s: #b⟩])",
        ],
    );
}

#[test]
fn the_ascii_dialect_lowers_identically() {
    // Same programs, both spellings: the dialects share one IR, so a divergence here
    // would mean the surface syntax leaked into lowering.
    parity(
        "ascii",
        &[
            "def f(x) [[ return x + 1 ]]
f(41)",
            "xs <- [1, 2, 3]
xs -> map { |n| n * 2 } -> sum",
            "r <- <<a: 1, b: 2>>
r.a + r.b",
            "match :ok [[\n  :ok -> 1\n  _ -> 2\n]]",
            "if true [[ 1 ]] else [[ 2 ]]",
            "n <~ 0
n := n + 5
n",
            "do host.console.println(\"ascii\")",
            "2 in [1, 2, 3]",
        ],
    );
}

#[test]
fn failures_agree_across_both_paths() {
    // Divergence on the error path is just as much a parity break as on the happy path,
    // and it is the direction a lowering bug is most likely to hide in.
    parity(
        "failures",
        &[
            "1 / 0",
            "\"a\" + 1",
            "~ 99 ⟦ 1 → #one ⟧",
            "◆ f(x) ⟦ ^ x ⟧
f()",
        ],
    );
}
