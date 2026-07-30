# Collections

Lists and records are the structured data tools you’ll use in almost every script.

## Lists

### Literals

```rite browser
empty ← []
nums ← [1, 2, 3, 4, 5]
mixed ← [1, "two", #three]
```

### Building transforms

Prefer pipelines (see [Pipelines](pipelines.md)):

```rite browser
xs ← [1, 2, 3, 4, 5, 6]

evens ← xs → keep { |n| n % 2 = 0 }
doubled ← xs → map { |n| n * 2 }
total ← xs → sum
n ← xs → count
```

### Indexing and ends

- Pipeline stages: `first` / `last` / `count` / `sum` / …  
- Empty list: `[] → first` and `[] → last` yield **`none`** (no panic).  
- **Rest is a match pattern**, not a pipeline stage: write `[h, ..rest]` in `~` / `match`, not `xs → rest`.

```rite browser
pair ← [10, 20, 30]
head ← pair → first
tail_sum ← ~ pair ⟦
  [h, ..rest] → rest → sum
  _ → 0
⟧
! @console.println(str(head))       // 10
! @console.println(str(tail_sum))   // 50
```

### Nested lists

```rite browser
// Nested lists: spaces keep `[[` from being read as a block opener
grid ← [ [1, 2], [3, 4] ]
// flatten when you need a single level — use flatten/builtin if available
```

## Records

### Literals

Glyph:

```rite browser
user ← ⟨
  id: 1,
  name: "Aura",
  roles: [#admin, #ops]
⟩
```

ASCII:

```rite browser
user <- <<
  id: 1,
  name: "Aura",
  roles: [:admin, :ops]
>>
```

### Field access

```rite
! @console.println(user.name)
missing ← user.email     // none if absent
! @console.println(missing)
```

Missing fields yield **`none`**, not an exception — combine with `??`:

```rite
email ← user.email ?? "nobody@example.com"
```

### Merge (`+`)

Record `+` is **right-biased merge**: keys on the right overwrite the left.

```rite browser
defaults ← ⟨host: "localhost", port: 8080, debug: false⟩
overrides ← ⟨port: 9090⟩
cfg ← defaults + overrides
// host: localhost, port: 9090, debug: false
! @console.println(cfg)
```

Useful for config layers and HTTP response shaping.

### Dynamic-ish updates

There is no heavy OOP “set field” model. Build a new record by merge:

```rite browser
base ← ⟨count: 0⟩
next ← base + ⟨count: 1⟩
```

## Records vs maps

Records are the v1 associative structure (implementation may use ordered maps). Keys are typically identifiers, strings, or atoms. Nested records are fine:

```rite browser
doc ← ⟨
  meta: ⟨version: 1, kind: #note⟩,
  body: "hello"
⟩
! @console.println(doc.meta.kind)
```

## Membership

```rite browser
xs ← [1, 2, 3]
// glyph: n ∈ xs   /  n ∉ xs
// ascii: n in xs  /  n not in xs
```

Use membership in `keep` predicates and guards.

## JSON round-trip

Lists and records map naturally to JSON via `@json` (see [Files and JSON](files-json.md)):

```rite browser
data ← ⟨hello: "world", n: 1⟩
text ← @json.encode(data)
again ← @json.decode(text)?
```

## When to use which

| Need | Prefer |
|------|--------|
| Ordered sequence, map/filter/reduce | **List** + pipelines |
| Named fields, config, JSON objects | **Record** |
| Status tags | **Atoms** (`#ok`) |
| Success/failure | **Result** (`ok` / `err`) |

## Examples

```bash
rite run examples/01-values/main.rite --allow-all
rite run examples/02-pipelines/main.rite --allow-all
rite run examples/03-files-and-json/main.rite --allow-all
```

## Next

[Pattern matching](matching.md) — destructure lists, records, atoms, and results.
