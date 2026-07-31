use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rite_runtime::{EvalError, Key, Value};
use std::time::Duration;

pub struct ClockCap {
    /// Optional fake clock for tests (unix millis).
    fake_now: RwLock<Option<i64>>,
}

impl ClockCap {
    pub fn new() -> Self {
        Self {
            fake_now: RwLock::new(None),
        }
    }

    pub fn set_fake_now(&self, millis: i64) {
        *self.fake_now.write() = Some(millis);
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "now",
            docs: "Current UTC timestamp as ISO-8601 string.",
            arity: 0,
            effectful: true,
            permission: "clock",
        },
        NativeFunctionDescriptor {
            name: "parse",
            docs: "Parse an ISO-8601 timestamp.",
            arity: 1,
            effectful: false,
            permission: "clock",
        },
        NativeFunctionDescriptor {
            name: "format",
            docs: "Format an RFC3339 timestamp with a strftime pattern, e.g. `%Y-%m-%d`. Answers `ok(string)`, or `err` if the timestamp or the pattern is not valid.",
            arity: 2,
            effectful: false,
            permission: "clock",
        },
        NativeFunctionDescriptor {
            name: "sleep",
            docs: "Sleep for a duration in milliseconds.",
            arity: 1,
            effectful: true,
            permission: "clock",
        },
        NativeFunctionDescriptor {
            name: "duration",
            docs: "Normalize a duration to whole milliseconds. Accepts an integer or float of milliseconds, or a string with a unit: `250ms`, `2s`, `5m`, `1h`, `1d`. Answers `ok(int)` or `err`.",
            arity: 1,
            effectful: false,
            permission: "clock",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        _effect: bool,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        perms.check_clock().map_err(EvalError::Permission)?;
        match method {
            "now" => {
                if let Some(ms) = *self.fake_now.read() {
                    let dt = DateTime::from_timestamp_millis(ms).unwrap_or_else(Utc::now);
                    return Ok(Value::string(dt.to_rfc3339()));
                }
                Ok(Value::string(Utc::now().to_rfc3339()))
            }
            "parse" => {
                let s = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("clock.parse expects string".into()))?;
                match DateTime::parse_from_rfc3339(s) {
                    Ok(dt) => Ok(Value::ok(Value::string(
                        dt.with_timezone(&Utc).to_rfc3339(),
                    ))),
                    Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                }
            }
            "format" => {
                let s = args.first().and_then(|v| v.as_str()).unwrap_or("");
                let pattern = args.get(1).and_then(|v| v.as_str()).unwrap_or("%+");
                let dt = match DateTime::parse_from_rfc3339(s) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(e) => {
                        return Ok(Value::err(Value::record(vec![
                            (Key::String("kind".into()), Value::string("clock.parse")),
                            (
                                Key::String("message".into()),
                                Value::string(format!("not an RFC3339 timestamp: {e}")),
                            ),
                        ])));
                    }
                };
                // `DateTime::format` panics on a bad pattern, and a script must not be
                // able to abort the host by writing "%Q". Collecting the items first
                // turns that into an ordinary `err`.
                let items: Vec<Item> = StrftimeItems::new(pattern).collect();
                if items.iter().any(|i| matches!(i, Item::Error)) {
                    return Ok(Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("clock.pattern")),
                        (
                            Key::String("message".into()),
                            Value::string(format!("invalid strftime pattern `{pattern}`")),
                        ),
                    ])));
                }
                Ok(Value::ok(Value::string(
                    dt.format_with_items(items.iter()).to_string(),
                )))
            }
            "sleep" => {
                let ms = args.first().and_then(|v| v.as_int()).unwrap_or(0).max(0) as u64;
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(Value::None)
            }
            // Normalize "how long" into milliseconds, so `@clock.sleep` and the socket
            // timeouts can all be fed the same way: `@clock.sleep(@clock.duration("2s")?)`.
            "duration" => Ok(match args.first() {
                Some(Value::Int(ms)) => Value::ok(Value::Int(*ms)),
                Some(Value::Float(f)) => Value::ok(Value::Int(f.round() as i64)),
                Some(v) => match v.as_str() {
                    Some(s) => parse_duration(s),
                    None => duration_err(&format!("cannot read `{v}` as a duration")),
                },
                None => duration_err("expects a duration"),
            }),
            other => Err(EvalError::Capability(format!("unknown @clock.{}", other))),
        }
    }
}

impl Default for ClockCap {
    fn default() -> Self {
        Self::new()
    }
}

fn duration_err(message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("clock.duration")),
        (Key::String("message".into()), Value::string(message)),
    ]))
}

/// `"250ms"`, `"2s"`, `"5m"`, `"1h"`, `"1d"` — and a bare number, which is read as
/// milliseconds so the string and integer forms agree.
///
/// Fractions are allowed (`"1.5s"`) because the natural way to say 1500ms out loud is
/// "one and a half seconds"; the result is still whole milliseconds.
fn parse_duration(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return duration_err("expects a duration");
    }
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let value: f64 = match num.parse() {
        Ok(v) => v,
        Err(_) => return duration_err(&format!("`{s}` is not a number followed by a unit")),
    };
    let scale = match unit.trim() {
        "" | "ms" => 1.0,
        "s" => 1_000.0,
        "m" => 60_000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        other => {
            return duration_err(&format!("unknown unit `{other}` — use ms, s, m, h or d"));
        }
    };
    Value::ok(Value::Int((value * scale).round() as i64))
}
