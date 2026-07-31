# Text RPG tutorial

Rite includes a **`@game`** capability for small parser-style adventures: rooms, items, inventory, look/go, and save/load. It’s both a fun demo and a stress test for records, atoms, effects, and results.

## Run the sample

```bash
rite run examples/text-rpg/game.rite --allow-all
# or the ladder copy:
rite run examples/09-text-rpg/main.rite --allow-all
```

ASCII twin: `examples/text-rpg/game.ascii.rite`.

## Core loop

1. **Register** items and rooms  
2. **`@game.start(room_atom)`** — set initial location  
3. **Commands** — `look`, movement, `take`, inventory  
4. Optional **save/load** as data  

## Register content

```rite browser
! @game.register_item(#glyph_key, ⟨
  name: "Violet Glyph Key",
  weight: 1
⟩)

! @game.register_room(#atrium, ⟨
  text: "Rain of neon drips from a broken skylight.",
  exits: ⟨east: "corridor", south: "garden"⟩
⟩)

! @game.register_room(#corridor, ⟨
  text: "A narrow corridor of fractured violet glass.",
  exits: ⟨west: "atrium", east: "vault"⟩
⟩)
```

Conventions used in the sample:

- Room / item ids as **atoms** (`#atrium`)  
- `text` — description for `look`  
- `exits` — record of direction → room name/id string  

## Start and explore

```rite
! @game.start(#atrium)
! @game.take(#glyph_key)
! @console.println(@game.look())

! @game.command("east")
! @console.println(@game.look())

! @game.command("inventory")
msgs ← @game.messages()
! @console.println(msgs)
```

| Call | Role |
|------|------|
| `@game.register_world(id, record)` | World metadata — title and the like |
| `@game.register_room(id, record)` | Add a room |
| `@game.register_item(id, record)` | Add an item |
| `@game.start(room)` | Spawn player state |
| `@game.look()` | Current room description |
| `@game.go(exit)` | Move through a named exit |
| `@game.command(text)` | Parse a simple command line |
| `@game.take(item)` | Pick up if present |
| `@game.drop(item)` | Put down, into the current room |
| `@game.inventory()` | List carried item ids |
| `@game.reveal(id)` | Reveal a room or set a flag |
| `@game.messages()` | Drain pending messages |
| `@game.state()` | Snapshot of world/player |
| `@game.save()` / `@game.load(s)` | JSON round-trip, below |

`go` takes the **exit** atom rather than the destination — `@game.go(#north)` where
`north` is a key of the room's `exits` record. It fails with `not in a room` if
nothing has called `start` yet.

`look`, `inventory`, `messages`, `state` and `save` are the unmarked ones: they read
game state the program itself built, so they need no `!`. Everything that changes the
world is marked.

> **`@game.say` cannot be called from Rite.** `say` is a language keyword (the
> shorthand for printing, see [Syntax sugar](sugar.md)), so `@game.say("…")` does not
> parse as a capability call and fails at runtime with `unknown @game.`. Use
> `@console.println` for narration, or let `@game.command` produce messages and drain
> them with `@game.messages()`.

## Save / load

```rite browser
save ← @game.save()?
! @console.println("saved")
! @game.load(save)?
! @console.println("reloaded")
! @console.println(@game.state())
```

Save returns a **result** — use `?` or match. Persist the payload with `@fs.write` if you want files on disk (needs FS permissions).

## Designing a tiny game

1. **Map** — 5+ rooms with sensible exits (the Violet Gate sample does this)  
2. **Items** — keys/tokens that unlock text or exits (extend with your own flags in records)  
3. **Verbs** — rely on `command` for `north`/`east`/`inventory`/`look` style input  
4. **Story state** — store progress in game state or your own records  

## Extending with pure Rite

Keep narrative logic in functions:

```rite browser
◆ describe_exit(dir, place) ⟦
  // pure helper for custom UIs (`room` is a reserved keyword)
  ^ dir + " -> " + place
⟧
```

Use `@game` for world mutation; keep string formatting pure.

## Permissions

`@game` is a host capability. Demo scripts often use `--allow-all`. Console-only play may work under defaults depending on install — if a call is denied, grant the game capability explicitly or use `--allow-all` for local play.

## Studio

Game host APIs that need full runtime may be limited in WASM. Prefer **CLI** for the full RPG sample; use Studio for pure helper functions and dialogue tables.

## Next

[Embedding](embedding.md) — run Rite inside a Rust host, or [Browser & Studio](browser.md) for the web UI.
