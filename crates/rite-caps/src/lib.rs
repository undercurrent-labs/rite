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
pub mod registry;
pub mod store;
pub mod tcp;
pub mod udp;

pub use permissions::{Permission, PermissionSet};
pub use registry::{CapabilityRegistry, HostCapabilities};

use rite_runtime::RuntimeContext;
use std::sync::Arc;

/// Install all default capabilities into a runtime context.
pub fn install_defaults(ctx: &mut RuntimeContext, perms: PermissionSet) -> Arc<HostCapabilities> {
    ctx.allow_all = perms.allow_all;
    // Console bypasses the capability host (it needs `&mut ctx`), so mirror the grant
    // onto the context where the evaluator can see it.
    ctx.console_allowed = perms.allow_all || perms.console;
    let host = Arc::new(HostCapabilities::with_defaults(perms.clone()));
    // Honour a host-pinned seed. Studio and the doctest runner want reproducible runs;
    // everything else gets OS entropy from `with_defaults`.
    if let Some(seed) = ctx.rng_seed {
        *host.random.write() = crate::random::RandomCap::new(seed);
    }
    ctx.capabilities = host.clone();
    host
}
