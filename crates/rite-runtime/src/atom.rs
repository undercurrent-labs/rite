use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomId(pub u32);

#[derive(Debug, Default)]
pub struct AtomInterner {
    inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    to_id: HashMap<String, AtomId>,
    to_str: Vec<Arc<str>>,
}

impl AtomInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&self, name: &str) -> AtomId {
        {
            let inner = self.inner.read();
            if let Some(id) = inner.to_id.get(name) {
                return *id;
            }
        }
        let mut inner = self.inner.write();
        if let Some(id) = inner.to_id.get(name) {
            return *id;
        }
        let id = AtomId(inner.to_str.len() as u32);
        inner.to_str.push(Arc::from(name));
        inner.to_id.insert(name.to_string(), id);
        id
    }

    pub fn resolve(&self, id: AtomId) -> Option<Arc<str>> {
        let inner = self.inner.read();
        inner.to_str.get(id.0 as usize).cloned()
    }

    pub fn name(&self, id: AtomId) -> String {
        self.resolve(id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("#?{}", id.0))
    }
}
