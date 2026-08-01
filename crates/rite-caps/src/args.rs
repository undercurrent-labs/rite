//! One argument vocabulary for every capability.
//!
//! Each capability grew its own: `fs.rs` had `path_arg` answering "expected path
//! string" with no idea which call complained, `crypto.rs` had `arg_str` naming the
//! callee and a 1-based index, `tcp.rs` had `int_arg` taking the whole message from
//! its caller, and `json`, `csv`, `clock`, `random`, `store` and `env` inlined
//! `args.get(n).and_then(…).unwrap_or(…)` per method.
//!
//! The messages were the smaller half of the problem. `unwrap_or` cannot tell a
//! missing argument from one of the wrong type, so several of these answered
//! something plausible instead of complaining:
//!
//! * `@fs.write(path)` with no content wrote an **empty file**, truncating whatever
//!   was there — the argument defaulted to `""`.
//! * `@random.int("a", "b")` answered `0`.
//! * `@csv.encode(not_a_list)` wrote an empty CSV.
//! * `@clock.sleep("soon")` slept for 0 ms.
//!
//! A capability reaches outside the program, so a wrong argument there is the worst
//! place to guess.

use rite_runtime::{EvalError, Value};

/// The argument at `i`, or an error naming the call and the position.
///
/// Positions are 1-based in the message because that is how a caller counts the
/// things they typed.
pub fn required<'a>(who: &str, args: &'a [Value], i: usize) -> Result<&'a Value, EvalError> {
    args.get(i).ok_or_else(|| {
        EvalError::Message(format!("{who} expects an argument at position {}", i + 1))
    })
}

/// A string argument.
pub fn str_arg(who: &str, args: &[Value], i: usize) -> Result<String, EvalError> {
    let v = required(who, args, i)?;
    v.as_str().map(|s| s.to_string()).ok_or_else(|| {
        EvalError::Message(format!(
            "{who} expects a string at position {}, got {}",
            i + 1,
            v.type_name()
        ))
    })
}

/// An integer argument.
pub fn int_arg(who: &str, args: &[Value], i: usize) -> Result<i64, EvalError> {
    let v = required(who, args, i)?;
    v.as_int().ok_or_else(|| {
        EvalError::Message(format!(
            "{who} expects an int at position {}, got {}",
            i + 1,
            v.type_name()
        ))
    })
}

/// An integer argument that may be omitted.
pub fn int_arg_or(who: &str, args: &[Value], i: usize, default: i64) -> Result<i64, EvalError> {
    match args.get(i) {
        None | Some(Value::None) => Ok(default),
        Some(_) => int_arg(who, args, i),
    }
}
