# Reshaping JSON

**You will build** a script that reads a file of orders, keeps the ones that
shipped, groups them by customer, and writes a ranked summary.

**You need** nothing but a Rite install. No network, no database.

Every block below was run to produce the output shown next to it.

## The data

Save this as `orders.json`:

```json
[
  { "id": "A-1001", "customer": "ada",    "total": 129.50, "status": "shipped" },
  { "id": "A-1002", "customer": "grace",  "total": 42.00,  "status": "pending" },
  { "id": "A-1003", "customer": "ada",    "total": 8.75,   "status": "shipped" },
  { "id": "A-1004", "customer": "linus",  "total": 310.00, "status": "cancelled" },
  { "id": "A-1005", "customer": "grace",  "total": 76.25,  "status": "shipped" }
]
```

## Read it, decode it

Two separate steps, and the split matters:

```rite native_only
◆! main() ⟦
  raw ← ! @fs.read("orders.json")?
  orders ← @json.decode(raw)?
  ! println(count(orders))
  ! println(orders → first)
⟧
```

```text
5
⟨customer: ada, id: A-1001, status: shipped, total: 129.5⟩
```

`@fs.read` is marked `!` because it touches the disk; `@json.decode` is not,
because it only transforms a string you already have. That distinction is the
whole point of [effects](../book/effects.md) — the marker shows where the program
reaches outside itself, and decoding never does.

Both answer a [Result](../book/results.md), so both take `?`. A missing file and
malformed JSON fail the same way, which is what you want: the script stops at the
first thing that went wrong, and says which.

Notice the record printed its fields in alphabetical order rather than the order
the file listed them. Records are unordered — if you need a fixed field order,
build it when you encode.

## Keep what shipped

```rite
shipped ← keep(orders, { |o| o.status = "shipped" })
```

`keep` takes what matches; `reject` takes what does not. There is no `filter` —
the pair is named for what it does to the data rather than making you remember
which way round the predicate reads.

`=` is comparison here. Rite binds with `←`, so `=` is never assignment and never
needs doubling.

## Group by customer

```rite
by_customer ← group(shipped, { |o| o.customer })
```

```text
[[ada,   [⟨… id: A-1001 …⟩, ⟨… id: A-1003 …⟩]],
 [grace, [⟨… id: A-1005 …⟩]]]
```

**`group` answers a list of `[key, items]` pairs, not a record.** That is worth
knowing before you write the next line, because it decides how you take the parts:
`first(g)` is the key and `last(g)` is the list of rows. A record would have lost
the ordering, and grouping is usually a step on the way to a *ranked* answer.

## Reduce each group to a row

```rite
map(by_customer, { |g|
  ⟨customer: first(g), orders: count(last(g)), total: sum(map(last(g), { |o| o.total }))⟩
})
```

`sum(map(…))` is the ordinary spelling of "add up one field". `sum` on an empty
list is `0`, so a group that somehow had no rows would not blow up here.

## Put it together

The whole transform is one pipeline, which is what Rite is shaped for:

```rite native_only
◆ summarize(orders) ⟦
  orders
    → { |os| keep(os, { |o| o.status = "shipped" }) }
    → { |os| group(os, { |o| o.customer }) }
    → { |gs| map(gs, { |g|
        ⟨customer: first(g), orders: count(last(g)), total: sum(map(last(g), { |o| o.total }))⟩
      }) }
    → { |rs| sort(rs, { |a, b| b.total - a.total }) }
⟧

◆! main() ⟦
  raw ← ! @fs.read("orders.json")?
  report ← summarize(@json.decode(raw)?)
  ! @fs.write("report.json", @json.encode(report))?
  ! println(@json.encode(report))
⟧
```

```bash
rite run summary.rite --allow fs:read=. --allow fs:write=.
```

```text
[{"customer":"ada","orders":2,"total":138.25},{"customer":"grace","orders":1,"total":76.25}]
```

`sort` takes a comparator returning a number: negative if the first argument
sorts earlier. `b.total - a.total` is therefore descending — subtracting the other
way round gives ascending, and there is no separate `reverse: true` flag to
remember.

## Why `summarize` takes no `!`

`summarize` is pure. It reads no file and writes none — it is handed a list and
answers a list — so it needs no effect marker, and Rite will tell you if you claim
otherwise. That is not bookkeeping for its own sake: a pure function is one you
can call from a test with a literal list, with no filesystem and no permissions,
and get the same answer every time.

Only `main` is marked `◆!`, and only because it touches the disk at both ends.

## Permissions

```bash
rite run summary.rite --allow fs:read=. --allow fs:write=.
```

Filesystem access is denied by default, and the grants are scoped to a directory
rather than being a blanket "yes". Run it without them and the script stops with
exit code 5 before reading anything:

```text
permission denied: fs:read permission denied for `orders.json`
```

That is worth doing once, deliberately, so you recognise it later.

## Next

- [Files and JSON](../book/files-json.md) — the rest of `@fs`, plus CSV
- [Auditing a directory](fs-audit.md) — the same shape, but the input is the
  filesystem itself
