//! Turning command-line strings into a [`PermissionSet`] and an
//! [`ExecutionBudget`].
//!
//! Every tool that runs Rite has to do this, and for a long time `rite-cli` was
//! the only one — so it did it inline in `main.rs`, with `parse_duration` as a
//! private function beside it. Any second executable needs exactly the same
//! behaviour, and two copies of "what does `--allow fs:read=./data` mean" is the
//! kind of duplication that stays in sync right up until it does not.
//!
//! Deliberately **no `clap`**. The reusable part is the meaning of the strings,
//! not the flag declarations: a tool's own `--help` text, argument names and
//! defaults are its business, and adding `clap` to the embedding crate would put
//! an argument parser in the dependency tree of every host that just wants to
//! run a script.
//!
//! ```
//! use rite::options::RuntimeOptions;
//! let opts = RuntimeOptions {
//!     allow: vec!["fs:read=./data".into()],
//!     timeout: Some("30s".into()),
//!     ..Default::default()
//! };
//! let perms = opts.permissions().unwrap();
//! let budget = opts.budget().unwrap();
//! assert_eq!(budget.timeout, Some(std::time::Duration::from_secs(30)));
//! let _ = perms;
//! ```

use rite_caps::{Permission, PermissionSet};
use rite_runtime::ExecutionBudget;
use std::time::Duration;

/// The permission and resource options a Rite run accepts.
///
/// Field names match the flags they come from, so a `clap` struct maps onto this
/// one field for field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOptions {
    /// `--allow fs:read=./data`, repeatable.
    pub allow: Vec<String>,
    /// `--deny console`, repeatable. Applied after grants, so denying something
    /// granted in the same invocation denies it.
    pub deny: Vec<String>,
    /// `--allow-all`. Starts from every permission instead of the secure default.
    pub allow_all: bool,
    /// `--timeout 30s`. `500ms`, `30s`, `5m`, or a bare number of seconds.
    pub timeout: Option<String>,
    pub max_steps: Option<u64>,
    pub max_call_depth: Option<usize>,
    pub max_collection_size: Option<usize>,
    pub max_string_size: Option<usize>,
}

impl RuntimeOptions {
    /// Build the permission set.
    ///
    /// Grants first, then denials — so `--allow-all --deny net` is "everything
    /// except the network", which is the only reading that makes the pair useful.
    pub fn permissions(&self) -> Result<PermissionSet, String> {
        let mut perms = if self.allow_all {
            PermissionSet::allow_all()
        } else {
            PermissionSet::default_secure()
        };
        for spec in &self.allow {
            perms.grant(Permission::parse(spec)?);
        }
        for spec in &self.deny {
            // A bad `--deny` used to be discarded silently by `rite run`, which
            // meant a typo in a *revocation* left the permission in place. It is
            // an error here for the same reason a bad `--allow` is.
            perms.deny(Permission::parse(spec)?);
        }
        Ok(perms)
    }

    /// Build the execution budget, leaving anything unset at its default.
    pub fn budget(&self) -> Result<ExecutionBudget, String> {
        let mut budget = ExecutionBudget::new();
        if let Some(text) = &self.timeout {
            budget = budget.with_timeout(
                parse_duration(text).map_err(|e| format!("invalid --timeout {text:?}: {e}"))?,
            );
        }
        if let Some(n) = self.max_steps {
            budget = budget.with_max_steps(n);
        }
        if let Some(n) = self.max_call_depth {
            budget.max_call_depth = n;
        }
        if let Some(n) = self.max_collection_size {
            budget.max_collection_size = n;
        }
        if let Some(n) = self.max_string_size {
            budget.max_string_size = n;
        }
        Ok(budget)
    }
}

/// Parse `500ms`, `30s`, `5m`, or a bare number of seconds.
///
/// Was private in `rite-cli`. Public because every tool that takes a timeout
/// needs the same spellings, and because an embedder configuring a budget from
/// its own config file wants them too.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let t = s.trim();
    let invalid = |unit: &str| format!("expected a number{unit} (e.g. 500ms, 30s, 5m)");
    if let Some(ms) = t.strip_suffix("ms") {
        return ms
            .trim()
            .parse()
            .map(Duration::from_millis)
            .map_err(|_| invalid(" of milliseconds"));
    }
    if let Some(sec) = t.strip_suffix('s') {
        return sec
            .trim()
            .parse()
            .map(Duration::from_secs)
            .map_err(|_| invalid(" of seconds"));
    }
    if let Some(m) = t.strip_suffix('m') {
        return m
            .trim()
            .parse::<u64>()
            .map(|n| Duration::from_secs(n * 60))
            .map_err(|_| invalid(" of minutes"));
    }
    t.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| invalid(" of seconds"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(parse_duration("7"), Ok(Duration::from_secs(7)));
        assert_eq!(parse_duration(" 7 "), Ok(Duration::from_secs(7)));
    }

    #[test]
    fn rejects_garbage_instead_of_ignoring_it() {
        for bad in ["", "abc", "1h", "-5s", "1.5s", "ms", "s"] {
            assert!(parse_duration(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn defaults_are_the_secure_ones() {
        let perms = RuntimeOptions::default().permissions().expect("defaults");
        // console/clock/random allowed, fs/net/env/process/db denied.
        assert_eq!(
            format!("{perms:?}"),
            format!("{:?}", PermissionSet::default_secure())
        );
    }

    #[test]
    fn allow_all_then_deny_leaves_a_hole() {
        let opts = RuntimeOptions {
            allow_all: true,
            deny: vec!["console".into()],
            ..Default::default()
        };
        let perms = opts.permissions().expect("permissions");
        assert!(!perms.allow_all, "the blanket flag must be cleared");
        assert!(!perms.console, "console was denied");
    }

    /// `net` and `env` are host- and name-scoped, so the bare word is not a
    /// permission. Worth pinning: the error is the only thing that tells someone
    /// `--deny net` did not do what they assumed, and it used to be swallowed.
    #[test]
    fn a_scoped_permission_needs_its_scope() {
        for spec in ["net", "fs"] {
            let opts = RuntimeOptions {
                deny: vec![spec.to_string()],
                ..Default::default()
            };
            let err = opts
                .permissions()
                .expect_err("{spec} should not parse bare");
            assert!(err.contains(spec), "{err}");
        }
        // With a scope, both work.
        let opts = RuntimeOptions {
            allow: vec!["net=example.com".into(), "fs:read=./data".into()],
            ..Default::default()
        };
        assert!(opts.permissions().is_ok());
    }

    #[test]
    fn a_bad_permission_is_an_error_on_both_sides() {
        let bad_allow = RuntimeOptions {
            allow: vec!["not a permission".into()],
            ..Default::default()
        };
        assert!(bad_allow.permissions().is_err());
        // A typo in a *revocation* used to be discarded silently.
        let bad_deny = RuntimeOptions {
            deny: vec!["not a permission".into()],
            ..Default::default()
        };
        assert!(bad_deny.permissions().is_err());
    }

    #[test]
    fn every_budget_knob_is_reachable() {
        let opts = RuntimeOptions {
            timeout: Some("2s".into()),
            max_steps: Some(11),
            max_call_depth: Some(22),
            max_collection_size: Some(33),
            max_string_size: Some(44),
            ..Default::default()
        };
        let budget = opts.budget().expect("budget");
        assert_eq!(budget.timeout, Some(Duration::from_secs(2)));
        assert_eq!(budget.max_steps, 11);
        assert_eq!(budget.max_call_depth, 22);
        assert_eq!(budget.max_collection_size, 33);
        assert_eq!(budget.max_string_size, 44);
    }

    #[test]
    fn a_bad_timeout_is_reported_rather_than_ignored() {
        let opts = RuntimeOptions {
            timeout: Some("later".into()),
            ..Default::default()
        };
        let err = opts.budget().expect_err("bad timeout");
        assert!(err.contains("later"), "{err}");
    }
}
