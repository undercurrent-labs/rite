# Deploying Sigil

```bash
pnpm sigil:build     # WASM, typecheck, component tests, Vue build, wrangler dry run
pnpm sigil:deploy    # the above, then `wrangler deploy`
```

Worker `rite-sigil`, serving `sigil.rite.foo` as a **Custom Domain** — the Worker
is the origin, not something behind a proxy route. One hop, and nothing in front
of it that could observe a request.

## What the Worker does

Almost nothing, deliberately.

| Route | Response |
|---|---|
| `GET /api/health` | `{"status":"ok"}`, uncached |
| `GET /api/version` | app version, renderer version, build commit |
| `GET /api/schema` | the graph and scene schema versions this build reads |
| everything else | static assets, with the SPA as the 404 fallback |

Non-`GET` on `/api/*` is `405`. A request body would imply something is sent
here, and nothing is.

**There is no render endpoint, and there must never be one.** Sigil renders in
the browser ([ADR 0005](../adr/0005-one-renderer-in-rust.md)) and the privacy
claim in [ADR 0007](../adr/0007-veil-and-source-privacy.md) is architectural
rather than procedural: there is nothing to misconfigure because there is nothing
there. `tests/worker.test.ts` reads the Worker's own source and fails if it grows
a body read, a `/api/render` route, or an import of the renderer — a behavioural
test cannot prove absence, so that one reads the file.

## Headers

Set on every response, assets and API alike.

```
Content-Security-Policy   default-src 'self'; script-src 'self' 'wasm-unsafe-eval';
                          style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:;
                          connect-src 'self'; object-src 'none'; base-uri 'none';
                          form-action 'none'; frame-ancestors 'none'
X-Content-Type-Options    nosniff
Referrer-Policy           no-referrer
Permissions-Policy        camera=(), microphone=(), geolocation=(), … 
X-Frame-Options           DENY
```

`wasm-unsafe-eval` is required and `unsafe-eval` is not. Instantiating
WebAssembly counts as evaluation to CSP, so without the first the renderer does
not start; the second is a strictly larger grant and is not made. A test asserts
both, including that `unsafe-eval` does not appear other than as part of
`wasm-unsafe-eval`.

`style-src 'unsafe-inline'` is the one concession: Vite inlines a small amount of
CSS. Removing it means hashing or nonce-ing the inline styles, which the Cloudflare
asset pipeline does not do for us. Worth revisiting.

## Caching

Hashed assets — `index.a1b2c3d4.js`, the `.wasm` — are `immutable` for a year.
HTML and the version manifest are `must-revalidate`, or a deploy would not be
visible until a cache expired.

## Before deploying

`scripts/build-sigil-site.sh` runs the typecheck, the component tests, the build,
and a `wrangler deploy --dry-run`, and then asserts the engine is in `dist/wasm/`.
The app loads it by URL at runtime, so a build that dropped it is a blank canvas
in production rather than a failure at build time.

CI runs the same job on every push (`sigil-site` in `.github/workflows/ci.yml`).
It has no `needs:` — it builds its own WASM and mirrors nothing — for the reason
the Cant site job is independent: Sigil must be removable without touching
anything else.

## Rollback

`wrangler rollback` on the Worker. The artifact is static, so a rollback is a
redeploy of the previous bundle; nothing has migrated and nothing is stored.

## The domain

`sigil.rite.foo` is declared in `site.toml` alongside `rite.foo` and
`cant.rite.foo`, and `crates/rite-cli/tests/site_domain_sync.rs` fails if any
tracked file names a host that file does not declare. Attaching the Custom Domain
needs the zone to exist in Cloudflare first; `wrangler deploy` fails with a clear
message if it does not.
