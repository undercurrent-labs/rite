# Cant conformance fixtures

Mirrors `conformance/` for Rite, with the same rule: **adding language behaviour
without a fixture here is incomplete work.**

| Directory | What it holds | Consumed by |
|---|---|---|
| `syntax/` | valid ASCII sources, one `case.cant` each | `cant-syntax/tests/fixtures.rs` — must parse with no diagnostics |
| `dialect/` | `ascii.cant` + `glyph.cant` pairs | `cant-syntax/tests/fixtures.rs` — both must parse and produce the *same* span-free structure |
| `diagnostics/` | invalid sources with the `expected.code` they must report | `cant-syntax/tests/fixtures.rs` — the first error's code must match |
| `graph/` | sources that parse but whose graph is wrong | `cant-sem/tests/fixtures.rs` — the validator's first error must match |
| `lowering/` | `case.cant` and the exact `expected.rite` it expands to | `cant/tests/expand.rs` — golden expansion, and Rite must accept it |
| `execution/` | `case.cant`, `expected.exit`, optional `expected.value`, optional `permissions.toml` | `cant-cli/tests/differential.rs` |

A fixture that cannot be read is a failure, not a skip. A fixture directory that
no test walks is also a failure — `every_fixture_directory_is_reachable` fails if
one is added under a name the runner does not know, so a fixture cannot be
silently orphaned.

## The differential harness

`conformance/cant/execution/` is run three ways by
`crates/cant-cli/tests/differential.rs`, and the three must agree on value,
stdout, normalized stderr and exit code:

1. `cant run case.cant`
2. `cant expand case.cant` piped into `rite run`
3. `cant build case.cant` and the binary it produces

The third is `#[ignore]`d — each fixture is a cold `cargo` build of a generated
crate. Run it with:

```bash
cargo test -p cant-cli --test differential -- --ignored
```

Stderr is compared by *outcome*, not text. `cant run` reports a failure as a Cant
diagnostic pointing at the `.cant`; `rite run` reports the same failure as a Rite
one pointing at generated code. Requiring those to match would be requiring the
remapping not to work.

Regenerating a golden expansion, when a lowering change is intended:

```bash
(cd conformance/cant/lowering/<case> && cant expand case.cant > expected.rite)
```

The logical name `case.cant` matters — it is embedded in the generated header and
in runtime messages, so a golden generated from an absolute path would fail on
every other machine.
