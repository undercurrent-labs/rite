//! Host capabilities and permission system for Rite.

pub mod args;
pub mod clock;
pub mod console;
pub mod crypto;
pub mod csv;
pub mod db;
pub mod env;
pub mod fs;
pub mod game;
pub mod http;
pub mod json;
pub mod mcp;
pub mod permissions;
pub mod process;
pub mod random;
pub mod regex;
pub mod registry;
pub mod stdin;
pub mod store;
pub mod sys;
pub mod tcp;
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
pub fn request_stop() {
    http::request_stop();
}

use rite_runtime::RuntimeContext;
use std::sync::Arc;

/// Install all default capabilities into a runtime context.
pub fn install_defaults(ctx: &mut RuntimeContext, perms: PermissionSet) -> Arc<HostCapabilities> {
    install_defaults_with_env(ctx, perms, Vec::new())
}

/// [`install_defaults`], seeding the environment overlay from a `--env-file`.
///
/// The values land where `@env.set` writes (see `env.rs`), so `@env.get` reads
/// them back and `@process.run` passes them on, without the process's own
/// environment being touched.
pub fn install_defaults_with_env(
    ctx: &mut RuntimeContext,
    perms: PermissionSet,
    env_file_values: Vec<(String, String)>,
) -> Arc<HostCapabilities> {
    ctx.allow_all = perms.allow_all;
    // Console bypasses the capability host (it needs `&mut ctx`), so mirror the grant
    // onto the context where the evaluator can see it.
    ctx.console_allowed = perms.allow_all || perms.console;
    let host = Arc::new(HostCapabilities::with_defaults(perms.clone()));
    host.env.seed(env_file_values);
    // Honour a host-pinned seed. Studio and the doctest runner want reproducible runs;
    // everything else gets OS entropy from `with_defaults`.
    if let Some(seed) = ctx.rng_seed {
        *host.random.write() = crate::random::RandomCap::new(seed);
    }
    ctx.capabilities = host.clone();
    host
}
