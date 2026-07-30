# Files, JSON, and CSV

Most automation scripts read files, parse structured data, transform records, and write outputs. Rite’s `@fs`, `@json`, and `@csv` capabilities cover that path.

## JSON encode / decode

```rite
data ← ⟨hello: "world", n: 1⟩
text ← @json.encode(data)
! @console.println(text)

decoded ← @json.decode(text)?
! @console.println(decoded)
```

- **`@json.encode(value)`** → string  
- **`@json.decode(text)`** → `ok(value)` / `err(...)`  
- Use **`?`** to unwrap or propagate decode errors  

Records and lists map to JSON objects and arrays. Atoms and richer values follow the runtime’s JSON mapping (prefer records/lists/strings/numbers/bools for portable data).

```bash
rite run examples/03-files-and-json/main.rite --allow-all
```

## Reading files

```rite
text ← ! @fs.read("data/config.json")?
cfg ← @json.decode(text)?
host ← cfg.host ?? "localhost"
! @console.println(host)
```

Requires filesystem **read** permission:

```bash
rite run app.rite --allow fs:read=./data
# or
rite run app.rite --allow-all
```

## Writing files

```rite
out ← @json.encode(⟨ok: true, count: 3⟩)
! @fs.write("output/result.json", out)
```

Requires **write** permission on the target path prefix:

```bash
rite run app.rite --allow fs:write=./output
```

## Typical pipeline

```rite
// 1. read
raw ← ! @fs.read("input.json")?

// 2. decode
doc ← @json.decode(raw)?

// 3. transform (pure)
items ← doc.items ?? []
total ← items → map { |it| it.amount ?? 0 } → sum

// 4. encode + write
report ← ⟨total: total, count: items → count⟩
! @fs.write("output/report.json", @json.encode(report))
! @console.println("wrote report, total=" + str(total))
```

Keep steps 3 pure when you can — easier to test in Studio without FS.

## CSV encode / decode

```rite
rows ← [
  ⟨name: "Ada", age: "36"⟩,
  ⟨name: "Bob", age: "42"⟩
]
text ← @csv.encode(rows)
! @console.println(text)

decoded ← @csv.decode(text)?
! @console.println(decoded)
```

| Call | Meaning |
|------|---------|
| `@csv.decode(text, opts?)` | Parse CSV → `ok(list<record>)` / `err(...)` |
| `@csv.encode(rows, opts?)` | Serialize list of records (or list of lists) → string |
| `@csv.read(path, opts?)` | Read file + decode (needs `fs:read`) |
| `@csv.write(path, rows, opts?)` | Encode + write file (needs `fs:write`) |

**Options** (optional record, all default-friendly):

| Field | Default | Notes |
|-------|---------|-------|
| `headers` | `true` | First row is column names when decoding; write a header row when encoding records |
| `delimiter` | `","` | Single-character field separator |
| `skip_empty` | `true` | Drop blank lines on decode |

With `headers: true` (default), each row is a **record** of string fields. With `headers: false`, each row is a **list** of string cells. Values are strings by default for safe round-trips.

```rite
// TSV
rows ← @csv.decode(text, ⟨delimiter: "\t"⟩)?
```

```bash
rite run examples/csv/main.rite --allow-all
```

## Paths and safety

- Prefer **relative paths** under known project directories  
- Grant the **narrowest** `--allow fs:…` prefix  
- Never `--allow-all` in production wrappers if you can avoid it  
- Path checks are enforced by the permission layer (escape attempts should fail closed)

## Errors

```rite
outcome ← ! @fs.read("missing.txt")

~ outcome ⟦
  ok text → ! @console.println(text)
  err e → ! @console.println("read failed")
⟧
```

Or linear style:

```rite
text ← ! @fs.read("missing.txt")?   // propagates err
```

## Listing and other FS ops

Depending on the installed `@fs` surface (see `rite docs build` / `rite capabilities`):

- read / write text  
- list directories  
- existence helpers  

Always assume **permissioned** access.

## Studio note

Hosted WASM Studio does **not** give full native filesystem access. Use:

- `@json` on in-memory strings  
- pure transforms  
- local `rite run` or `rite studio` for real files  

## Related examples

```bash
rite run examples/03-files-and-json/main.rite --allow-all
rite run examples/data-pipeline/summarize.rite --allow-all   # if present
```

## Next

[HTTP services](http.md) — listen, routes, and request JSON.
