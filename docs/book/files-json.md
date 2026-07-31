# Files, JSON, and CSV

Most automation scripts read files, parse structured data, transform records, and write outputs. Rite’s `@fs`, `@json`, and `@csv` capabilities cover that path.

## JSON encode / decode

```rite browser
data ← ⟨hello: "world", n: 1⟩
text ← @json.encode(data)
! @console.println(text)

decoded ← @json.decode(text)?
! @console.println(decoded)
```

- **`@json.encode(value)`** → string, compact  
- **`@json.encode_pretty(value)`** → string, indented two spaces  
- **`@json.decode(text)`** → `ok(value)` / `err(...)`  
- Use **`?`** to unwrap or propagate decode errors  

None of the three is marked: encoding and decoding are functions of their argument,
touching nothing outside the program. Compare `@json.read` / `@json.write` below,
which touch the disk and are marked for it.

Records and lists map to JSON objects and arrays. Atoms and richer values follow the runtime’s JSON mapping (prefer records/lists/strings/numbers/bools for portable data).

**Keys come out sorted, not in the order you wrote them.** A record is
insertion-ordered in Rite, but encoding sorts:

```rite browser
! @console.println(@json.encode(⟨name: "aura", tags: ["a"], n: 3⟩))
```

```text
{"n":3,"name":"aura","tags":["a"]}
```

That makes output stable — the same value always produces byte-identical JSON, so
it diffs cleanly and hashes consistently — but do not rely on field order to carry
meaning across the boundary.

### Straight to and from a file

`@json.read` and `@json.write` are the two-step versions collapsed:

```rite native_only
! @json.write("out.json", ⟨name: "aura", n: 3⟩)?
back ← ! @json.read("out.json")?
! @console.println(back.name)
```

```text
aura
```

`@json.write` writes the **pretty** form, on the assumption that a file is
something a person may open. It needs `fs:write`; `@json.read` needs `fs:read`.

```bash
rite run examples/03-files-and-json/main.rite --allow-all
```

## Reading files

```rite native_only
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

### Line by line

`@fs.lines` answers a list of strings with the line terminators removed:

```rite native_only
! @console.println(! @fs.lines("a.txt")?)
```

```text
[one, two, three]
```

A trailing newline does not produce a final empty element, so a three-line file is
three elements whether or not it ends in `\n`.

This still reads the **whole file into memory** first — it is a convenience over
`@fs.read`, not a streaming reader. There is no incremental line reader yet, so a
file bigger than you want resident is not something `@fs` can chew through today.

### Asking whether a path is there

```rite native_only
! @console.println(! @fs.exists("a.txt")?)
! @console.println(! @fs.exists("nope.txt")?)
```

```text
true
false
```

`exists` needs `fs:read`, because "is there a file called this" is information about
the filesystem. It answers `ok(true)` / `ok(false)` — the *absence* of a file is an
answer, not an error.

Note that checking before acting is inherently racy: anything can change between the
two calls. Prefer doing the operation and matching on its `err`, and keep `exists`
for choosing a code path rather than for guarding one.

## Writing files

```rite native_only
out ← @json.encode(⟨ok: true, count: 3⟩)
! @fs.write("output/result.json", out)
```

Requires **write** permission on the target path prefix:

```bash
rite run app.rite --allow fs:write=./output
```

`@fs.write` replaces the file. To add to one instead, use `@fs.append`, which
creates the file if it is not there yet — the shape a log wants:

```rite native_only
! @fs.append("run.log", "started\n")?
```

Neither call creates missing parent directories; see `@fs.mkdir` below.

## Rearranging files

| Call | Does | Needs |
|---|---|---|
| `@fs.mkdir(path)` | Creates a directory, and any missing parents | `fs:write` |
| `@fs.copy(from, to)` | Copies a file | `fs:read` on `from`, `fs:write` on `to` |
| `@fs.move(from, to)` | Renames or moves | `fs:write` on **both** |
| `@fs.remove(path)` | Deletes | `fs:write` |

> **`@fs.remove` on a directory is recursive.** It removes the directory and
> everything inside it, with no confirmation and no undo — `rm -rf`, spelled as one
> short call. There is no separate "remove empty directory" operation, so check what
> you are pointing it at. This is the single easiest way to lose data with `@fs`, and
> the narrowest possible `--allow fs:write=…` is the thing that limits the blast
> radius.

`@fs.mkdir` creates missing parents, so one call is enough for a whole branch:

```rite native_only
! @fs.mkdir("deep/nested/further")?
```

A path is resolved before it is permission-checked, and resolution copes with a
destination several levels away from anything that exists yet — but the *resolved*
location is what has to be inside the grant. `../outside` still fails, and so does a
path that climbs out through directories that were never created.

## Typical pipeline

```rite native_only
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

```rite browser
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

**A write grant implies read.** `--allow fs:write=./output` also lets the script
*read* anything under `./output`, without any `fs:read` grant:

```bash
rite run app.rite --allow fs:write=./output   # can also read ./output
```

The reasoning is that a script able to overwrite a file can generally discover its
contents anyway, so pretending otherwise would be security theatre. The practical
consequence is the one to remember: granting write to a directory is *not* a way to
let a script deposit files somewhere it cannot look. If read and write really must
differ, they have to be different directories.

Paths are resolved before they are checked, which is what stops `../` and a symlink
from walking out of a granted root — the check sees where the path really lands, not
how it was spelled.

## Errors

```rite native_only
outcome ← ! @fs.read("missing.txt")

~ outcome ⟦
  ok text → ! @console.println(text)
  err e → ! @console.println("read failed")
⟧
```

Or linear style:

```rite native_only
text ← ! @fs.read("missing.txt")?   // propagates err
```

## Listing and inspecting

`@fs.glob` expands a pattern; `@fs.metadata` describes what it found. Both are
reads, so both need `fs:read` covering the path.

```rite native_only
paths ← ! @fs.glob("logs/*.log")?
```

The pattern must point inside a granted read root — one aimed outside is a
permission error rather than an empty list, since the script asked for something
it may not have. Individual matches that fall outside a root are dropped quietly,
because `**` legitimately walks into places a narrower grant excludes.

`@fs.metadata` answers a record:

```rite native_only
m ← ! @fs.metadata("notes.txt")?
// ⟨len: 17, is_file: true, is_dir: false, is_symlink: false,
//  mtime: 2026-07-31T02:59:58.656493400+00:00⟩
```

| Field | |
|---|---|
| `len` | Size in bytes |
| `is_file`, `is_dir` | What the path resolves to |
| `is_symlink` | Whether the path *itself* is a symbolic link |
| `mtime` | Last modification, as an RFC3339 UTC string — or `none` if the filesystem does not record one |

**`is_symlink` is the odd one out**, deliberately: every other field describes
what the path *resolves to*, because `@fs.metadata` follows links. A symlink to a
file reports `is_file: true` with the target's length, the same split `ls -l`
shows. Only `is_symlink` describes the path you asked about.

One consequence worth knowing: a **broken** link cannot be detected. Following it
fails before anything can report on it, so `@fs.metadata` on a dangling symlink
returns `err(⟨kind: "io.not_found", …⟩)` rather than a record saying
`is_symlink: true`.

### Finding what changed

`mtime` is a string, and specifically the same string `@clock.now` produces, so
the two compare directly:

```rite native_only
◆! changed_since(dir, cutoff) ⟦
  paths ← ! @fs.glob(dir + "/*.log")?
  each(paths, { |p|
    m ← ! @fs.metadata(p)?
    ? m.mtime > cutoff ⟦ ! println(p) ⟧
  })
⟧
```

RFC3339 timestamps in UTC sort lexicographically, so `>` on the raw strings is a
real time comparison — no parsing step required. `@clock.parse` accepts an
`mtime` unchanged if you want one.

What you **cannot** do yet is arithmetic on them. There is no "seven days ago":
Rite has no date maths, so a cutoff has to be a timestamp you already hold —
one recorded on the last run, or a literal. Comparison is the whole toolkit.

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
