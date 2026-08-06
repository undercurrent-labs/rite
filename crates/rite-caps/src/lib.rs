//! Host capabilities and permission system for Rite.
//!
//! The `native` feature (on by default) carries the half of the surface that
//! needs a host: filesystem, clock, environment, subprocesses, sockets, HTTP,
//! MCP and the database. Without it the crate builds for wasm32, and what
//! remains is the half that needs nothing a browser lacks — `@json`, `@csv`,
//! `@crypto`, `@regex`, `@store`, plus `@random` and `@game`, which own their
//! state in the process. Same implementations and same descriptors either way;
//! the feature decides which are compiled, not how they behave.

pub mod args;
#[cfg(feature = "native")]
pub mod clock;
pub mod console;
pub mod crypto;
pub mod csv;
#[cfg(feature = "native")]
pub mod db;
#[cfg(feature = "native")]
pub mod env;
#[cfg(feature = "native")]
pub mod fs;
pub mod game;
#[cfg(feature = "native")]
pub mod http;
pub mod json;
#[cfg(feature = "native")]
pub mod mcp;
pub mod permissions;
#[cfg(feature = "native")]
pub mod process;
pub mod proto;
pub mod random;
pub mod regex;
pub mod registry;
#[cfg(feature = "native")]
pub mod stdin;
pub mod store;
#[cfg(feature = "native")]
pub mod sys;
#[cfg(feature = "native")]
pub mod tcp;
#[cfg(feature = "native")]
pub mod udp;

pub use permissions::{Permission, PermissionSet};
pub use registry::{CapabilityRegistry, HostCapabilities};

/// Stop every running server, the way Ctrl-C does.
///
/// `@http.listen` installs its own Ctrl-C handler, and only one handler in a
/// process can win. A long-lived host such as a REPL that installs one first has to
/// be able to do what that handler would have done, or a server started from
/// the prompt would keep the terminal after the line that started it was
/// interrupted.
#[cfg(feature = "native")]
pub fn request_stop() {
    http::request_stop();
}

use rite_runtime::RuntimeContext;
use std::sync::Arc;

/// Install all default capabilities into a runtime context.
///
/// Without the `native` feature this installs the browser-reachable subset; the
/// rest answer with a capability error naming themselves, which is what
/// `rite-wasm` wants and what `NopCapabilities` could not give.
pub fn install_defaults(ctx: &mut RuntimeContext, perms: PermissionSet) -> Arc<HostCapabilities> {
    ctx.allow_all = perms.allow_all;
    // Console bypasses the capability host (it needs `&mut ctx`), so mirror the grant
    // onto the context where the evaluator can see it.
    ctx.console_allowed = perms.allow_all || perms.console;
    let host = Arc::new(HostCapabilities::with_defaults(perms));
    // Honour a host-pinned seed. Studio and the doctest runner want reproducible runs;
    // everything else gets OS entropy from `with_defaults`.
    if let Some(seed) = ctx.rng_seed {
        *host.random.write() = crate::random::RandomCap::new(seed);
    }
    ctx.capabilities = host.clone();
    host
}

/// [`install_defaults`], seeding the environment overlay from a `--env-file`.
///
/// The values land where `@env.set` writes (see `env.rs`), so `@env.get` reads
/// them back and `@process.run` passes them on, without the process's own
/// environment being touched.
#[cfg(feature = "native")]
pub fn install_defaults_with_env(
    ctx: &mut RuntimeContext,
    perms: PermissionSet,
    env_file_values: Vec<(String, String)>,
) -> Arc<HostCapabilities> {
    let host = install_defaults(ctx, perms);
    host.env.seed(env_file_values);
    host
}
