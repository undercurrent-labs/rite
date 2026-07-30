//! Host capabilities and permission system for Rite.

pub mod clock;
pub mod console;
pub mod csv;
pub mod db;
pub mod env;
pub mod fs;
pub mod game;
pub mod http;
pub mod json;
pub mod permissions;
pub mod process;
pub mod random;
pub mod registry;
pub mod store;

pub use permissions::{Permission, PermissionSet};
pub use registry::{CapabilityRegistry, HostCapabilities};

use rite_runtime::RuntimeContext;
use std::sync::Arc;

/// Install all default capabilities into a runtime context.
pub fn install_defaults(ctx: &mut RuntimeContext, perms: PermissionSet) -> Arc<HostCapabilities> {
    ctx.allow_all = perms.allow_all;
    let host = Arc::new(HostCapabilities::with_defaults(perms.clone()));
    ctx.capabilities = host.clone();
    host
}
