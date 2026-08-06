#[cfg(feature = "native")]
use crate::clock::ClockCap;
use crate::console::ConsoleCap;
use crate::crypto::CryptoCap;
use crate::csv::CsvCap;
#[cfg(feature = "native")]
use crate::db::DbCap;
#[cfg(feature = "native")]
use crate::env::EnvCap;
#[cfg(feature = "native")]
use crate::fs::FsCap;
use crate::game::GameCap;
#[cfg(feature = "native")]
use crate::http::HttpCap;
use crate::json::JsonCap;
#[cfg(feature = "native")]
use crate::mcp::McpCap;
use crate::permissions::PermissionSet;
#[cfg(feature = "native")]
use crate::process::ProcessCap;
use crate::random::RandomCap;
use crate::regex::RegexCap;
#[cfg(feature = "native")]
use crate::stdin::StdinCap;
use crate::store::StoreCap;
#[cfg(feature = "native")]
use crate::sys::SysCap;
#[cfg(feature = "native")]
use crate::tcp::TcpCap;
#[cfg(feature = "native")]
use crate::udp::UdpCap;
use async_trait::async_trait;
use parking_lot::RwLock;
use rite_runtime::{CapabilityHost, EvalError, RuntimeContext, Value};
use std::sync::Arc;

/// Descriptor metadata for documentation and validation.
#[derive(Debug, Clone)]
pub struct NativeFunctionDescriptor {
    pub name: &'static str,
    pub docs: &'static str,
    pub arity: usize,
    pub effectful: bool,
    pub permission: &'static str,
}

pub trait Capability: Send + Sync {
    fn name(&self) -> &'static str;
    fn functions(&self) -> &'static [NativeFunctionDescriptor];
}

pub struct CapabilityRegistry {
    pub perms: PermissionSet,
}

/// The names this crate implements only with the `native` feature.
///
/// Kept so a build without it can say *why* `@fs` is missing instead of
/// reporting it as a capability nobody has heard of.
#[cfg(not(feature = "native"))]
const NATIVE_ONLY: &[&str] = &[
    "fs", "clock", "stdin", "env", "sys", "process", "http", "mcp", "udp", "tcp", "db",
];

pub struct HostCapabilities {
    pub perms: PermissionSet,
    pub console: ConsoleCap,
    #[cfg(feature = "native")]
    pub fs: FsCap,
    pub json: JsonCap,
    pub csv: CsvCap,
    pub crypto: CryptoCap,
    #[cfg(feature = "native")]
    pub clock: ClockCap,
    #[cfg(feature = "native")]
    pub stdin: StdinCap,
    pub regex: RegexCap,
    #[cfg(feature = "native")]
    pub env: EnvCap,
    #[cfg(feature = "native")]
    pub sys: SysCap,
    #[cfg(feature = "native")]
    pub process: ProcessCap,
    pub random: Arc<RwLock<RandomCap>>,
    #[cfg(feature = "native")]
    pub http: HttpCap,
    #[cfg(feature = "native")]
    pub mcp: McpCap,
    #[cfg(feature = "native")]
    pub udp: UdpCap,
    #[cfg(feature = "native")]
    pub tcp: TcpCap,
    pub game: Arc<RwLock<GameCap>>,
    pub store: Arc<RwLock<StoreCap>>,
    #[cfg(feature = "native")]
    pub db: Arc<RwLock<DbCap>>,
}

impl HostCapabilities {
    pub fn with_defaults(perms: PermissionSet) -> Self {
        Self {
            console: ConsoleCap,
            #[cfg(feature = "native")]
            fs: FsCap,
            json: JsonCap,
            csv: CsvCap,
            crypto: CryptoCap,
            #[cfg(feature = "native")]
            clock: ClockCap::new(),
            #[cfg(feature = "native")]
            stdin: StdinCap::new(),
            regex: RegexCap::new(),
            #[cfg(feature = "native")]
            env: EnvCap::default(),
            #[cfg(feature = "native")]
            sys: SysCap,
            #[cfg(feature = "native")]
            process: ProcessCap,
            random: Arc::new(RwLock::new(RandomCap::from_entropy())),
            #[cfg(feature = "native")]
            http: HttpCap::new(),
            #[cfg(feature = "native")]
            mcp: McpCap::new(),
            #[cfg(feature = "native")]
            udp: UdpCap::new(),
            #[cfg(feature = "native")]
            tcp: TcpCap::new(),
            game: Arc::new(RwLock::new(GameCap::new())),
            store: Arc::new(RwLock::new(StoreCap::new())),
            #[cfg(feature = "native")]
            db: Arc::new(RwLock::new(DbCap::new())),
            perms,
        }
    }

    /// Every capability this build carries, in the order the reference lists them.
    ///
    /// Built by pushes rather than a literal so the `native` entries can be
    /// dropped without moving the rest: `docs/generated/capabilities.md` is
    /// tracked, and reordering it fails the generation guard in CI.
    #[allow(clippy::vec_init_then_push)]
    pub fn all_descriptors(&self) -> Vec<(&'static str, &'static [NativeFunctionDescriptor])> {
        let mut out: Vec<(&'static str, &'static [NativeFunctionDescriptor])> = Vec::new();
        out.push(("console", ConsoleCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("fs", FsCap::DESCRIPTORS));
        out.push(("json", JsonCap::DESCRIPTORS));
        out.push(("csv", CsvCap::DESCRIPTORS));
        out.push(("crypto", CryptoCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("clock", ClockCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("stdin", StdinCap::DESCRIPTORS));
        out.push(("regex", RegexCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("env", EnvCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("sys", SysCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("process", ProcessCap::DESCRIPTORS));
        out.push(("random", RandomCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("http", HttpCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("mcp", McpCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("udp", UdpCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("tcp", TcpCap::DESCRIPTORS));
        out.push(("game", GameCap::DESCRIPTORS));
        out.push(("store", StoreCap::DESCRIPTORS));
        #[cfg(feature = "native")]
        out.push(("db", DbCap::DESCRIPTORS));
        out
    }
}

fn resolve_atom_args(args: Vec<Value>, ctx: &RuntimeContext) -> Vec<Value> {
    args.into_iter()
        .map(|v| match v {
            Value::Atom(id) => Value::string(ctx.atoms.name(id)),
            Value::List(xs) => Value::List(
                xs.into_iter()
                    .map(|x| match x {
                        Value::Atom(id) => Value::string(ctx.atoms.name(id)),
                        other => other,
                    })
                    .collect(),
            ),
            Value::Record(mut r) => {
                for (_k, val) in r.iter_mut() {
                    if let Value::Atom(id) = val {
                        *val = Value::string(ctx.atoms.name(*id));
                    }
                }
                Value::Record(r)
            }
            other => other,
        })
        .collect()
}

#[async_trait]
impl CapabilityHost for HostCapabilities {
    async fn call(
        &self,
        path: &[String],
        args: Vec<Value>,
        effect: bool,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        let cap = path.first().map(|s| s.as_str()).unwrap_or("");
        let method = path.get(1).map(|s| s.as_str()).unwrap_or("");
        // `effect` is only read by the native arms; without them it is the
        // dispatcher's own unused parameter, not a caller's mistake.
        let _ = effect;
        match cap {
            "console" => self.console.call(method, args, &self.perms, ctx).await,
            #[cfg(feature = "native")]
            "fs" => self.fs.call(method, args, &self.perms, ctx).await,
            "json" => self.json.call(method, args, &self.perms, &ctx.atoms).await,
            "csv" => self.csv.call(method, args, &self.perms, &ctx.atoms).await,
            // Pure value transforms, apart from `random_bytes` — nothing to await.
            "crypto" => self.crypto.call(method, args, &self.perms),
            #[cfg(feature = "native")]
            "clock" => self.clock.call(method, args, effect, &self.perms).await,
            #[cfg(feature = "native")]
            "stdin" => self.stdin.call(method, args, effect, &self.perms).await,
            "regex" => self.regex.call(method, args, &self.perms),
            #[cfg(feature = "native")]
            "env" => self.env.call(method, args, &self.perms).await,
            #[cfg(feature = "native")]
            "sys" => self.sys.call(method, args, &self.perms).await,
            // The environment overlay travels with the spawn: a variable this
            // run set with `@env.set` is inherited by the command it starts.
            #[cfg(feature = "native")]
            "process" => {
                self.process
                    .call(method, args, &self.perms, ctx, &self.env.overlay())
                    .await
            }
            "random" => {
                let mut rng = self.random.write();
                rng.call(method, args, &self.perms)
            }
            #[cfg(feature = "native")]
            "http" => self.http.call(method, args, &self.perms, ctx).await,
            #[cfg(feature = "native")]
            "mcp" => self.mcp.call(method, args, &self.perms, ctx).await,
            #[cfg(feature = "native")]
            "udp" => self.udp.call(method, args, &self.perms).await,
            // `listen` needs the context: its handler block is a closure that must
            // resolve the module scope it was written in, exactly as `@http` does.
            #[cfg(feature = "native")]
            "tcp" => self.tcp.call(method, args, &self.perms, ctx).await,
            "game" => {
                let args = resolve_atom_args(args, ctx);
                let mut game = self.game.write();
                game.call(method, args, &ctx.atoms)
            }
            "store" => {
                let mut store = self.store.write();
                store.call(method, args, &ctx.atoms)
            }
            #[cfg(feature = "native")]
            "db" => {
                let db = self.db.read();
                db.call(method, args, &self.perms, ctx.budget.limits())
            }
            #[cfg(not(feature = "native"))]
            other if NATIVE_ONLY.contains(&other) => Err(EvalError::Capability(format!(
                "capability @{other} is native-only and unavailable in the browser runtime"
            ))),
            other => Err(EvalError::Capability(format!(
                "unknown capability `@{}`",
                other
            ))),
        }
    }
}
