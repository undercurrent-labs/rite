use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rite_runtime::{EvalError, Value};
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
            docs: "Reserved, not implemented: the pattern is ignored and the timestamp is returned unchanged. Do not build on it.",
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
            docs: "Reserved, not implemented: returns the milliseconds it was given. Do not build on it.",
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
                // simplified: return as-is; pattern ignored beyond identity for v1
                let _pattern = args.get(1).and_then(|v| v.as_str()).unwrap_or("%+");
                Ok(Value::string(s))
            }
            "sleep" => {
                let ms = args.first().and_then(|v| v.as_int()).unwrap_or(0).max(0) as u64;
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(Value::None)
            }
            "duration" => {
                let ms = args.first().and_then(|v| v.as_int()).unwrap_or(0);
                Ok(Value::Int(ms))
            }
            other => Err(EvalError::Capability(format!("unknown @clock.{}", other))),
        }
    }
}

impl Default for ClockCap {
    fn default() -> Self {
        Self::new()
    }
}
