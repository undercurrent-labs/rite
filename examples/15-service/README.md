# 15 Service

A small OSINT-collector service, shaped like the one in the scry-core field
report: several modules, subprocess collectors parsed with regexes, a work
queue, and one DuckDB connection shared by every handler.

It exists to exercise the pieces together rather than one at a time — a
private helper called across a module boundary, `parallel` fan-out, `break` /
`continue` / `^` in loops, `RETURNING *`, and concurrent handlers over a
single-writer database.

**Browser:** no (`@db`, `@process`, `@http.listen`)

**Run:**

```bash
rite run examples/15-service/main.rite \
  --allow db=. --allow process --allow net=127.0.0.1

curl -sS 127.0.0.1:8080/scan/a.example
curl -sS 127.0.0.1:8080/scan-all
curl -sS 127.0.0.1:8080/findings
curl -sS 127.0.0.1:8080/queue
```

| File | What it shows |
|---|---|
| `collectors.rite` | A public export calling a **private** sibling; raw-string regexes; `parallel` over an effectful function |
| `repo.rite` | `INSERT … RETURNING *` keeping its column names; the connection passed in, never opened per call |
| `work.rite` | `break`, `continue`, and `^` returning from inside a loop |
| `main.rite` | One connection opened before `listen` and shared by handlers; custom middleware alongside `log` / `recover` |

The database file is written next to the working directory as
`findings.duckdb`; `--allow db=.` is what grants that.
