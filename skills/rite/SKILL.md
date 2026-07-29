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
3. Truthiness: only `false` and `none` are falsey. Empty lists/strings/zero are truthy.
4. Bindings: `←`/`<-` immutable; `↢`/`<~` mutable with `:=` assignment.
5. Pipelines pass the prior value as the first argument: `xs → map { |x| x * 2 }`.
6. Capabilities: `@console`, `@fs`, `@json`, `@csv`, `@db`, `@http`, `@clock`, `@env`, `@process`, `@random`, `@game`, `@store`.
7. Browser runtime forbids `@process` and unrestricted native FS/net; use virtual HTTP / memory FS in Studio.
8. Prefer `rite check` / `rite fmt` / `rite run --allow-all` for local validation.

## Common workflows

```bash
rite run script.rite --allow-all
rite fmt script.rite --dialect glyph
rite convert script.rite --to ascii
rite check script.rite
rite build script.rite --allow-all
rite describe language --json
rite docs agent
```

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

## Minimal example

```rite
◆ greet(name) ⟦
  ^ "hello, {name}"
⟧
! @console.println(greet("Aura"))
```

## HTTP (native)

```rite
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

## When stuck

- Read `machine/capabilities.json` for APIs.
- Read `machine/diagnostics.json` for error codes.
- Prefer pipelines and records over ad-hoc strings.
- Never treat generated Rust as the language surface.
