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

1. **`use name`** loads the module and brings **public** exports into scope.  
2. **`use path as alias`** — call exports as `alias.name(...)` (e.g. `use math as m` → `m.square(12)`).  
3. **`use ./rel` / `use ../pkg/mod`** — path relative to the **importing file**.  
4. **`pub use name`** — re-export that module’s public names from this file (facades).  
5. **Circular imports** are errors (`E024`) and include the import chain in diagnostics.  
6. Prefer **acyclic** graphs: leaves = pure helpers, root = main/server.

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
