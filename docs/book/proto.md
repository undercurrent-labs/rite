# Protocol buffers

`@proto` reads and writes protobuf wire format. Protobuf is not self-describing —
the bytes carry field *numbers*, not names or types — so every call starts from a
schema.

`@proto` is a native capability. The browser build omits it: the `.proto` compiler
and the reflection machinery add about a megabyte to a bundle that is otherwise
under one, so Studio answers `@proto` by name rather than shipping it. Everything
on this page needs the CLI.

## A schema is a handle

Compiling a schema answers a handle. Every other call takes it as its first
argument.

```rite native_only
schema ← ! @proto.load_file("api/user.proto")?

! @console.println(@proto.messages(schema)?)
```

`load_file` reads the disk, so it needs `--allow fs:read=.` like any other read.
Compiling from a string in memory touches nothing:

```rite native_only
src ← "syntax = \"proto3\"; package demo; message Ping {{ int64 n = 1; }}"
schema ← ! @proto.compile(src)?

! @console.println(@proto.messages(schema)?)
```

Note the doubled braces. A plain Rite string interpolates `{…}`, and `.proto` is
mostly braces, so an inline schema needs `{{` and `}}` throughout. That is why
`load_file` is the form to reach for; `compile` is for a schema your program
built or received.

Four ways in, all answering a handle:

| Call | Source |
|---|---|
| `@proto.compile(src)` | one `.proto` as text |
| `@proto.compile_all(⟨name: src, …⟩)` | several that import each other |
| `@proto.load_file(path)` | a `.proto` on disk |
| `@proto.load_set(bytes)` | an encoded `FileDescriptorSet` from `protoc --descriptor_set_out` |

All four are effectful — a handle is state the capability hands out — so all four
take `!`. Nothing else on this page does.

## Encoding and decoding

`decode` and `encode` carry no marker. The schema arrives as an argument and a
compiled schema never changes, so both are functions of what they are given, the
way `@json.decode` is.

```rite native_only
src ← "syntax = \"proto3\"; package demo; message User {{ int64 id = 1; string name = 2; }}"
schema ← ! @proto.compile(src)?

body ← @proto.encode(schema, "demo.User", ⟨id: 7, name: "ada"⟩)?
! @console.println(to_hex(body))

back ← @proto.decode(schema, "demo.User", body)?
! @console.println(back.name)
```

```
08071203616461
ada
```

`encode` answers bytes — the same type `@fs.read_bytes`, `@udp` and `@http`
bodies use, so a message can go straight out a socket.

## How fields map

| Protobuf | Rite |
|---|---|
| `int32` `int64` `sint*` `sfixed*` `uint32` | int |
| `uint64` `fixed64` | int, or `err` past `9223372036854775807` |
| `float` `double` | float |
| `bool` | bool |
| `string` | string |
| `bytes` | bytes |
| enum | atom |
| message | record |
| `repeated T` | list |
| `map<K, V>` | record |

Enums answer atoms, so a decoded field matches and compares like one written in
the source:

```rite native_only
src ← "syntax = \"proto3\"; package demo; enum Tier {{ FREE = 0; PRO = 1; }} message U {{ Tier tier = 1; }}"
schema ← ! @proto.compile(src)?

u ← @proto.decode(schema, "demo.U", @proto.encode(schema, "demo.U", ⟨tier: #PRO⟩)?)?

! @console.println(~ u.tier ⟦
  #FREE → "free tier"
  #PRO → "paid"
  _ → "unknown"
⟧)
```

```
paid
```

A number the schema has no name for — a variant added by a newer `.proto` —
decodes to the number rather than failing.

Rite has no unsigned 64-bit integer. A `uint64` above `9223372036854775807`
answers `err` instead of silently becoming a float or a string, so a field's Rite
type does not change with its magnitude.

## Presence

proto3 does not put an unset scalar on the wire, and a scalar set to its default
is indistinguishable from one never set. The decoded record holds **only the
fields the message actually set**:

```rite native_only
src ← "syntax = \"proto3\"; package demo; message U {{ int64 id = 1; string name = 2; }}"
schema ← ! @proto.compile(src)?

empty ← @proto.encode(schema, "demo.U", ⟨⟩)?
! @console.println(len(empty))
! @console.println(@proto.decode(schema, "demo.U", empty)?)
```

```
0
⟨⟩
```

An absent field reads as `none`, so match it or compare against `none` rather
than expecting a zero:

```rite native_only
src ← "syntax = \"proto3\"; package demo; message U {{ int64 id = 1; }}"
schema ← ! @proto.compile(src)?
u ← @proto.decode(schema, "demo.U", bytes(""))?

! @console.println(u.id)
! @console.println(~ u.id ⟦ none → 0 ⟧)
```

```
none
0
```

Mark a field `optional` in the `.proto` when the difference matters — that is
what protobuf's own presence tracking is for, and Rite honours it.

## Errors

Everything answers `ok`/`err`. A schema that does not parse, bytes that are not a
valid message, a name the schema does not define, and a record key that is not a
field are all data:

```rite native_only
src ← "syntax = \"proto3\"; package demo; message U {{ int64 id = 1; }}"
schema ← ! @proto.compile(src)?

! @console.println(@proto.encode(schema, "demo.U", ⟨nmae: 1⟩))
```

```
err(⟨kind: proto.encode, message: `demo.U` has no field named `nmae`⟩)
```

A key that is not a field is refused rather than dropped. Dropping it would
encode a message that looks valid and is missing the data a typo'd field name was
meant to carry.

## What is not covered

- **Unknown fields are not preserved.** Decoding and re-encoding a message drops
  fields the schema does not define.
- **Well-known types** (`Timestamp`, `Duration`, `Any`, `Struct`) decode as plain
  messages, not as Rite dates or values.
- **Services** are ignored. `@proto` is a codec; there is no gRPC client here.

## Permissions

Only `load_file` needs a grant, and it is an ordinary file read:

```bash
rite run app.rite --allow fs:read=./schemas
```

Compiling from a string, decoding and encoding need nothing: they compute over
their arguments.
