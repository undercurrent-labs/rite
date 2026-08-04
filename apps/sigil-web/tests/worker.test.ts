/**
 * The Worker.
 *
 * Two kinds of assertion. The first is behavioural — the endpoints answer, the
 * headers are set, a deep route falls through to the SPA. The second reads this
 * repository's own source, because the most important property of the Worker is
 * something it *does not* have: an endpoint that accepts a program. A behavioural
 * test cannot prove absence; reading the file can.
 */
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

vi.stubGlobal("__SIGIL_VERSION__", "v0.1.0");
vi.stubGlobal("__SIGIL_BUILD__", {
  commit: "abc1234",
  renderer: "v0.1.0",
  schemas: { "cant.graph": ["1"], "rite.sigil.graph": [1], "rite.sigil.scene": [1] },
});

const worker = (await import("../src/worker/index")).default;

const env = {
  ASSETS: {
    fetch: async (request: Request) =>
      new Response(`<!doctype html><title>asset for ${new URL(request.url).pathname}</title>`, {
        headers: { "content-type": "text/html" },
      }),
  },
};

const get = (path: string, method = "GET") =>
  worker.fetch(new Request(`https://sigil.rite.foo${path}`, { method }), env);

describe("api", () => {
  it("reports health", async () => {
    const response = await get("/api/health");
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ status: "ok" });
  });

  it("reports versions that come from the crates", async () => {
    const body = await (await get("/api/version")).json();
    expect(body).toMatchObject({ app: "v0.1.0", renderer: "v0.1.0", commit: "abc1234" });
  });

  it("reports the schemas it reads", async () => {
    const body = await (await get("/api/schema")).json();
    expect(body).toHaveProperty("cant.graph");
    expect(body).toHaveProperty("rite.sigil.graph");
    expect(body).toHaveProperty("rite.sigil.scene");
  });

  /** Read-only. A body would imply something is sent here, and nothing is. */
  it("refuses anything that is not a read", async () => {
    for (const method of ["POST", "PUT", "PATCH", "DELETE"]) {
      const response = await get("/api/health", method);
      expect(response.status, method).toBe(405);
    }
  });
});

describe("headers", () => {
  it("sets the security headers on assets and on the api", async () => {
    for (const path of ["/", "/api/health"]) {
      const headers = (await get(path)).headers;
      expect(headers.get("x-content-type-options"), path).toBe("nosniff");
      expect(headers.get("referrer-policy"), path).toBe("no-referrer");
      expect(headers.get("permissions-policy"), path).toContain("camera=()");
      expect(headers.get("content-security-policy"), path).toBeTruthy();
    }
  });

  /**
   * `wasm-unsafe-eval` is required — instantiating WebAssembly counts as
   * evaluation to CSP, and without it the renderer does not start. Plain
   * `unsafe-eval` is not required and must not be granted.
   */
  it("grants wasm evaluation without granting eval", async () => {
    const csp = (await get("/")).headers.get("content-security-policy") ?? "";
    expect(csp).toContain("'wasm-unsafe-eval'");
    expect(csp).not.toMatch(/(?<!wasm-)'unsafe-eval'/);
    expect(csp).toContain("frame-ancestors 'none'");
    expect(csp).toContain("object-src 'none'");
  });

  it("caches hashed assets immutably and html not at all", async () => {
    const hashed = await get("/assets/index.a1b2c3d4.js");
    expect(hashed.headers.get("cache-control")).toContain("immutable");
    const html = await get("/");
    expect(html.headers.get("cache-control")).toContain("must-revalidate");
  });
});

describe("routing", () => {
  it("falls through to the SPA for an app route", async () => {
    const response = await get("/gallery/some/deep/route");
    expect(response.status).toBe(200);
    expect(await response.text()).toContain("<!doctype html>");
  });
});

/**
 * The claim that cannot be tested by calling it: there is no endpoint that
 * accepts a program. ADR 0007 makes the privacy guarantee architectural rather
 * than procedural — there is nothing to misconfigure because there is nothing
 * there — and this is what keeps that true as the file changes.
 */
describe("no server-side rendering", () => {
  // Resolved from the project root rather than from `import.meta.url`: vitest
  // transforms the module, so its URL is not a `file:` one.
  const source = readFileSync(resolve(process.cwd(), "src/worker/index.ts"), "utf8");

  it("never reads a request body", () => {
    for (const banned of ["request.text(", "request.json(", "request.formData(", "request.arrayBuffer(", "request.blob("]) {
      expect(source, banned).not.toContain(banned);
    }
  });

  it("declares no render endpoint", () => {
    expect(source).not.toMatch(/["'`]\/api\/render/);
    expect(source).not.toMatch(/["'`]\/api\/svg/);
  });

  it("does not import the renderer", () => {
    expect(source).not.toContain("sigil_wasm");
    expect(source).not.toContain("renderCant");
  });
});
