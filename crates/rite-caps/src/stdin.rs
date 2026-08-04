use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use parking_lot::RwLock;
use rite_runtime::{EvalError, Value};
use std::io::Read;

/// The process's own standard input.
///
/// This is what makes a script a shell citizen: `cat access.log | rite run
/// filter.rite`, with the data arriving on the pipe rather than through a
/// filename. Reading stdin is an effect (it observes state
/// outside the program) and carries its own permission, allowed by default
/// like `console` and revocable with `--deny stdin`.
///
/// # One read, cached
///
/// Standard input can only be consumed once, so the first call drains it to
/// EOF and every later call answers from the cache. That makes
/// `@stdin.read` followed by `@stdin.lines` see the same bytes, and repeated
/// calls deterministic — the alternative, a second read silently answering
/// `""`, is a bug that looks like an empty pipe.
///
/// On a terminal (no pipe), the read blocks until end-of-input (Ctrl-D) —
/// the same contract `cat` has.
pub struct StdinCap {
    /// The drained input. `None` until the first read.
    cached: RwLock<Option<String>>,
    /// Optional fake input for tests, set instead of ever touching the real
    /// descriptor.
    fake: RwLock<Option<String>>,
}

impl StdinCap {
    pub fn new() -> Self {
        Self {
            cached: RwLock::new(None),
            fake: RwLock::new(None),
        }
    }

    pub fn set_fake_input(&self, input: &str) {
        *self.fake.write() = Some(input.to_string());
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "read",
            docs: "The whole of standard input as one string, read to EOF on first use and cached. Empty when nothing was piped in.",
            arity: 0,
            effectful: true,
            permission: "stdin",
        },
        NativeFunctionDescriptor {
            name: "lines",
            docs: "Standard input as a list of lines, without their terminators. An empty input is an empty list, so a pipeline over `@stdin.lines` runs zero times rather than once over `\"\"`.",
            arity: 0,
            effectful: true,
            permission: "stdin",
        },
    ];

    fn input(&self) -> Result<String, EvalError> {
        if let Some(fake) = self.fake.read().as_ref() {
            return Ok(fake.clone());
        }
        if let Some(cached) = self.cached.read().as_ref() {
            return Ok(cached.clone());
        }
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| EvalError::Message(format!("stdin: {e}")))?;
        *self.cached.write() = Some(buffer.clone());
        Ok(buffer)
    }

    pub async fn call(
        &self,
        method: &str,
        _args: Vec<Value>,
        _effect: bool,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        perms.check_stdin().map_err(EvalError::Permission)?;
        match method {
            "read" => Ok(Value::string(self.input()?)),
            "lines" => {
                let input = self.input()?;
                // `lines()` and not `split('\n')`: an input ending in a newline
                // is N lines, not N lines and an empty one.
                Ok(Value::list(input.lines().map(Value::string)))
            }
            other => Err(EvalError::Capability(format!("unknown @stdin.{}", other))),
        }
    }
}

impl Default for StdinCap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Permission, PermissionSet};

    #[tokio::test]
    async fn lines_split_without_a_phantom_trailing_line() {
        let cap = StdinCap::new();
        cap.set_fake_input("alpha\nbeta\n");
        let perms = PermissionSet::default_secure();
        let value = cap
            .call("lines", vec![], true, &perms)
            .await
            .expect("lines");
        assert_eq!(format!("{value}"), "[alpha, beta]");
    }

    #[tokio::test]
    async fn empty_input_is_an_empty_list_and_an_empty_string() {
        let cap = StdinCap::new();
        cap.set_fake_input("");
        let perms = PermissionSet::default_secure();
        let lines = cap
            .call("lines", vec![], true, &perms)
            .await
            .expect("lines");
        assert_eq!(format!("{lines}"), "[]");
        let read = cap.call("read", vec![], true, &perms).await.expect("read");
        assert_eq!(format!("{read}"), "");
    }

    /// Allowed by default, and revocable — both directions, so the gate is a gate.
    #[tokio::test]
    async fn denied_stdin_is_a_permission_error() {
        let cap = StdinCap::new();
        cap.set_fake_input("secret");
        let mut perms = PermissionSet::default_secure();
        perms.deny(Permission::Stdin);
        let result = cap.call("read", vec![], true, &perms).await;
        assert!(matches!(result, Err(EvalError::Permission(_))));
    }
}
