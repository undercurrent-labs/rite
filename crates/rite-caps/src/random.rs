use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rite_runtime::{EvalError, Value};

pub struct RandomCap {
    rng: StdRng,
}

impl RandomCap {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "int",
            docs: "Random integer in [min, max].",
            arity: 2,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "float",
            docs: "Random float in [0, 1).",
            arity: 0,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "choose",
            docs: "Choose a random element from a list.",
            arity: 1,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "shuffle",
            docs: "Return a shuffled copy of a list.",
            arity: 1,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "uuid",
            docs: "Generate a UUID v4 string.",
            arity: 0,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "seed",
            docs: "Reseed the RNG for deterministic sequences.",
            arity: 1,
            effectful: true,
            permission: "random",
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
                let min = args.first().and_then(|v| v.as_int()).unwrap_or(0);
                let max = args.get(1).and_then(|v| v.as_int()).unwrap_or(min);
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
            "uuid" => Ok(Value::string(uuid::Uuid::new_v4().to_string())),
            "seed" => {
                let seed = args.first().and_then(|v| v.as_int()).unwrap_or(0) as u64;
                self.rng = StdRng::seed_from_u64(seed);
                Ok(Value::None)
            }
            other => Err(EvalError::Capability(format!(
                "unknown @random.{}",
                other
            ))),
        }
    }
}
