---
name: rite
description: >
  Author and debug Rite scripts (glyph/ASCII dual syntax, explicit effects,
  capabilities @fs/@json/@csv/@db/@http, CLI). Use when writing .rite files,
  fixing Rite diagnostics, or using rite run/fmt/check/skill.
---

# Rite Agent Skill

**Language version:** 1  
**Tooling:** Rite CLI, LSP, Studio, WASM browser runtime

## Identity

Rite is a Rust-backed scripting language with dual **glyphic** and **ASCII** syntax, explicit effects, and capability-based host access.

## Critical rules

1. Do **not** invent syntax. Use only constructs from `machine/aliases.json` and `machine/grammar.ebnf`.
2. Effectful host calls require `!` (glyph) or `do` (ASCII). Missing markers are errors (`E021`).
   **Reads count**: `@fs.read`, `@db.query`, `@env.get`, `@clock.now`, `@process.args` all
   need `!`, not just writes. The marker goes on the call even when you bind or `?` the
   result: `text ← ! @fs.read(path)?`. Naming a capability without arguments still calls
   it, so `now ← ! @clock.now` needs the marker too — there is no way to capture a host
   function as a value. `machine/capabilities.json` has the `effectful` flag per function.
3. Truthiness: only `false` and `none` are falsey. Empty lists/strings/zero are truthy.
4. Bindings: `←`/`<-` immutable; `↢`/`<~` mutable with `:=` assignment. Both are
   statements, not expressions.
5. Pipelines pass the prior value as the first argument: `xs → map { |x| x * 2 }`.
   `→` binds **tighter than the operators**, so `xs → count > 2` is `(xs → count) > 2`.
   A stage is a name, a call, or a trailing-block call — never a bare operator expression.
6. Capabilities: `@console`, `@fs`, `@json`, `@csv`, `@db`, `@http`, `@clock`, `@env`, `@process`, `@random`, `@game`, `@store`.
7. Browser runtime forbids `@process`, `@db`, outbound `@http`, and unrestricted native
   FS/net; use virtual HTTP / memory FS in Studio.
8. Prefer `rite check` / `rite fmt` / `rite run --allow-all` for local validation.
   `rite fmt` preserves comments and the layout you wrote (a multi-line record stays
   multi-line, a one-line block stays inline).
9. Strings: `"{name}"` interpolates; `\{` or `{{` is a literal brace; `r"…"` is raw and
   never interpolates. A line starting with `(` or `[` begins a new statement — it is not
   applied to the previous line.

## Common workflows

```bash
rite run script.rite --allow-all
rite run script.rite -- alpha beta      # read with `! @process.args`
rite fmt script.rite --dialect glyph
rite convert script.rite --to ascii
rite check script.rite
rite build script.rite --allow-all
rite describe language --json
rite describe capability http --json    # exact signatures + effect flags
rite docs agent
rite doc src/                           # render `///` comments from your own scripts
```

## Documentation comments

`///` above a declaration documents it; `//!` at the top of a file documents the file.
Plain `//` is a comment and is never rendered. Tags: `@param <name> <text>`,
`@returns <text>`, `@effects <perm>`, `@permission <grant>`; a fenced block becomes an
example. `rite doc <path>` renders them; undocumented declarations are omitted.

```rite
//! Geometry helpers.

/// Area of a circle.
/// @param radius Distance from the centre to the edge.
/// @returns The area, as a float.
pub ◆ circle_area(radius) ⟦
  ^ 3.14159 * radius * radius
⟧
```

## Permissions

Default: console, clock and random allowed; fs, net, env, process and db denied.

```bash
rite run app.rite --allow fs:read=./data --allow net=api.example.com --allow db
rite run app.rite --deny console          # any grant can be denied
```

`--allow net=<host>` covers both directions: the bind address of `@http.listen` (loopback
needs no grant; `0.0.0.0` does) and the target host of an outbound `@http.get`.
`! @process.args` needs no grant — the arguments are the caller's own input.

## Glyph ↔ ASCII

| Glyph | ASCII |
|-------|-------|
| ◆ | def |
| ← | <- |
| ↢ | <~ |
| → | -> |
| ^ | return |
| ? | if |
| ~ | match |
| ! | do |
| @ | host. |
| #name | :name |
| ⟦ ⟧ | [[ ]] |
| ⟨ ⟩ | << >> |
| ∈ | in |
| ∉ | not in |
| ⊏ | use |
| : (else branch) | else |
| ‥ | ..= |
| ..rec (spread) | ..rec |

## Minimal example

```rite
◆ greet(name) ⟦
  ^ "hello, {name}"
⟧
! @console.println(greet("Aura"))
```

## Records

```rite
base ← ⟨host: "localhost", port: 80⟩
prod ← ⟨..base, port: 443⟩      // spread: later entries win; same as base + ⟨port: 443⟩
```

## HTTP (native)

Serving — a handler returns status and body by juxtaposition:

```rite
@http.listen "127.0.0.1:0" ⟦
  ⊏ @http.log
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
  GET "/echo/:word" |req| ⟦
    ^ 200 ⟨echo: req.path.word⟩
  ⟧
⟧
```

Calling out — the response has the same shape a handler receives:

```rite
resp ← ! @http.get("https://api.example.com/status")?   // --allow net=api.example.com
body ← resp.json?
! @console.println(str(resp.status) + " " + body.message)
```

`@http.post(url, ⟨…⟩)` sends a record as JSON; `@http.request(⟨method, url, headers, body⟩)`
is the general form. A refused connection or timeout is `err(⟨kind: "net.error", …⟩)`,
not a crash.

## When stuck

- Read `machine/capabilities.json` for APIs.
- Read `machine/diagnostics.json` for error codes.
- Prefer pipelines and records over ad-hoc strings.
- Never treat generated Rust as the language surface.
