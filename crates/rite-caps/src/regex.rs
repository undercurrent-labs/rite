use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use parking_lot::Mutex;
use regex::Regex;
use rite_runtime::{EvalError, Key, Value};

/// Regular expressions, as pure text transforms.
///
/// Every method answers a result: `ok(answer)` or `err` for a pattern that
/// does not compile — a bad pattern is data, not a crash, so `?` and the usual
/// postures apply. Matching is the `regex` crate's guaranteed-linear-time
/// engine: no backtracking, so no pathological pattern can hang a script,
/// which is why this capability needs no permission: like `@json`, it
/// computes and touches nothing.
pub struct RegexCap {
    /// Compiled patterns, keyed by source. A pipeline applies one pattern per
    /// emission; recompiling per line turned the linear-time engine into a
    /// compile-time benchmark. Bounded, cleared when full — a cache, not a leak.
    compiled: Mutex<IndexMap<String, Regex>>,
}

const CACHE_CAP: usize = 64;

impl RegexCap {
    pub fn new() -> Self {
        Self {
            compiled: Mutex::new(IndexMap::new()),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "is_match",
            docs: "Whether the pattern matches anywhere in the text. Answers `ok(bool)`, or `err` for a pattern that does not compile.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "find",
            docs: "The first match of the pattern in the text, as `ok(string)` — `ok(none)` when nothing matches.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "find_all",
            docs: "Every non-overlapping match, in order, as `ok(list)`.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "captures",
            docs: "The first match's groups as `ok(list)` — the whole match first, then each group, `none` for a group that did not participate. `ok(none)` when nothing matches.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "replace",
            docs: "Every match replaced. `$1`, `$2`, `${name}` in the replacement refer to capture groups. Answers `ok(string)`.",
            arity: 3,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "split",
            docs: "The text split around every match of the pattern, as `ok(list)`.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
    ];

    fn pattern(&self, source: &str) -> Result<Regex, Value> {
        let mut cache = self.compiled.lock();
        if let Some(re) = cache.get(source) {
            return Ok(re.clone());
        }
        match Regex::new(source) {
            Ok(re) => {
                if cache.len() >= CACHE_CAP {
                    cache.clear();
                }
                cache.insert(source.to_string(), re.clone());
                Ok(re)
            }
            Err(e) => Err(Value::err(Value::record(vec![
                (Key::String("kind".into()), Value::string("regex.pattern")),
                (Key::String("message".into()), Value::string(e.to_string())),
            ]))),
        }
    }

    pub fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        _perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        let text = args
            .first()
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| EvalError::Message(format!("regex.{method} expects text first")))?;
        let source = args
            .get(1)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| EvalError::Message(format!("regex.{method} expects a pattern")))?;
        let re = match self.pattern(&source) {
            Ok(re) => re,
            Err(err_value) => return Ok(err_value),
        };

        Ok(match method {
            "is_match" => Value::ok(Value::Bool(re.is_match(&text))),
            "find" => Value::ok(
                re.find(&text)
                    .map(|m| Value::string(m.as_str()))
                    .unwrap_or(Value::None),
            ),
            "find_all" => Value::ok(Value::list(
                re.find_iter(&text).map(|m| Value::string(m.as_str())),
            )),
            "captures" => Value::ok(match re.captures(&text) {
                Some(caps) => Value::list(
                    caps.iter()
                        .map(|g| g.map(|m| Value::string(m.as_str())).unwrap_or(Value::None)),
                ),
                None => Value::None,
            }),
            "replace" => {
                let replacement = args
                    .get(2)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        EvalError::Message("regex.replace expects a replacement".into())
                    })?;
                Value::ok(Value::string(
                    re.replace_all(&text, replacement.as_str()).into_owned(),
                ))
            }
            "split" => Value::ok(Value::list(re.split(&text).map(Value::string))),
            other => {
                return Err(EvalError::Capability(format!("unknown @regex.{}", other)));
            }
        })
    }
}

impl Default for RegexCap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_runtime::ResultValue;

    fn call(method: &str, args: Vec<Value>) -> Value {
        RegexCap::new()
            .call(method, args, &PermissionSet::default_secure())
            .expect("no eval error")
    }

    fn unwrap_ok(v: Value) -> Value {
        match v {
            Value::Result(ResultValue::Ok(inner)) => *inner,
            other => panic!("expected ok(...), got {other:?}"),
        }
    }

    #[test]
    fn matching_finding_and_capturing() {
        let text = || Value::string("ERROR 42: disk full");
        let hit = unwrap_ok(call("is_match", vec![text(), Value::string(r"^ERROR \d+")]));
        assert_eq!(hit, Value::Bool(true));

        let found = unwrap_ok(call("find", vec![text(), Value::string(r"\d+")]));
        assert_eq!(found, Value::string("42"));

        let caps = unwrap_ok(call(
            "captures",
            vec![text(), Value::string(r"ERROR (\d+): (\w+)")],
        ));
        assert_eq!(
            caps,
            Value::list(vec![
                Value::string("ERROR 42: disk"),
                Value::string("42"),
                Value::string("disk"),
            ])
        );
    }

    /// A bad pattern is an `err` value, not a crash — the usual postures apply.
    #[test]
    fn a_bad_pattern_is_an_err_value() {
        let out = call("is_match", vec![Value::string("x"), Value::string("(")]);
        assert!(matches!(out, Value::Result(ResultValue::Err(_))), "{out:?}");
    }

    #[test]
    fn replace_supports_group_references() {
        let out = unwrap_ok(call(
            "replace",
            vec![
                Value::string("2026-08-04"),
                Value::string(r"(\d+)-(\d+)-(\d+)"),
                Value::string("$3/$2/$1"),
            ],
        ));
        assert_eq!(out, Value::string("04/08/2026"));
    }
}
