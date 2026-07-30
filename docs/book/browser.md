# Browser runtime and Studio

Rite’s public site and playground run as a static app plus a **WASM** build of the language core. Full host power still lives in the native CLI.

## Product site map

Hosted at [https://rite.undrc.dev](https://rite.undrc.dev):

| Path | Content |
|------|---------|
| `/` | Homepage |
| `/docs`, `/docs/:chapter` | This guided book |
| `/studio` | Browser playground |

Share snippets: **`/studio#s=…`** (hash is updated as you edit).

Source repo: [github.com/undercurrent-labs/rite](https://github.com/undercurrent-labs/rite)

## What works in WASM Studio

| Feature | Hosted Studio |
|---------|----------------|
| Parse / check / format / convert | Yes |
| Run pure scripts | Yes |
| `@console` printing | Yes |
| Pipelines, match, functions | Yes |
| Real `@fs` disk I/O | No (use CLI) |
| `@process` | No (blocked) |
| Real `@http.listen` sockets | Virtual / limited — use CLI |
| Full RPG + save files | Prefer CLI |

If Run shows a clear error about capabilities or “native host”, switch to local CLI.

## Try a pure example

In [Studio](/studio):

```rite
◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
```

Click **Run** → expect `144`.

## Local product site

```bash
pnpm install
pnpm site:dev          # http://127.0.0.1:5173
```

Build for deploy:

```bash
pnpm site:build        # scripts/build-site.sh → apps/rite-web/dist
pnpm site:deploy       # build + wrangler deploy
```

WASM package:

```bash
bash scripts/build-wasm.sh
# → apps/rite-studio/public/wasm and apps/rite-web/public/wasm
```

## Local Studio with full capabilities

Native host API (FS, HTTP listen, process when allowed):

```bash
# terminal 1
rite studio --port 4041 --no-open

# terminal 2 — optional UI
pnpm site:dev
# or studio-only:
pnpm --dir apps/rite-studio dev
```

Point the UI at the API if needed:

```bash
VITE_RITE_API=http://127.0.0.1:4041 pnpm site:dev
```

The SPA probes `/api/v1/version` for a **JSON** response. Cloudflare’s SPA fallback returns HTML for unknown paths — the client treats that as “no API” and uses WASM instead.

## Architecture (short)

```text
Browser
  ├─ Vue app (apps/rite-web)  → home, docs, shell
  ├─ Studio UI (apps/rite-studio sources)
  └─ rite_wasm.wasm           → parse/format/analyze/run (pure)
Native (optional)
  └─ rite studio              → Axum API + full evaluator + caps
```

## Next

You’re at the end of the guided book. Deeper material:

- Generated API: `rite docs build` → `docs/generated/`  
- Status: `IMPLEMENTATION.md`  
- Examples ladder: `examples/01-values` … `examples/10-embedded-rust`
