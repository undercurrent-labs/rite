use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// `std::time::Instant` panics on wasm32-unknown-unknown. Wall-clock timeouts
// are only enforced on native; wasm relies on step/depth budgets.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// How deep Rite calls may nest before the budget stops them.
///
/// This is a *native stack* limit in disguise. The evaluator is an async tree-walker, so
/// one Rite call costs several nested future frames, and a debug build's frames are far
/// larger than a release build's — measured at roughly 64 KB per Rite call level in debug
/// versus a small fraction of that optimised. Rust gives a spawned thread 2 MiB by
/// default, so a limit that is comfortable in release aborts the process in debug before
/// the budget is ever consulted, which is how it reached CI: Linux tests run on a larger
/// stack and passed, macOS did not.
///
/// A budget that stops the program cleanly is the entire point, so the limit follows the
/// profile rather than pretending one number fits both.
///
/// # Budget the stack, not just the depth
///
/// The limit is only safe relative to the stack the evaluator is running on. Debug
/// figures, measured by bisecting `ulimit -s` against `spin(n)` tail recursion:
///
/// | Stack | Deepest clean recursion (debug) |
/// |-------|-------------------------------|
/// | 2 MiB (Rust's default for a spawned thread) | 8 |
/// | 4 MiB | 16 |
/// | 8 MiB (a process main thread) | 32 — the default below, with nothing to spare |
/// | 16 MiB | under 32 for the IR path |
///
/// That is roughly **256 KB of debug stack per Rite call level**, four times the figure
/// this note used to quote; an embedder sizing a thread by the old number got aborts.
/// Release frames are far smaller, which is why the release default is 256.
///
/// `rite run` evaluates on the main thread, so the debug default below fits — but only
/// just. An embedder that evaluates on a spawned thread (a tokio worker included, at
/// 2 MiB) will abort well before the budget triggers, and must size the thread to
/// match:
///
/// ```ignore
/// std::thread::Builder::new().stack_size(32 * 1024 * 1024).spawn(|| {
///     // ctx.budget.max_call_depth = 512;
/// });
/// ```
///
/// The real cure is a smaller per-call frame — the boxed future per AST node — not a
/// bigger number here. `stacker`-style stack growth does not apply: the stack cannot be
/// grown across an `await`.
pub const DEFAULT_MAX_CALL_DEPTH: usize = if cfg!(debug_assertions) { 32 } else { 256 };

#[derive(Debug, Clone)]
pub struct ExecutionBudget {
    pub max_call_depth: usize,
    pub max_steps: u64,
    pub max_collection_size: usize,
    pub max_string_size: usize,
    pub timeout: Option<Duration>,
    inner: Arc<BudgetInner>,
    #[cfg(not(target_arch = "wasm32"))]
    started: Instant,
}

#[derive(Debug)]
struct BudgetInner {
    steps: AtomicU64,
    cancelled: AtomicBool,
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionBudget {
    pub fn new() -> Self {
        Self {
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            max_steps: 10_000_000,
            max_collection_size: 1_000_000,
            max_string_size: 10_000_000,
            timeout: Some(Duration::from_secs(60)),
            inner: Arc::new(BudgetInner {
                steps: AtomicU64::new(0),
                cancelled: AtomicBool::new(false),
            }),
            #[cfg(not(target_arch = "wasm32"))]
            started: Instant::now(),
        }
    }

    pub fn unlimited() -> Self {
        Self {
            max_call_depth: usize::MAX,
            max_steps: u64::MAX,
            max_collection_size: usize::MAX,
            max_string_size: usize::MAX,
            timeout: None,
            inner: Arc::new(BudgetInner {
                steps: AtomicU64::new(0),
                cancelled: AtomicBool::new(false),
            }),
            #[cfg(not(target_arch = "wasm32"))]
            started: Instant::now(),
        }
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn with_max_steps(mut self, n: u64) -> Self {
        self.max_steps = n;
        self
    }

    /// Reset wall-clock start and step counter for a new evaluation unit.
    ///
    /// Critical for long-lived hosts (REPL): the default budget starts a 60s
    /// wall clock at construction. Without a restart, idle time in the REPL
    /// counts against the next evaluation and surfaces as
    /// "execution wall-clock timeout exceeded".
    pub fn restart(&mut self) {
        self.inner = Arc::new(BudgetInner {
            steps: AtomicU64::new(0),
            cancelled: AtomicBool::new(false),
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.started = Instant::now();
        }
    }

    pub fn tick(&self) -> Result<(), BudgetError> {
        if self.inner.cancelled.load(Ordering::Relaxed) {
            return Err(BudgetError::Cancelled);
        }
        let steps = self.inner.steps.fetch_add(1, Ordering::Relaxed) + 1;
        if steps > self.max_steps {
            return Err(BudgetError::StepsExceeded);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(timeout) = self.timeout {
            if self.started.elapsed() > timeout {
                return Err(BudgetError::Timeout);
            }
        }
        Ok(())
    }

    pub fn check_depth(&self, depth: usize) -> Result<(), BudgetError> {
        if depth > self.max_call_depth {
            Err(BudgetError::StackOverflow)
        } else {
            Ok(())
        }
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    pub fn steps(&self) -> u64 {
        self.inner.steps.load(Ordering::Relaxed)
    }

    pub fn child(&self) -> Self {
        Self {
            max_call_depth: self.max_call_depth,
            max_steps: self.max_steps,
            max_collection_size: self.max_collection_size,
            max_string_size: self.max_string_size,
            timeout: self.timeout,
            inner: Arc::new(BudgetInner {
                steps: AtomicU64::new(0),
                cancelled: AtomicBool::new(false),
            }),
            #[cfg(not(target_arch = "wasm32"))]
            started: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {
    StepsExceeded,
    Timeout,
    Cancelled,
    StackOverflow,
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetError::StepsExceeded => write!(f, "execution step budget exceeded"),
            BudgetError::Timeout => write!(f, "execution wall-clock timeout exceeded"),
            BudgetError::Cancelled => write!(f, "execution cancelled"),
            BudgetError::StackOverflow => write!(f, "maximum call depth exceeded"),
        }
    }
}

impl std::error::Error for BudgetError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn restart_clears_wall_clock() {
        let mut b = ExecutionBudget::new().with_timeout(Duration::from_millis(30));
        thread::sleep(Duration::from_millis(40));
        assert!(matches!(b.tick(), Err(BudgetError::Timeout)));
        b.restart();
        assert!(b.tick().is_ok());
    }
}
