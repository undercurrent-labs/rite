use crate::value::Value;
use rite_sem::LocalId;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Environment {
    frames: Vec<Frame>,
}

#[derive(Debug, Clone, Default)]
struct Frame {
    by_name: HashMap<String, Binding>,
    by_id: HashMap<u32, Binding>,
}

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

impl Environment {
    pub fn new() -> Self {
        let mut env = Self::default();
        env.push_frame();
        env
    }

    pub fn push_frame(&mut self) {
        self.frames.push(Frame::default());
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    pub fn define(&mut self, name: &str, local: LocalId, value: Value, mutable: bool) {
        let frame = self.frames.last_mut().expect("frame");
        let b = Binding { value, mutable };
        frame.by_name.insert(name.to_string(), b.clone());
        frame.by_id.insert(local.0, b);
    }

    pub fn define_name(&mut self, name: &str, value: Value, mutable: bool) {
        let frame = self.frames.last_mut().expect("frame");
        frame.by_name.insert(
            name.to_string(),
            Binding {
                value,
                mutable,
            },
        );
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            if let Some(b) = frame.by_name.get(name) {
                return Some(b.value.clone());
            }
        }
        None
    }

    pub fn get_local(&self, id: LocalId) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            if let Some(b) = frame.by_id.get(&id.0) {
                return Some(b.value.clone());
            }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        for frame in self.frames.iter_mut().rev() {
            if let Some(b) = frame.by_name.get_mut(name) {
                if !b.mutable {
                    return Err(format!("cannot assign to immutable binding `{}`", name));
                }
                b.value = value;
                return Ok(());
            }
        }
        Err(format!("undefined name `{}`", name))
    }

    pub fn assign_local(&mut self, id: LocalId, value: Value) -> Result<(), String> {
        for frame in self.frames.iter_mut().rev() {
            if let Some(b) = frame.by_id.get_mut(&id.0) {
                if !b.mutable {
                    return Err(format!("cannot assign to immutable local {}", id.0));
                }
                b.value = value;
                return Ok(());
            }
        }
        Err(format!("undefined local {}", id.0))
    }

    pub fn bindings_snapshot(&self) -> Vec<(String, Value)> {
        let mut map = HashMap::new();
        for frame in &self.frames {
            for (k, v) in &frame.by_name {
                map.insert(k.clone(), v.value.clone());
            }
        }
        map.into_iter().collect()
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }
}
