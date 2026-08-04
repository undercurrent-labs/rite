/**
 * The Worker.
 *
 * Deliberately almost nothing. It answers three read-only questions and adds
 * security headers; everything else is a static asset.
 *
 * **There is no render endpoint and there must never be one.** Sigil renders in
 * the browser (ADR 0005), and the privacy claim in ADR 0007 is architectural
 * rather than procedural: there is no endpoint to misconfigure because there is
 * no endpoint. A `POST /api/render` would quietly undo the product's central
 * promise, which is why `no_server_render_endpoint_exists` asserts on this file.
 */

interface Env {
  ASSETS: { fetch: (request: Request) => Promise<Response> };
}

/** Injected at build time from the crate versions. See `vite.config.ts`. */
declare const __SIGIL_VERSION__: string;
declare const __SIGIL_BUILD__: { commit: string; renderer: string; schemas: Record<string, unknown> };

/**
 * Headers on every response.
 *
 * The CSP is the interesting one. `wasm-unsafe-eval` is required — instantiating
 * WebAssembly counts as evaluation to CSP, and without it the renderer will not
 * start — but plain `unsafe-eval` is not, and is not granted. `connect-src
 * 'none'` is the privacy claim as a header: the page cannot make a network
 * request even if some future code tried to.
 */
const SECURITY_HEADERS: Record<string, string> = {
  "Content-Security-Policy": [
    "default-src 'self'",
    // Vite inlines a small amount of CSS and the app's styles; scripts are
    // bundled files served from this origin.
    "script-src 'self' 'wasm-unsafe-eval'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self'",
    // Nothing is fetched, uploaded, or reported. The renderer is local.
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join("; "),
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  // No capability this app uses, denied to everything including itself.
  "Permissions-Policy":
    "camera=(), microphone=(), geolocation=(), interest-cohort=(), payment=(), usb=()",
  "X-Frame-Options": "DENY",
};

function json(body: unknown, maxAge: number): Response {
  return new Response(JSON.stringify(body, null, 2), {
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": `public, max-age=${maxAge}`,
      ...SECURITY_HEADERS,
    },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname.startsWith("/api/")) {
      // Read-only. A body would imply something is sent here, and nothing is.
      if (request.method !== "GET" && request.method !== "HEAD") {
        return new Response("method not allowed", {
          status: 405,
          headers: { allow: "GET, HEAD", ...SECURITY_HEADERS },
        });
      }

      switch (url.pathname) {
        case "/api/health":
          return json({ status: "ok" }, 0);
        case "/api/version":
          return json(
            {
              app: __SIGIL_VERSION__,
              renderer: __SIGIL_BUILD__.renderer,
              commit: __SIGIL_BUILD__.commit,
            },
            60
          );
        case "/api/schema":
          return json(__SIGIL_BUILD__.schemas, 60);
        default:
          return json({ error: "not found" }, 0);
      }
    }

    const response = await env.ASSETS.fetch(request);
    const headers = new Headers(response.headers);
    for (const [key, value] of Object.entries(SECURITY_HEADERS)) {
      headers.set(key, value);
    }

    // Hashed assets are immutable; HTML is not, or a deploy would not be visible
    // until a cache expired.
    if (/\.[0-9a-f]{8,}\.(js|css|wasm|woff2?)$/.test(url.pathname)) {
      headers.set("cache-control", "public, max-age=31536000, immutable");
    } else if (url.pathname === "/" || url.pathname.endsWith(".html")) {
      headers.set("cache-control", "public, max-age=0, must-revalidate");
    }

    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers,
    });
  },
};
