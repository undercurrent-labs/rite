# CLI reference

Every subcommand, argument and flag, generated from the command definitions themselves — so this page cannot describe a flag that is not there.

> Rite — esoteric Rust-backed scripting language

## `rite ast`

Dump AST

| Argument | Meaning |
|---|---|
| `<file>` | Script to parse |

| Flag | Meaning |
|---|---|
| `--json` | Print the tree as JSON |

## `rite build`

Compile a Rite script to a native binary

| Argument | Meaning |
|---|---|
| `<file>` | Script to compile |

| Flag | Meaning |
|---|---|
| `--release` | Build with optimisations (slower to build, faster to run) |
| `--emit-rust` | Write the generated Rust instead of linking a binary |
| `-o`, `--output` | Path for the compiled binary |
| `--allow-all` | Bake in every permission — trusted scripts only |
| `--allow` | Bake a permission into the binary, e.g. `fs:read=./data` |

## `rite capabilities`

List capabilities

## `rite check`

Lex, parse, resolve, and effect-check without executing

| Argument | Meaning |
|---|---|
| `<file>` | Script to check |

| Flag | Meaning |
|---|---|
| `--json-errors` | Report diagnostics as JSON on stderr instead of rendered text |

## `rite convert`

Convert dialect (ascii/glyph/mixed)

| Argument | Meaning |
|---|---|
| `<file>` | Script to convert |

| Flag | Meaning |
|---|---|
| `--to` | Target dialect: `glyph`, `ascii` or `mixed` |
| `--stdout` | Print the result instead of rewriting the file |
| `--check` | Exit 1 if the file is not already in the target dialect |

## `rite describe`

Machine-readable language description

## `rite describe capability`

One host capability and its functions

| Argument | Meaning |
|---|---|
| `<name>` | Capability name, e.g. `fs` or `http` |

| Flag | Meaning |
|---|---|
| `--json` | Emit JSON instead of text |

## `rite describe diagnostic`

One diagnostic code and what triggers it

| Argument | Meaning |
|---|---|
| `<code>` | Diagnostic code, e.g. `E021` |

| Flag | Meaning |
|---|---|
| `--json` | Emit JSON instead of text |

## `rite describe language`

Sigils, keywords, capabilities and diagnostics in one payload

| Flag | Meaning |
|---|---|
| `--json` | Emit JSON instead of text |

## `rite describe syntax`

Glyph and ASCII spellings for every construct

| Flag | Meaning |
|---|---|
| `--json` | Emit JSON instead of text |

## `rite doc`

Generate documentation

| Argument | Meaning |
|---|---|
| `<path>` | Rite file or directory whose `///` doc comments to include (optional; without it only the language reference is generated) |

| Flag | Meaning |
|---|---|
| `--out` | Directory to write the generated documentation into |

## `rite docs`

Documentation commands

## `rite docs agent`

Generate the agent skill bundle

| Flag | Meaning |
|---|---|
| `--output` | Output directory (default: <checkout>/skills/rite) |

## `rite docs build`

Generate reference docs (+ agent bundle) from a Rite checkout

| Flag | Meaning |
|---|---|
| `--out` | Output directory (default: <checkout>/docs/generated) |
| `--skill-out` | Agent skill bundle output (default: <checkout>/skills/rite) |
| `--no-skill` | Only generate reference docs, not the agent bundle |
| `--scripts` | Also document the `///` comments in this Rite file or directory |

## `rite docs check`

Run documentation doctests

| Flag | Meaning |
|---|---|
| `--out` | Regenerate reference docs here first (default: <checkout>/docs/generated) |
| `--book` | Book directory (default: <checkout>/docs/book) |
| `--tutorials` | Tutorials directory (default: <checkout>/docs/tutorials) |
| `--diagnostics` | Diagnostics directory (default: <checkout>/docs/diagnostics) |
| `--skill` | Agent skill directory (default: <checkout>/skills/rite) |

## `rite docs json`

Generate reference docs only (machine-readable JSON included)

| Flag | Meaning |
|---|---|
| `--out` | Output directory (default: <checkout>/docs/generated) |
| `--scripts` | Also document the `///` comments in this Rite file or directory |

## `rite docs open`

Open generated documentation for a symbol (or the index)

| Argument | Meaning |
|---|---|
| `<symbol>` | Symbol to open, e.g. `fs.read`; omit for the index |

| Flag | Meaning |
|---|---|
| `--root` | Documentation root (default: <checkout>/docs/generated) |
| `--print` | Print the resolved path instead of opening a browser |

## `rite docs serve`

Serve generated docs over loopback HTTP

| Flag | Meaning |
|---|---|
| `--port` | Port to serve the generated documentation on |
| `--root` | Directory to serve (default: <checkout>/docs/generated) |
| `--no-open` | Do not open a browser window on start |

## `rite emit-rust`

Emit generated Rust without full native link

| Argument | Meaning |
|---|---|
| `<file>` | Script to lower into Rust |

## `rite explain`

Show desugared forms

| Argument | Meaning |
|---|---|
| `<file>` | Script whose desugared form to print |

## `rite fmt`

Format Rite source (rewrites files in place)

| Argument | Meaning |
|---|---|
| `<paths>` | Files or directories to format; required unless --all is given |

| Flag | Meaning |
|---|---|
| `--all` | Format every .rite file under the current directory |
| `--ascii` | Shorthand for --dialect ascii |
| `--dialect` | Dialect to format to: `glyph` or `ascii` |
| `--check` | Exit 1 if any file would change (does not write) |
| `--dry-run` | List files that would change, then exit 0 (does not write) |

## `rite ir`

Dump semantic IR

| Argument | Meaning |
|---|---|
| `<file>` | Script to lower |

| Flag | Meaning |
|---|---|
| `--json` | Print the IR as JSON |

## `rite lsp`

Start language server (stdio)

## `rite repl`

Interactive REPL

| Flag | Meaning |
|---|---|
| `--allow-all` | Grant every permission for the session — local exploration only |

## `rite run`

Run a Rite script through the interpreter

Output written by the script is always emitted, including when the script fails. When the script's final expression evaluates to something other than `none`, that value is printed after the script's own output.

Arguments after `--` are readable with `! @process.args`.

| Argument | Meaning |
|---|---|
| `<file>` | Script to interpret |
| `<args>` | Script arguments (after `--`), readable with `! @process.args` |

| Flag | Meaning |
|---|---|
| `--allow` | Grant a permission, e.g. `fs:read=./data` or `net=api.example.com` |
| `--allow-all` | Grant every permission — trusted scripts only |
| `--deny` | Revoke a permission that is allowed by default (console, clock, random) |
| `--timeout` | Wall-clock limit for the run, e.g. `30s` or `5m` |
| `--max-steps` | Stop after this many evaluation steps |
| `--trace` | Print an execution summary (steps, elapsed) and stack traces to stderr |
| `--json-errors` | Report diagnostics as JSON on stderr instead of rendered text |

## `rite self-update`

Alias for `update`

| Flag | Meaning |
|---|---|
| `--check` | Only report; exit 1 if an update is available |
| `--force` | Reinstall even if versions match |
| `--version` | Install a specific release tag Release tag to fetch (default: latest) |

## `rite semantic-ir`

Dump semantic IR (alias of ir)

| Argument | Meaning |
|---|---|
| `<file>` | Script to lower |

| Flag | Meaning |
|---|---|
| `--json` | Print the IR as JSON |

## `rite skill`

Install / update the agent skill bundle (Grok, Claude, Cursor, …)

## `rite skill install`

Download the skill into the local cache and install into agent skill dirs

| Flag | Meaning |
|---|---|
| `--target` | Targets: grok, claude, cursor, project, all, cache (comma-separated) |
| `--dir` | Install to a specific directory (overrides --target) |
| `--from` | Source: local path, archive, or URL |
| `--version` | Release tag to fetch (default: latest) Release tag to fetch (default: latest) |
| `--force` | Refresh even if cache looks current |

## `rite skill path`

Print default skill install paths

## `rite skill status`

Show install state, cache path, and last pull time

## `rite skill update`

Re-fetch skill and reinstall to previously recorded paths

| Flag | Meaning |
|---|---|
| `--force` | Refresh even if the cache looks current |

## `rite studio`

Launch Rite Studio local service (loopback, token-authenticated)

| Flag | Meaning |
|---|---|
| `--port` | Port for the loopback API |
| `--no-open` | Do not open a browser window on start |
| `--project` | Project root: relative paths in executed scripts resolve here |
| `--allow-all` | Let Studio-executed scripts use the full host (fs, network, processes) |

## `rite syntax-tree`

Dump syntax tree (alias of ast)

| Argument | Meaning |
|---|---|
| `<file>` | Script to parse |

| Flag | Meaning |
|---|---|
| `--json` | Print the tree as JSON |

## `rite test`

Run Rite tests

| Argument | Meaning |
|---|---|
| `<paths>` | Files or directories to search (default: `tests` and `examples`) |

| Flag | Meaning |
|---|---|
| `--filter` | Only run tests whose name contains this substring |
| `--interpreted` | Run through the interpreter (the default) |
| `--compiled` | Run through the compiled path instead of the interpreter |
| `--both` | Run both ways and require the results to agree |
| `--json` | Emit results as JSON |

## `rite update`

Check for / install CLI and skill updates

| Flag | Meaning |
|---|---|
| `--check` | Only report; exit 1 if an update is available |
| `--force` | Reinstall even if versions match |
| `--version` | Install a specific release tag (e.g. v0.1.7) Release tag to fetch (default: latest) |

## `rite version`

Print version

| Flag | Meaning |
|---|---|
| `--verbose` | Include build details and the paths the CLI resolved |

## `rite vscode`

Download / install the VS Code (or Cursor) extension

## `rite vscode download`

Download the .vsix without installing

| Flag | Meaning |
|---|---|
| `-o`, `--out` | Output path for the .vsix |
| `--version` | Release tag to fetch (default: latest) |

## `rite vscode info`

Show release asset details and install instructions

| Flag | Meaning |
|---|---|
| `--version` | Release tag to fetch (default: latest) |

## `rite vscode install`

Download the .vsix and install via `code` / `cursor` if available

| Flag | Meaning |
|---|---|
| `--editor` | Editor CLI: code, cursor, codium (default: auto-detect) |
| `--download-only` | Only download; print path and metadata |
| `-o`, `--out` | Output path for the .vsix |
| `--version` | Release tag to fetch (default: latest) |

