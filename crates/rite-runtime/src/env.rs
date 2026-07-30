use crate::value::Value;
use parking_lot::RwLock;
use rite_sem::LocalId;
use std::collections::HashMap;
use std::sync::Arc;

/// Lexical environment: a chain of *shared* frames, innermost last.
///
/// Cloning an `Environment` copies the chain of handles but **not** the frames, so
/// every clone reads and writes the same bindings. That is what makes closures
/// lexically scoped: `Closure::env` holds a clone of the defining chain, which keeps
/// those frames alive after the defining call returns, resolves names against the
/// definition site instead of the caller, and still lets `x := …` inside an
/// `each`/`while_loop` body update the enclosing scope's mutable binding.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    frames: Vec<Arc<RwLock<Frame>>>,
}

#[derive(Debug, Default)]
struct Frame {
    /// Bindings in definition order. `by_name` and `by_id` are views onto these
    /// slots, so a write through either key is observed through both.
    slots: Vec<Binding>,
    by_name: HashMap<String, usize>,
    by_id: HashMap<u32, usize>,
}

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

impl Frame {
    fn add(&mut self, value: Value, mutable: bool) -> usize {
        self.slots.push(Binding { value, mutable });
        self.slots.len() - 1
    }
}

impl Environment {
    pub fn new() -> Self {
        let mut env = Self::default();
        env.push_frame();
        env
    }

    pub fn push_frame(&mut self) {
        self.frames.push(Arc::new(RwLock::new(Frame::default())));
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    fn innermost(&mut self) -> Arc<RwLock<Frame>> {
        if self.frames.is_empty() {
            self.push_frame();
        }
        self.frames.last().cloned().expect("frame")
    }

    pub fn define(&mut self, name: &str, local: LocalId, value: Value, mutable: bool) {
        let frame = self.innermost();
        let mut frame = frame.write();
        let idx = frame.add(value, mutable);
        frame.by_name.insert(name.to_string(), idx);
        frame.by_id.insert(local.0, idx);
    }

    pub fn define_name(&mut self, name: &str, value: Value, mutable: bool) {
        let frame = self.innermost();
        let mut frame = frame.write();
        let idx = frame.add(value, mutable);
        frame.by_name.insert(name.to_string(), idx);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            let frame = frame.read();
            if let Some(&idx) = frame.by_name.get(name) {
                return Some(frame.slots[idx].value.clone());
            }
        }
        None
    }

    pub fn get_local(&self, id: LocalId) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            let frame = frame.read();
            if let Some(&idx) = frame.by_id.get(&id.0) {
                return Some(frame.slots[idx].value.clone());
            }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        for frame in self.frames.iter().rev() {
            let mut frame = frame.write();
            if let Some(&idx) = frame.by_name.get(name) {
                if !frame.slots[idx].mutable {
                    return Err(format!("cannot assign to immutable binding `{}`", name));
                }
                frame.slots[idx].value = value;
                return Ok(());
            }
        }
        Err(format!("undefined name `{}`", name))
    }

    pub fn assign_local(&mut self, id: LocalId, value: Value) -> Result<(), String> {
        for frame in self.frames.iter().rev() {
            let mut frame = frame.write();
            if let Some(&idx) = frame.by_id.get(&id.0) {
                if !frame.slots[idx].mutable {
                    return Err(format!("cannot assign to immutable local {}", id.0));
                }
                frame.slots[idx].value = value;
                return Ok(());
            }
        }
        Err(format!("undefined local {}", id.0))
    }

    pub fn bindings_snapshot(&self) -> Vec<(String, Value)> {
        let mut map = HashMap::new();
        for frame in &self.frames {
            let frame = frame.read();
            for (k, &idx) in &frame.by_name {
                map.insert(k.clone(), frame.slots[idx].value.clone());
            }
        }
        map.into_iter().collect()
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Make `ambient`'s outermost (global) frame visible from this chain.
    ///
    /// No-op for environments captured in the same context — they already share that
    /// frame. It matters for closures built outside any running program (the HTTP
    /// capability layer rebuilds function values per request with a fresh
    /// `Environment`): the captured frames keep priority, with the host context's
    /// globals as the outermost fallback so top-level names still resolve.
    pub fn ensure_globals_from(&mut self, ambient: &Environment) {
        let Some(root) = ambient.frames.first() else {
            return;
        };
        if self.frames.iter().any(|f| Arc::ptr_eq(f, root)) {
            return;
        }
        self.frames.insert(0, Arc::clone(root));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    #[test]
    fn clone_shares_frames_so_writes_are_mutual() {
        let mut outer = Environment::new();
        outer.define_name("n", int(1), true);
        let captured = outer.clone();
        // Assigning through the capture updates the binding the outer scope reads.
        captured.clone().assign("n", int(2)).unwrap();
        assert_eq!(outer.get("n"), Some(int(2)));
        // …and a name defined later in the shared frame is visible from the capture.
        outer.define_name("m", int(3), false);
        assert_eq!(captured.get("m"), Some(int(3)));
    }

    #[test]
    fn frames_pushed_after_capture_are_invisible_to_it() {
        let mut env = Environment::new();
        env.define_name("n", int(1), false);
        let captured = env.clone();
        env.push_frame();
        env.define_name("n", int(99), false);
        assert_eq!(env.get("n"), Some(int(99)));
        assert_eq!(captured.get("n"), Some(int(1)));
        env.pop_frame();
        assert_eq!(env.get("n"), Some(int(1)));
    }

    #[test]
    fn popping_a_clone_does_not_shorten_the_original() {
        let mut env = Environment::new();
        env.push_frame();
        let mut captured = env.clone();
        captured.pop_frame();
        assert_eq!(captured.depth(), 1);
        assert_eq!(env.depth(), 2);
    }

    #[test]
    fn name_and_local_views_share_one_slot() {
        let mut env = Environment::new();
        env.define("n", LocalId(7), int(1), true);
        env.assign("n", int(2)).unwrap();
        assert_eq!(env.get_local(LocalId(7)), Some(int(2)));
        env.assign_local(LocalId(7), int(3)).unwrap();
        assert_eq!(env.get("n"), Some(int(3)));
    }

    #[test]
    fn immutable_bindings_reject_assignment() {
        let mut env = Environment::new();
        env.define_name("k", int(1), false);
        assert!(env.assign("k", int(2)).is_err());
        assert!(env.assign("missing", int(2)).is_err());
    }

    #[test]
    fn ensure_globals_grafts_only_foreign_roots() {
        let mut host = Environment::new();
        host.define_name("g", int(1), false);

        // Same lineage: nothing changes.
        let mut same = host.clone();
        same.push_frame();
        let before = same.depth();
        same.ensure_globals_from(&host);
        assert_eq!(same.depth(), before);

        // Foreign lineage: the host globals become the outermost fallback and the
        // captured bindings keep priority.
        let mut foreign = Environment::new();
        foreign.define_name("local", int(5), false);
        foreign.define_name("g", int(2), false);
        foreign.ensure_globals_from(&host);
        assert_eq!(foreign.depth(), 2);
        assert_eq!(foreign.get("local"), Some(int(5)));
        assert_eq!(foreign.get("g"), Some(int(2)));
        // A name only the host knows resolves through the grafted frame.
        host.define_name("host_only", int(9), false);
        assert_eq!(foreign.get("host_only"), Some(int(9)));
    }
}
