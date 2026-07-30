# Databases (`@db`)

Rite’s **`@db`** capability runs SQL via **DuckDB** on the native CLI host. Connections and prepared statements are opaque handles. Browser Studio / WASM does **not** include DuckDB — use `rite run` for database scripts.

## Permissions

| Spec | Allows |
|------|--------|
| `--allow db` | In-memory databases only (`:memory:`) |
| `--allow db=./data` | File DBs under that path prefix (+ memory) |
| `--allow-all` | Unrestricted |

Default secure posture: **db denied**.

```bash
rite run script.rite --allow db
rite run script.rite --allow db=./data
```

## Open / close

```rite browser
conn ← ! @db.open()?              // in-memory
// conn ← ! @db.open(":memory:")?
// conn ← ! @db.open("./data/app.duckdb")?   // needs --allow db=./data

! @db.close(conn)?
```

`open` / `exec` / `query` / … return **`ok` / `err`** results (use `?` or match).

## Exec and query

```rite browser
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE events(id INTEGER, name VARCHAR)")?
! @db.exec(conn, "INSERT INTO events VALUES (1, 'boot'), (2, 'tick')")?

rows ← ! @db.query(conn, "SELECT name FROM events ORDER BY id")?
// rows is a list of records: [⟨name: "boot"⟩, ⟨name: "tick"⟩]

! @console.println(rows)
! @db.close(conn)?
```

Optional **params** as a list (positional `?` in SQL):

```rite
rows ← ! @db.query(conn, "SELECT * FROM events WHERE id = ?", [1])?
```

## Prepared statements

```rite
stmt ← ! @db.prepare(conn, "INSERT INTO events VALUES (?, ?)")?
! @db.exec_prepared(stmt, [3, "done"])?
rows ← ! @db.query_prepared(stmt_select, [3])?   // if prepared as SELECT
! @db.close_stmt(stmt)?
```

## Transactions

```rite
! @db.begin(conn)?
! @db.exec(conn, "INSERT INTO events VALUES (99, 'temp')")?
! @db.rollback(conn)?   // or @db.commit(conn)?
```

## CSV + SQL

You can load CSV via `@csv` then insert, or use DuckDB’s own readers when the path is allowed under `--allow db=…`:

```rite native_only
// Pure Rite path
rows ← ! @csv.read("data/events.csv")?
// … insert into @db …

// DuckDB path (file must be under db allow root)
// rows ← ! @db.query(conn, "SELECT * FROM read_csv_auto('data/events.csv')")?
```

## Example

```bash
rite run examples/11-db/main.rite --allow db
```

## Next

[Files, JSON, and CSV](files-json.md) · [Effects and capabilities](effects.md)
