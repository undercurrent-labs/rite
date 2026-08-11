# Modules

As scripts grow, split them into files. Rite modules are **files** imported with `use`. Only **`pub`** declarations are exported.

## Layout

```text
examples/modules/
  math.rite      # library
  main.rite      # entry
```

### Library (`math.rite`)

```rite browser
//! Pure math helpers (public exports)

pub ◆ square(value) ⟦
  ^ value * value
⟧

pub ◆ double(value) ⟦
  ^ value * 2
⟧

◆ private_helper(x) ⟦
  ^ x
⟧
```

- `pub ◆` / `pub def` — visible to importers  
- Private `◆ private_helper` — same file only  

### Entry (`main.rite`)

```rite
use math

! @console.println(str(square(12)))
! @console.println(str(double(7)))
```

```bash
rite run examples/modules/main.rite --allow-all
```

The runtime resolves `use math` to `math.rite` next to the entry file (and configured module roots).

## Import rules

1. **`use name`** loads the module, brings its **public** exports into scope, and
   binds `name` as a qualifier — `square(12)`, `math.square(12)` and
   `@math.square(12)` all work.
2. **`use path as alias`** binds the qualifier under another name, and *only* under
   that name: `use math as m` gives `m.square(12)` and no bare `square`.
   `use math -> m` (glyph: `⊏ math → m`) is the same declaration.
3. **`use ./rel` / `use ../pkg/mod`** — path relative to the **importing file**.
4. **`pub use name`** — re-export that module's public names from this file (facades).
5. **Circular imports** are errors (`E024`) and include the import chain in diagnostics.
6. **Modules may import modules.** A module's own imports are private to it and are
   never re-exported; use `pub use` for that.
7. Prefer **acyclic** graphs: leaves = pure helpers, root = main/server.

## A module's own names are its own

A module's functions call each other — private helpers included — no matter
what the importing file declares. A binding named `double` in the entry does
not change what `math.square` does internally.

Two collisions are errors at `rite check` rather than surprises at runtime:

- **A top-level binding named like an imported function** (`E022`). The binding
  would replace the function for every bare call after it in the file. Rename
  the binding, or keep the module behind a qualifier with `use … as …`.
An export named like a builtin is handled differently: it is simply **not bound
to the bare name**. A module exporting `entries` stays perfectly usable as
`queue.entries(…)`, while a bare `entries(…)` keeps meaning the builtin it
meant before the import existed — so adding a `use` can never change what a
name already in the file does. The names that behave this way are the
[builtin reference](reference/builtins.md).

## The `@` qualifier

`@m.square(12)` is qualified module access through the sigil. It calls the same
function `m.square(12)` does, with one difference: `@m` always means the module.
A parameter or binding named `m` shadows the bare qualifier — `◆ f(m) ⟦ ^ m.x ⟧`
reads a field of the argument — but never the sigil form, so `@m.square` keeps
meaning the import wherever it appears.

```rite
use math as m

! @console.println(str(@m.square(12)))
f ← @m.square          // exports are values too
```

`@` is the same sigil capabilities use, and the two cannot collide: an import
may not bind a capability namespace as its qualifier. `use fs` is an error with
the fix in it — alias the module (`use fs as fsm`) and `@fs.read` still means
the host. Effect discipline is unchanged either way: calling an effectful
export needs its marker, `! @m.fetch(url)`, exactly as the bare spelling does.

An `@name` that is neither a capability nor an import is `E042` at `rite
check`, with the import to add named in the help.

## Two modules, one name

Nothing stops two modules from exporting the same name. Importing both is fine —
the clash is only reported if you then call the name **unqualified**, because only
then is it ambiguous:

```text
error[E022]: `helper` is exported by both `alpha` and `beta`
  help: call it as `alpha.helper` or `beta.helper`, or import one with `use … as …`
```

Qualify, and both stay usable:

```rite
use alpha
use beta

! @console.println(alpha.helper())
! @console.println(beta.helper())
```

Qualified calls are checked when you compile, not when you run. Reaching for
something a module does not export names the mistake up front:

```text
error[E020]: module `math` has no public `squre`
  help: `math` exports: double, square
```

### Relative imports

```text
app/
  main.rite
  lib/
    helpers.rite
```

```rite
// app/main.rite
use ./lib/helpers

! @console.println(str(triple(5)))
```

Resolves to `lib/helpers.rite` (or `lib/helpers/mod.rite`) next to the importer.

### Aliases

```rite
use math as m
! @console.println(str(m.square(12)))
```

`as` is the ASCII spelling and `→` the glyph one; the formatter converts
between them, and `->` parses in either dialect:

```rite
⊏ math → m
! @console.println(str(@m.square(12)))
```

### Re-exports

```rite
// facade.rite
pub use math

// main.rite
use facade
! @console.println(str(square(3)))
```

## What to export

| Export | Good for |
|--------|----------|
| Pure functions | Shared logic, testable in Studio |
| Constants / constructors | Small config helpers |
| Avoid exporting | Process/FS wrappers without clear ownership |

Keep side effects near the edges (`main`, HTTP handlers), not deep in utility modules.

## Visibility checklist

```rite browser
pub ◆ exported(x) ⟦ ^ x + 1 ⟧

◆ internal(x) ⟦ ^ x * 2 ⟧   // not imported by use
```

Importers cannot call `internal` unless you re-export or move it.

## Running module projects

Always run the **entry** file; libraries are pulled in automatically:

```bash
rite run path/to/main.rite --allow-all
rite check path/to/main.rite
```

`check` should resolve imports when files exist on disk.

## Multi-file analysis / LSP

The language server and analysis engine index imports for workspace symbols and references. Open the project root so `use` targets resolve.

## ASCII modules

```rite
// math.rite
pub def square(value) [[
  return value * value
]]

// main.rite
use math
do host.console.println(str(square(12)))
```

## Patterns

### Feature split

```text
app/
  main.rite       # listen + wire routes
  handlers.rite   # pub route helpers
  db.rite         # pub query wrappers (permissioned host)
```

### Shared pure core

```text
core/
  transform.rite
cli/
  main.rite
```

## Errors you’ll see

| Symptom | Likely cause |
|---------|----------------|
| Unresolved name after `use` | Forgot `pub` on definition |
| Module not found | Wrong relative path / cwd when running |
| `E024` cycle | A → B → A; break the cycle |

## Next

[Compiling to Rust](compiling.md) — native binaries and IR parity.
