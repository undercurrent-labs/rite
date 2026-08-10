use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rite_runtime::{EvalError, Value};

pub struct RandomCap {
    rng: StdRng,
}

impl RandomCap {
    /// A reproducible generator, as `@random.seed(n)` and a host-supplied seed produce.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// The default: seeded from the operating system, so two runs differ.
    ///
    /// This used to be `new(42)`, which meant every `rite run` on every machine drew
    /// the identical sequence forever — a dice roll was a constant. The descriptor for
    /// `seed` promises "deterministic sequences", which only means something if the
    /// default is not one, and the book's own example calls `@random.seed(1)` first for
    /// exactly that reason.
    pub fn from_entropy() -> Self {
        Self {
            rng: StdRng::from_entropy(),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "int",
            docs: "Random integer in [min, max].",
            arity: 2,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "float",
            docs: "Random float in [0, 1).",
            arity: 0,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "choose",
            docs: "Choose a random element from a list.",
            arity: 1,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "shuffle",
            docs: "Return a shuffled copy of a list.",
            arity: 1,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "uuid",
            docs: "Generate a UUID v4 string.",
            arity: 0,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "seed",
            docs: "Reseed the RNG for deterministic sequences.",
            arity: 1,
            effectful: true,
            permission: "random",
            returns_result: false,
        },
    ];

    pub fn call(
        &mut self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        perms.check_random().map_err(EvalError::Permission)?;
        match method {
            "int" => {
                // `@random.int("a", "b")` answered `0` — a number in range, from a
                // call that named no range at all.
                let min = crate::args::int_arg_or("random.int", &args, 0, 0)?;
                let max = crate::args::int_arg_or("random.int", &args, 1, min)?;
                if max < min {
                    return Err(EvalError::Message("random.int: max < min".into()));
                }
                Ok(Value::Int(self.rng.gen_range(min..=max)))
            }
            "float" => Ok(Value::Float(self.rng.gen::<f64>())),
            "choose" => {
                let Some(Value::List(xs)) = args.into_iter().next() else {
                    return Ok(Value::None);
                };
                if xs.is_empty() {
                    return Ok(Value::None);
                }
                let i = self.rng.gen_range(0..xs.len());
                Ok(xs[i].clone())
            }
            "shuffle" => {
                let Some(Value::List(xs)) = args.into_iter().next() else {
                    return Ok(Value::list(Vec::<Value>::new()));
                };
                let mut v: Vec<Value> = xs.into_iter().collect();
                for i in (1..v.len()).rev() {
                    let j = self.rng.gen_range(0..=i);
                    v.swap(i, j);
                }
                Ok(Value::list(v))
            }
            // Drawn from this generator, not from system entropy: `@random.seed(n)`
            // promises a reproducible run, and a UUID that ignored the seed made every
            // "deterministic" run non-deterministic in the one place it is most visible.
            "uuid" => {
                let mut bytes = [0u8; 16];
                self.rng.fill(&mut bytes);
                Ok(Value::string(
                    uuid::Builder::from_random_bytes(bytes)
                        .into_uuid()
                        .to_string(),
                ))
            }
            "seed" => {
                let seed = crate::args::int_arg_or("random.seed", &args, 0, 0)? as u64;
                self.rng = StdRng::seed_from_u64(seed);
                Ok(Value::None)
            }
            other => Err(EvalError::Capability(format!("unknown @random.{}", other))),
        }
    }
}
