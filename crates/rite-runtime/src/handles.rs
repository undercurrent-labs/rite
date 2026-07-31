//! Open host resources belonging to one run.
//!
//! A capability that hands a script a handle — an open file today — keeps the
//! resource here rather than in a global of its own. Two reasons.
//!
//! **It closes when the run does.** The table lives on the `RuntimeContext`, so
//! dropping the context drops every resource in it. Under `rite run` that hardly
//! matters: the process exits and the OS cleans up. Under `RiteEngine` it matters
//! a great deal — the host process keeps going, and a guest that opened a file and
//! never closed it would hold that descriptor for the life of the host, with no
//! way for the next run to reach it. Cleanup that has to be *called* is cleanup
//! someone eventually forgets; this one cannot be forgotten because nothing calls
//! it.
//!
//! **One run cannot see another's handles.** Ids come from a counter per table, so
//! a handle from a finished run does not address a live resource in the next one.
//!
//! Handles are plain `⟨kind, id⟩` values, which is what lets a script pass one
//! between functions without the language needing any notion of lifetime.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// How many resources one run may hold open at once.
///
/// A limit exists so a loop that forgets to close says so in Rite's own words,
/// rather than surfacing the operating system's "too many open files" later, from
/// some unrelated call that merely happened to be next.
pub const DEFAULT_OPEN_HANDLE_LIMIT: usize = 1024;

struct Entry {
    kind: &'static str,
    resource: Box<dyn Any + Send>,
}

pub struct HandleTable {
    next: AtomicU64,
    limit: usize,
    entries: Mutex<HashMap<u64, Entry>>,
}

impl Default for HandleTable {
    fn default() -> Self {
        Self::new(DEFAULT_OPEN_HANDLE_LIMIT)
    }
}

impl std::fmt::Debug for HandleTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HandleTable({} open)", self.len())
    }
}

impl HandleTable {
    pub fn new(limit: usize) -> Self {
        HandleTable {
            // Ids start at 1 so a zero handle is always wrong rather than
            // accidentally the first one.
            next: AtomicU64::new(1),
            limit,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Take ownership of a resource and answer the id a script will refer to it by.
    ///
    /// `Err` carries the limit, so the capability can name itself in the message —
    /// the table does not know whether it is holding files or sockets.
    pub fn insert(&self, kind: &'static str, resource: Box<dyn Any + Send>) -> Result<u64, usize> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() >= self.limit {
            return Err(self.limit);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        entries.insert(id, Entry { kind, resource });
        Ok(id)
    }

    /// Work with a resource by id, if it is open and of the expected type.
    ///
    /// `None` covers both "closed already" and "that is a handle to something
    /// else" — a caller that needs to tell them apart should ask `kind_of` first.
    pub fn with<T: 'static, R>(&self, id: u64, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get_mut(&id)?;
        let resource = entry.resource.downcast_mut::<T>()?;
        Some(f(resource))
    }

    pub fn kind_of(&self, id: u64) -> Option<&'static str> {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.get(&id).map(|e| e.kind)
    }

    /// Close one resource. Answers whether it was open.
    ///
    /// Closing something already closed is deliberately not an error: `@tcp.close`
    /// settled that convention, and a script that closes in both a success and a
    /// cleanup path is being careful, not wrong.
    pub fn close(&self, id: u64) -> bool {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.remove(&id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_not_reused_after_a_close() {
        let table = HandleTable::default();
        let a = table.insert("test", Box::new(1u32)).unwrap();
        table.close(a);
        let b = table.insert("test", Box::new(2u32)).unwrap();
        assert_ne!(
            a, b,
            "a closed id came back and now addresses something else"
        );
    }

    #[test]
    fn closing_twice_is_not_an_error() {
        let table = HandleTable::default();
        let id = table.insert("test", Box::new(1u32)).unwrap();
        assert!(table.close(id));
        assert!(!table.close(id), "second close should report nothing to do");
    }

    #[test]
    fn the_limit_is_enforced_and_closing_frees_a_slot() {
        let table = HandleTable::new(2);
        let a = table.insert("test", Box::new(1u32)).unwrap();
        let _b = table.insert("test", Box::new(2u32)).unwrap();
        assert_eq!(table.insert("test", Box::new(3u32)), Err(2));
        table.close(a);
        assert!(
            table.insert("test", Box::new(4u32)).is_ok(),
            "a closed handle did not free its slot"
        );
    }

    #[test]
    fn a_handle_of_the_wrong_type_is_not_handed_over() {
        let table = HandleTable::default();
        let id = table.insert("test", Box::new(1u32)).unwrap();
        assert_eq!(table.with::<String, _>(id, |s| s.len()), None);
        assert_eq!(table.with::<u32, _>(id, |n| *n), Some(1));
    }

    /// The point of the whole design: no one calls anything to clean up.
    #[test]
    fn dropping_the_table_releases_what_it_holds() {
        use std::sync::Arc;
        let shared = Arc::new(7u8);
        let table = HandleTable::default();
        table.insert("test", Box::new(shared.clone())).unwrap();
        assert_eq!(Arc::strong_count(&shared), 2);
        drop(table);
        assert_eq!(
            Arc::strong_count(&shared),
            1,
            "the resource outlived the table it was in"
        );
    }
}
