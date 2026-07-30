export type Example = {
  id: string;
  title: string;
  description: string;
  source: string;
  /** ASCII transliteration, loaded when the dialect selector is set to `ascii`. */
  ascii: string;
};

export const EXAMPLES: Example[] = [
  {
    id: "values",
    title: "Values",
    description: "Atoms, lists, records, truthiness",
    source: `name ← "Aura"
xs ← [1, 2, 3]
rec ← ⟨ok: true, tags: [#demo]⟩
! @console.println(name)
! @console.println(xs → sum)
`,
    ascii: `name <- "Aura"
xs <- [1, 2, 3]
rec <- <<ok: true, tags: [:demo]>>
do host.console.println(name)
do host.console.println(xs -> sum)
`,
  },
  {
    id: "pipelines",
    title: "Pipelines",
    description: "map / keep / sum",
    source: `[1, 2, 3, 4, 5]
  → keep { |n| n > 2 }
  → map { |n| n * n }
  → sum
`,
    ascii: `[1, 2, 3, 4, 5]
  -> keep { |n| n > 2 }
  -> map { |n| n * n }
  -> sum
`,
  },
  {
    id: "matching",
    title: "Pattern matching",
    description: "Match on atoms with a fallback arm",
    source: `status ← #ok
msg ← ~ status ⟦
  #ok → "ready"
  #error → "failed"
  _ → "unknown"
⟧
! @console.println(msg)
`,
    ascii: `status <- :ok
msg <- match status [[
  :ok -> "ready"
  :error -> "failed"
  _ -> "unknown"
]]
do host.console.println(msg)
`,
  },
  {
    id: "http",
    title: "HTTP routes",
    description: "Route table — listed here, served by the CLI",
    source: `@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
`,
    ascii: `host.http.listen "127.0.0.1:0" [[
  GET "/health" [[
    return 200 <<status: :ok>>
  ]]
]]
`,
  },
  {
    id: "rpg",
    title: "Text RPG",
    description: "Game capability demo",
    source: `! @game.register_room(#atrium, ⟨text: "Neon rain on glass.", exits: ⟨east: "vault"⟩⟩)
! @game.register_room(#vault, ⟨text: "A sealed gate hums.", exits: ⟨west: "atrium"⟩⟩)
! @game.start(#atrium)
! @console.println(@game.look())
`,
    ascii: `do host.game.register_room(:atrium, <<text: "Neon rain on glass.", exits: <<east: "vault">>>>)
do host.game.register_room(:vault, <<text: "A sealed gate hums.", exits: <<west: "atrium">>>>)
do host.game.start(:atrium)
do host.console.println(host.game.look())
`,
  },
];
