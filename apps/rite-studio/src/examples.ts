export type Example = {
  id: string;
  title: string;
  description: string;
  source: string;
  ascii?: string;
  browser: boolean;
};

export const EXAMPLES: Example[] = [
  {
    id: "values",
    title: "01 Values",
    description: "Atoms, lists, records, truthiness",
    browser: true,
    source: `name ← "Aura"
xs ← [1, 2, 3]
rec ← ⟨ok: true, tags: [#demo]⟩
! @console.println(name)
! @console.println(xs → sum)
`,
  },
  {
    id: "pipelines",
    title: "02 Pipelines",
    description: "map / keep / sum",
    browser: true,
    source: `[1, 2, 3, 4, 5]
  → keep { |n| n > 2 }
  → map { |n| n * n }
  → sum
`,
  },
  {
    id: "http",
    title: "07 HTTP",
    description: "Sinatra-style routes (virtual in browser)",
    browser: true,
    source: `@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
`,
  },
  {
    id: "rpg",
    title: "09 Text RPG",
    description: "Game capability demo",
    browser: true,
    source: `! @game.register_room(#atrium, ⟨text: "Neon rain on glass.", exits: ⟨east: "vault"⟩⟩)
! @game.register_room(#vault, ⟨text: "A sealed gate hums.", exits: ⟨west: "atrium"⟩⟩)
! @game.start(#atrium)
! @console.println(@game.look())
`,
  },
];
