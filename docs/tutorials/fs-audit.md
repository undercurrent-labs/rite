# Auditing a directory

**You will build** a command-line tool that walks a directory, reports how much
is there, finds the biggest file, and flags anything that has not been touched
since a date you pick.

**You need** nothing but a Rite install — and a directory worth looking at.

This is the shape of most small operations scripts: expand a pattern, ask each
match about itself, aggregate, report.

## Find the files

```rite native_only
paths ← ! @fs.glob("logs/*.log")?
```

`@fs.glob` expands a pattern and answers a list of paths. `*` matches within one
directory segment and `**` walks down through them, so `logs/**/*.log` would find
nested files too.

The pattern is permission-checked twice, and the difference matters. A pattern
aimed **outside** your granted read roots — `/etc/ssh/*` — is a permission error
rather than an empty list, because the script asked for something it may not have
and silently answering "nothing here" would hide that. Individual *matches* that
fall outside a root are dropped quietly instead, because `**` legitimately walks
into places a narrower grant excludes.

## Ask each file about itself

```rite native_only
◆! describe(path) ⟦
  m ← ! @fs.metadata(path)?
  ^ ⟨path: path, len: m.len, mtime: m.mtime⟩
⟧
```

`@fs.metadata` answers a record: `len` in bytes, `is_file` and `is_dir`,
`is_symlink`, and `mtime`.

`describe` is declared `◆!` — with the marker — because it calls something
effectful. That is not a style choice: Rite infers effect-ness from the body and
checks it against the declaration, so leaving the `!` off is an error naming the
line that reaches out. The declaration is the contract a caller reads.

## Aggregate

```rite native_only
◆! main() ⟦
  paths ← ! @fs.glob("logs/*.log")?
  files ← map(paths, { |p| ! describe(p)? })

  ! println("files  " + str(count(files)))
  ! println("bytes  " + str(sum(map(files, { |f| f.len }))))

  biggest ← first(sort(files, { |a, b| b.len - a.len }))
  ! println("largest " + biggest.path + " (" + str(biggest.len) + ")")
⟧
```

```bash
rite run audit.rite --allow fs:read=.
```

```text
files  3
bytes  4102
largest logs/access.log (4000)
```

The lambda inside `map` performs an effect and unwraps a Result — `{ |p| !
describe(p)? }` — which is allowed exactly because the function containing it is
itself marked. Effects propagate through the call graph, so a pure `◆ main()`
holding this `map` would be rejected rather than quietly doing I/O.

## Flag what has gone stale

`mtime` is an RFC3339 UTC string — the same spelling `@clock.now` produces:

```text
2026-07-31T02:59:58.656493400+00:00
```

Which means you can compare timestamps with `<` and `>` directly:

```rite native_only
◆! main() ⟦
  cutoff ← "2026-01-01T00:00:00+00:00"
  paths ← ! @fs.glob("logs/*.log")?
  each(paths, { |p|
    m ← ! @fs.metadata(p)?
    ? m.mtime < cutoff ⟦ ! println("stale  " + p + "  " + m.mtime) ⟧
  })
⟧
```

```text
stale  logs/debug.log  2020-01-01T07:00:00+00:00
```

That works because RFC3339 in UTC sorts lexicographically — a plain string
comparison really is a time comparison, with no parsing step. It is also why the
format is fixed rather than configurable.

A literal cutoff is fine when the date is fixed, but "anything older than thirty
days" is the question you usually want. `@clock.add` with a negative duration says
it directly:

```rite native_only
cutoff ← @clock.add(! @clock.now(), "-30d")?
```

That is a timestamp in the same RFC3339 spelling, so the same `<` comparison works
against it — the arithmetic produces a value the ordering already understood.

## A wrinkle worth knowing: symlinks

`@fs.metadata` **follows** links. A symlink pointing at a file reports
`is_file: true` with the *target's* size — the same split `ls -l` shows. Only
`is_symlink` describes the path you actually asked about.

So a directory audit that adds up `len` will double-count a file that is also
linked to. If that matters, skip them:

```rite
keep(files, { |f| not f.is_symlink })
```

One case has no answer: a **broken** link cannot be detected. Following it fails
before anything can report on it, so `@fs.metadata` answers
`err(⟨kind: "io.not_found", …⟩)` rather than a record saying `is_symlink: true`.

## Permissions

```bash
rite run audit.rite --allow fs:read=.
```

Read access is scoped to a directory. `--allow fs:read=.` grants the working
directory and everything under it; the tool never needs write access, so do not
give it any.

## The whole script

Everything above, in one file. Save it as `audit.rite` beside a `logs/` directory:

```rite
// audit.rite — size a directory of logs and flag anything that has gone stale.

◆! describe(path) ⟦
  m ← ! @fs.metadata(path)?
  ^ ⟨path: path, len: m.len, mtime: m.mtime⟩
⟧

◆! main() ⟦
  cutoff ← "2026-01-01T00:00:00+00:00"

  paths ← ! @fs.glob("logs/*.log")?
  files ← map(paths, { |p| ! describe(p)? })

  ! println("files   " + str(count(files)))
  ! println("bytes   " + str(sum(map(files, { |f| f.len }))))

  biggest ← first(sort(files, { |a, b| b.len - a.len }))
  ! println("largest " + biggest.path + " (" + str(biggest.len) + ")")

  stale ← keep(files, { |f| f.mtime < cutoff })
  ! println("stale   " + str(count(stale)))
  each(stale, { |f|
    ! println("  " + f.path + "  " + f.mtime)
  })
⟧
```

```bash
rite run audit.rite --allow fs:read=.
```

Against three logs — a 4000-byte `access.log`, a 100-byte `error.log`, and a
2-byte `debug.log` last written in 2020:

```text
files   3
bytes   4102
largest logs/access.log (4000)
stale   1
  logs/debug.log  2020-01-01T07:00:00+00:00
```

Only `fs:read` is granted. The tool never writes, and a tool that never writes
should not be able to.

Those three files live in the repository, and CI runs this script against them on
every build and compares the output to what is printed above — including the
modification time, which is pinned so "stale" has something to find.

## Next

- [Reshaping JSON](json-pipeline.md) — the same pipeline shape over structured data
- [Files, JSON, and CSV](../book/files-json.md) — the rest of `@fs`
