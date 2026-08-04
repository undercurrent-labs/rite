/**
 * The family's addresses, injected from `site.toml` at build time.
 *
 * Read rather than typed, so a host cannot drift from the canonical list —
 * `crates/rite-cli/tests/site_domain_sync.rs` is the gate this satisfies. The
 * defines were already in vite.config.ts; this is the one place they surface.
 */
export const RITE_URL = `https://${__RITE_HOST__}`;
export const CANT_URL = `https://${__CANT_HOST__}`;
export const SIGIL_VERSION = __SIGIL_VERSION__;
export const GITHUB_URL = "https://github.com/undercurrent-labs/rite/tree/main/docs/sigil";
