//! Host capabilities and permission system for Rite.

pub mod console;
pub mod clock;
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

use rite_runtime::{set_http_route_registrar, FunctionEntry, RuntimeContext};
use rite_sem::RouteIr;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

/// Install all default capabilities into a runtime context.
pub fn install_defaults(ctx: &mut RuntimeContext, perms: PermissionSet) -> Arc<HostCapabilities> {
    // Wire HTTP route registration once
    set_http_route_registrar(register_http_routes);
    ctx.allow_all = perms.allow_all;
    let host = Arc::new(HostCapabilities::with_defaults(perms.clone()));
    ctx.capabilities = host.clone();
    host
}

fn register_http_routes(addr: String, routes: &[RouteIr], ctx: &RuntimeContext) {
    let rite_routes: Vec<http::RiteRoute> = routes
        .iter()
        .map(|r| http::RiteRoute {
            method: r.method.clone(),
            path: r.path.clone(),
            param_name: Some("req".into()),
            body: r.body.clone(),
        })
        .collect();
    let perms = if ctx.allow_all {
        PermissionSet::allow_all()
    } else {
        PermissionSet::default_secure()
    };
    let state = http::ServerState {
        routes: rite_routes,
        functions: ctx.functions.clone(),
        perms,
        lock: Arc::new(AsyncMutex::new(())),
    };
    let _ = FunctionEntry {
        params: vec![],
        body: rite_sem::BlockIr {
            params: vec![],
            body: vec![],
            span: rite_core::Span::DUMMY,
        },
    };
    http::set_pending_server(addr, state);
}
