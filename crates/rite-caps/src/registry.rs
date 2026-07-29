use crate::clock::ClockCap;
use crate::console::ConsoleCap;
use crate::csv::CsvCap;
use crate::db::DbCap;
use crate::env::EnvCap;
use crate::fs::FsCap;
use crate::game::GameCap;
use crate::http::HttpCap;
use crate::json::JsonCap;
use crate::permissions::PermissionSet;
use crate::process::ProcessCap;
use crate::random::RandomCap;
use crate::store::StoreCap;
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

pub struct HostCapabilities {
    pub perms: PermissionSet,
    pub console: ConsoleCap,
    pub fs: FsCap,
    pub json: JsonCap,
    pub csv: CsvCap,
    pub clock: ClockCap,
    pub env: EnvCap,
    pub process: ProcessCap,
    pub random: Arc<RwLock<RandomCap>>,
    pub http: HttpCap,
    pub game: Arc<RwLock<GameCap>>,
    pub store: Arc<RwLock<StoreCap>>,
    pub db: Arc<RwLock<DbCap>>,
}

impl HostCapabilities {
    pub fn with_defaults(perms: PermissionSet) -> Self {
        Self {
            console: ConsoleCap,
            fs: FsCap,
            json: JsonCap,
            csv: CsvCap,
            clock: ClockCap::new(),
            env: EnvCap,
            process: ProcessCap,
            random: Arc::new(RwLock::new(RandomCap::new(42))),
            http: HttpCap::new(),
            game: Arc::new(RwLock::new(GameCap::new())),
            store: Arc::new(RwLock::new(StoreCap::new())),
            db: Arc::new(RwLock::new(DbCap::new())),
            perms,
        }
    }

    pub fn all_descriptors(&self) -> Vec<(&'static str, &'static [NativeFunctionDescriptor])> {
        vec![
            ("console", ConsoleCap::DESCRIPTORS),
            ("fs", FsCap::DESCRIPTORS),
            ("json", JsonCap::DESCRIPTORS),
            ("csv", CsvCap::DESCRIPTORS),
            ("clock", ClockCap::DESCRIPTORS),
            ("env", EnvCap::DESCRIPTORS),
            ("process", ProcessCap::DESCRIPTORS),
            ("random", RandomCap::DESCRIPTORS),
            ("http", HttpCap::DESCRIPTORS),
            ("game", GameCap::DESCRIPTORS),
            ("store", StoreCap::DESCRIPTORS),
            ("db", DbCap::DESCRIPTORS),
        ]
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
        match cap {
            "console" => self.console.call(method, args, &self.perms, ctx).await,
            "fs" => self.fs.call(method, args, &self.perms).await,
            "json" => self.json.call(method, args, &self.perms).await,
            "csv" => self.csv.call(method, args, &self.perms).await,
            "clock" => self.clock.call(method, args, effect, &self.perms).await,
            "env" => self.env.call(method, args, &self.perms).await,
            "process" => self.process.call(method, args, &self.perms).await,
            "random" => {
                let mut rng = self.random.write();
                rng.call(method, args, &self.perms)
            }
            "http" => self.http.call(method, args, &self.perms, ctx).await,
            "game" => {
                let args = resolve_atom_args(args, ctx);
                let mut game = self.game.write();
                game.call(method, args)
            }
            "store" => {
                let mut store = self.store.write();
                store.call(method, args)
            }
            "db" => {
                let db = self.db.read();
                db.call(method, args, &self.perms)
            }
            other => Err(EvalError::Capability(format!(
                "unknown capability `@{}`",
                other
            ))),
        }
    }
}
