use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// `std::time::Instant` panics on wasm32-unknown-unknown. Wall-clock timeouts
// are only enforced on native; wasm relies on step/depth budgets.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

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
            max_call_depth: 256,
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
