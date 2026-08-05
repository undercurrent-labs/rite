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
- `rest` drops the first element, `butlast` drops the last. Both work as calls and as
  pipeline stages, and `..rest` is *also* a match pattern — the same word in two
  roles:

```rite browser
! @console.println(rest([10, 20, 30]))
! @console.println([10, 20, 30] → rest)
! @console.println(butlast([10, 20, 30]))
```

```text
[20, 30]
[20, 30]
[10, 20]
```

`init` is another name for `butlast`, and `tail` another for `rest`; they behave
identically, so pick one spelling and keep to it.

### These read strings and bytes too

A string is a sequence of characters and bytes are a sequence of numbers, so the
whole family works on all three and gives you back the kind you handed it:

```rite browser
! @console.println(take("abcde", 2))
! @console.println(drop("abcde", 2))
! @console.println(sort("cba"))
! @console.println(chunk("abcdef", 2))
! @console.println(str(first(bytes("abc"))))
```

```text
ab
cde
abc
[ab, cd, ef]
97
```

`take`, `drop`, `first`, `last`, `rest`, `init`, `reverse`, `sort`, `unique`,
`chunk` and `enumerate` all read strings and bytes, alongside `count`, `slice`,
`index_of`, `contains` and `repeat`, which always did. Characters mean characters,
not bytes — `take("héllo", 2)` is `"hé"`. A byte comes back as an int, which is
what `byte_at` answers.

`zip` and `flatten` are the exceptions, and they say so rather than answering
something: both are about the structure of a *list of lists*, which a string does
not have.

`sum`, `min`, `max` and `join` read all three kinds too — summing bytes is a
checksum, and `min` uses the ordering `sort` uses, so `min("cba")` is `"a"`.

### What can be ordered

`<`, `<=`, `>`, `>=`, `sort`, `min` and `max` all ask the same question, and it
does not have an answer for every pair:

| Ordered | How |
|---|---|
| numbers | numerically, `int` against `float` included |
| strings | by Unicode scalar |
| `bool` | `false` before `true` |
| bytes | lexicographically |
| lists | lexicographically — element by element, then by length |

Anything else raises: two different kinds, two atoms, two records, and `NaN`,
which is unordered against everything including itself.

```rite browser
! @console.println(str([1, 2] < [1, 3]))     // true
! @console.println(str(sort([3, 1, 2])))     // [1, 2, 3]
```

`"a" < 1` is an error rather than an answer. It used to be `false` — and `"a" <= 1`
and `"a" >= 1` were both **true**, because anything the comparison did not
understand was treated as equal. That also made `sort` on a mixed list hand back
the list unchanged: not sorted, not an error, just a plausible-looking answer.

Atoms and records are deliberately unordered. Atoms are symbols, and a record's
fields are in insertion order, so any ordering of either would be an artefact of
how the value was built rather than a property of what it means.

### Ordering it yourself

`sort` takes a comparator for everything the language will not order for you, and
for orders that are not the natural one. It answers a number: negative if the
first argument comes first, positive if the second does, zero if neither.

```rite browser
files ← [⟨name: "a", len: 3⟩, ⟨name: "b", len: 9⟩, ⟨name: "c", len: 5⟩]

biggest ← sort(files, { |a, b| b.len - a.len })
! @console.println(biggest → map { |f| f.name })   // ["b", "c", "a"]
```

Records have no order of their own, and this is why they do not need one.

**Handed the wrong kind, these raise.** `sum` of a list of strings, `keys` of
anything but a record, `lines` of a list, `flatten` of a string: each says so at
the call. They used to answer `0`, `[]` or `ok([])` — the same answers a correct
empty input gives, so the mistake became visible frames later, wearing a different
type's name.

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
! @console.println(flatten(grid))
```

```text
[1, 2, 3, 4]
```

The spaces are not style. `[[` is the ASCII spelling of `⟦`, the block opener, so
`[[1, 2], [3, 4]]` is a parse error — `expected RBracket`. Write `[ [1, 2], [3, 4] ]`
and the ambiguity disappears.

`flatten` removes exactly **one** level, so a three-deep list needs two passes.

## Asking a list a question

`keep` and `map` build a new list. These four answer something *about* one:

```rite browser
xs ← [3, 1, 2, 3, 4]

! @console.println(all(xs, { |n| n > 0 }))
! @console.println(any(xs, { |n| n > 3 }))
! @console.println(find(xs, { |n| n > 2 }))
! @console.println(find(xs, { |n| n > 99 }))
```

```text
true
true
3
none
```

`find` answers the **first matching element**, or `none` when nothing matches — not
a result, because "no match" is an ordinary outcome rather than a failure. Since
`none` is falsy you can test it directly with `?`.

`all` on an empty list is `true` and `any` on an empty list is `false`, which is the
conventional reading: nothing violates the predicate, and nothing satisfies it.

## Reshaping a list

```rite browser
xs ← [3, 1, 2, 3, 4]

! @console.println(unique(xs))
! @console.println(reverse(xs))
! @console.println(chunk(xs, 2))
```

```text
[3, 1, 2, 4]
[4, 3, 2, 1, 3]
[[3, 1], [2, 3], [4]]
```

`unique` keeps the **first** occurrence of each value and preserves order — it is not
a sort. `chunk` splits into fixed-size pieces and the final piece is short rather
than padded, so it suits batching a list of work into requests of at most *n*.

### Pairing things up

```rite browser
! @console.println(enumerate(["a", "b", "c"]))
! @console.println(zip([1, 2, 3], ["a", "b", "c"]))
```

```text
[[0, a], [1, b], [2, c]]
[[1, a], [2, b], [3, c]]
```

Both answer a list of two-element lists, so both destructure the same way in a
lambda or a match arm. `enumerate` pairs each element with its **index**, counting
from zero; `with_index` is another name for it. `zip` stops at the shorter input.

### Folding to a single value

`sum` and `count` are the common folds; `reduce` is the general one:

```rite browser
! @console.println(reduce([3, 1, 2], { |acc, n| acc + n }, 0))
```

```text
6
```

The order is `reduce(list, function, initial)`: **function second, seed last**. It
the callback receives `(accumulator, element)` in that order. Both are easy to get
backwards, and getting them backwards usually produces a confusing "cannot call
value" error rather than a wrong number.

## Inspecting values

```rite browser
! @console.println(keys(⟨name: "aura", n: 3⟩))
! @console.println(type_of([1]) + " " + type_of("s") + " " + type_of(1.5))
! @console.println(parse_float("3.25"))
```

```text
[name, n]
list string float
ok(3.25)
```

`keys` answers a record's field names **in insertion order** — worth contrasting
with [`@json.encode`](files-json.md), which sorts them. `type_of` answers a plain
string, useful for branching on a value from JSON whose shape you do not control.
`parse_float` answers a **result**, because "3.25" and "banana" are both strings and
only one of them is a number.

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

## Doing the slow parts together

`map` visits one item at a time. When each item means waiting — a request, a
file, a query — `parallel` runs the branches together instead:

```rite native_only
◆! fetch_one(url) ⟦
  ^ ! @http.get(url)?
⟧

pages ← ! parallel(urls, fetch_one)
```

It answers as if it had not. Results come back in **input order** however the
branches finish, anything a branch prints is spliced in input order too, and if
several fail the one reported is the first in input order — so the same program
prints the same thing twice running.

```rite browser
◆! step(n) ⟦
  ! @console.println("step " + str(n))
  ^ n * 2
⟧

! parallel([1, 2, 3], step)
```

Three consequences:

- **It is concurrency, not parallelism.** Branches interleave whenever one waits.
  Work that never waits, such as arithmetic or string building, gains nothing and should
  use `map`, which does not pay for the extra machinery.
- **Branches share the host.** A write to `@store` or `@db` in one branch is
  visible to the others and to the parent, because they share one host, not a
  copy of it.
- **Passing an effectful function marks the call** — `! parallel(urls, fetch_one)`
  — for the same reason `each(shout)` does. See
  [Effects and capabilities](effects.md).

## Next

[Pattern matching](matching.md) — destructure lists, records, atoms, and results.
