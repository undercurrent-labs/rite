# Contributing tests

How the Rite project itself is tested. For testing *your own* Rite code, see
[Testing](testing.md).

This page is the process guard so **observable host behavior** does not regress while unit tests stay green.

## Philosophy

Prefer **contracts** over line coverage:

| Contract type | Example assertion |
|---------------|-------------------|
| Value | `eval("1+2") == 3` |
| Stream | access log contains `rite: GET /health 200` |
| Registration | middleware list is `["log","recover"]` after `use` |
| Process | CLI `rite run` exit code + stdout |
| Docs | book claims map to named tests |

Deleting `flush_handler_io` or turning `@http.log` into a no-op **must** fail a test.

## Suite map

| Area | Location |
|------|----------|
| Eval / sugar | `crates/rite-caps/tests/eval_edge_cases.rs`, `sugar_pack.rs`, `sugar_dual_dialect.rs` |
| HTTP e2e | `crates/rite-caps/tests/http_handlers.rs` |
| HTTP observability | `crates/rite-caps/tests/http_observability.rs` |
| Permissions | `crates/rite-caps/tests/permissions.rs` |
| Parse | `crates/rite-syntax/tests/*` |
| CLI / examples / docs contract | `crates/rite-cli/tests/*` |
| Conformance | `conformance/semantics/*` + `rite-test` |
| REPL | `crates/rite-repl` unit tests |

## HTTP I/O capture (in-process)

```rust
use rite_caps::http::{begin_test_io_capture, take_test_io_capture, last_registered_middleware};

begin_test_io_capture();
// spawn server + request …
let cap = take_test_io_capture();
assert!(cap.stdout.contains("handler-marker"));
assert!(cap.stderr.contains("rite: GET "));
assert!(last_registered_middleware().contains(&"log".into()));
```

Tests that touch HTTP **must** take `http_test_lock` / the shared mutex so servers do not race.

## PR checklist (caps / HTTP / CLI)

- [ ] Side effects asserted (stdout/stderr/files), not only return values  
- [ ] Glyph **and** ASCII form for new dual-dialect surface  
- [ ] Book/example claim linked to a test name  
- [ ] `cargo fmt --check` and `RUSTFLAGS=-Dwarnings` clean  

## Local commands

```bash
cargo test --workspace --all-features
cargo test -p rite-caps --test http_observability
cargo test -p rite-cli --test docs_contract --test example_gates
cargo fmt --all -- --check
RUSTFLAGS=-Dwarnings cargo check --workspace
```

## Release

Tag `vX.Y.Z` after CI is green. Installer pins via `RITE_VERSION=vX.Y.Z`.
