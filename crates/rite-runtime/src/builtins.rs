//! Pure standard-library builtins.

use crate::atom::AtomInterner;
use crate::value::{Key, ResultValue, Value};
use crate::EvalError;
use indexmap::IndexMap;

/// Dispatch a pure builtin.
///
/// `atoms` is needed because `Display for Value` cannot resolve an atom's name and
/// renders it as its interner index: `str(#ok)` produced `"#0"`, and so did `"{status}"`
/// and `[#a, #b] → join(", ")`. Anything user-visible must go through
/// [`Value::to_display`], which takes the interner — that is why it is threaded here
/// rather than left for each builtin to do without.
pub fn call_builtin(
    name: &str,
    args: Vec<Value>,
    atoms: &AtomInterner,
) -> Result<Value, EvalError> {
    match name {
        "ok" => Ok(Value::ok(args.into_iter().next().unwrap_or(Value::None))),
        "err" => Ok(Value::err(args.into_iter().next().unwrap_or(Value::None))),
        "str" => Ok(Value::string(
            args.first().unwrap_or(&Value::None).to_display(atoms),
        )),
        "len" | "count" => builtin_count(args),
        "type_of" => Ok(Value::string(
            args.first().map(|v| v.type_name()).unwrap_or("none"),
        )),
        "first" => builtin_first(args),
        "last" => builtin_last(args),
        "rest" | "tail" => builtin_rest(args),
        "init" | "butlast" => builtin_init(args),
        "take" => builtin_take(args),
        "drop" => builtin_drop(args),
        "reverse" => builtin_reverse(args),
        "flatten" => builtin_flatten(args),
        "concat" => builtin_concat(args),
        "sum" => builtin_sum(args),
        "min" => builtin_min_max(args, true),
        "max" => builtin_min_max(args, false),
        "unique" => builtin_unique(args),
        "range" => builtin_range(args),
        "range_incl" => builtin_range_incl(args),
        "lines" => builtin_lines(args),
        "words" => builtin_words(args),
        "join" => builtin_join(args, atoms),
        "zip" => builtin_zip(args),
        "chunk" => builtin_chunk(args),
        "keys" => builtin_keys(args),
        "values" => builtin_values(args),
        "abs" => builtin_abs(args),
        "clamp" => builtin_clamp(args),
        "pow" => builtin_pow(args),
        "idiv" => builtin_idiv(args),
        "split" => builtin_split(args),
        "trim" => builtin_trim(args, true, true),
        "trim_start" => builtin_trim(args, true, false),
        "trim_end" => builtin_trim(args, false, true),
        "replace" => builtin_replace(args),
        "starts_with" => builtin_affix(args, true),
        "ends_with" => builtin_affix(args, false),
        "upper" => builtin_case(args, true),
        "lower" => builtin_case(args, false),
        "pad_start" => builtin_pad(args, true),
        "pad_end" => builtin_pad(args, false),
        "slice" => builtin_slice(args),
        "index_of" => builtin_index_of(args),
        "round" => builtin_round_family(args, 0),
        "floor" => builtin_round_family(args, 1),
        "ceil" => builtin_round_family(args, 2),
        "sqrt" => builtin_sqrt(args),
        "parse_int" => builtin_parse_number(args, true),
        "parse_float" => builtin_parse_number(args, false),
        "bytes" => builtin_bytes(args),
        "from_hex" => builtin_from_hex(args),
        "to_hex" => builtin_to_hex(args),
        "to_text" => builtin_to_text(args),
        "byte_at" => builtin_byte_at(args),
        "xor" => builtin_xor(args),
        "or_else" => builtin_or_else(args),
        "is_ok" => builtin_is_ok(args),
        "is_err" => builtin_is_err(args),
        "unwrap_or" => builtin_unwrap_or(args),
        "collect_results" => builtin_collect_results(args),
        "require" => builtin_require(args),
        "repeat" => builtin_repeat(args),
        "contains" => builtin_contains(args),
        "enumerate" | "with_index" => builtin_enumerate(args),
        "panic" => Err(EvalError::Panic(
            args.first()
                .map(|v| v.to_display(atoms))
                .unwrap_or_else(|| "panic".into()),
        )),
        "expect" => builtin_expect(args),
        "fail" => Err(EvalError::Message(
            args.first()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "test failed".into()),
        )),
        // Handled by the evaluator, not here: these need to call back into it — either
        // to invoke a closure argument, or (print/println) to reach the context's output
        // buffer. `Evaluator::call_native` dispatches them. They are listed so the error
        // says what is actually true; `while_loop`, `compose`, `print` and `println` used
        // to fall through to "unknown builtin", which was simply wrong.
        "map" | "keep" | "reject" | "reduce" | "each" | "find" | "any" | "all" | "group"
        | "parallel" | "while_loop" | "compose" | "print" | "println" | "and_then" | "sort" => Err(
            EvalError::Message(format!("builtin `{}` requires evaluator dispatch", name)),
        ),
        other => Err(EvalError::Message(format!("unknown builtin `{}`", other))),
    }
}

fn builtin_count(args: Vec<Value>) -> Result<Value, EvalError> {
    let v = args.into_iter().next().unwrap_or(Value::None);
    let n = match v {
        Value::List(xs) => xs.len() as i64,
        Value::String(s) => s.chars().count() as i64,
        Value::Record(r) => r.len() as i64,
        Value::Bytes(b) => b.len() as i64,
        _ => 0,
    };
    Ok(Value::Int(n))
}

/// Which kind of sequence a value is, so a result can be rebuilt as the same kind.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SeqKind {
    List,
    Str,
    Bytes,
}

/// A value viewed as a sequence of elements.
///
/// `count`, `slice`, `reverse`, `index_of`, `contains` and `repeat` already read a
/// string as a sequence of characters, but the rest of the family only understood
/// lists — and answered an empty *list* for anything else. `drop("abc", 1)` gave
/// `[]`, and the mistake surfaced later and somewhere else as
/// `upper expects a string, got list`. Every sequence builtin goes through this,
/// so a string is a sequence everywhere or nowhere.
///
/// Elements are ordinary values: a character is a one-character string, a byte is
/// an int, which is what `byte_at` already answers.
pub(crate) struct Seq {
    kind: SeqKind,
    pub(crate) items: Vec<Value>,
}

impl Seq {
    /// View a value as a sequence, or say so if it is not one.
    ///
    /// `who` names the builtin, because the message a caller sees has to point at
    /// the call they wrote rather than at some later victim of a wrong type.
    pub(crate) fn of(v: Option<Value>, who: &str) -> Result<Seq, EvalError> {
        match v {
            Some(Value::List(xs)) => Ok(Seq {
                kind: SeqKind::List,
                items: xs.into_iter().collect(),
            }),
            Some(Value::String(s)) => Ok(Seq {
                kind: SeqKind::Str,
                items: s.chars().map(|c| Value::string(c.to_string())).collect(),
            }),
            Some(Value::Bytes(b)) => Ok(Seq {
                kind: SeqKind::Bytes,
                items: b.iter().map(|byte| Value::Int(*byte as i64)).collect(),
            }),
            other => Err(EvalError::Message(format!(
                "{who} expects a list, string or bytes, got {}",
                other
                    .map(|v| v.type_name().to_string())
                    .unwrap_or_else(|| "none".into())
            ))),
        }
    }

    /// Rebuild a sequence of the same kind from elements taken out of this one.
    ///
    /// Bytes only accept ints in 0–255, which holds for anything that came from a
    /// byte sequence; a sort or a filter cannot invent a value outside it.
    fn rebuild(kind: SeqKind, items: Vec<Value>) -> Value {
        match kind {
            SeqKind::List => Value::list(items),
            SeqKind::Str => Value::string(
                items
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.to_string(),
                        other => format!("{other}"),
                    })
                    .collect::<String>(),
            ),
            SeqKind::Bytes => Value::Bytes(
                items
                    .iter()
                    .filter_map(|v| v.as_int())
                    .map(|n| n.clamp(0, 255) as u8)
                    .collect::<Vec<u8>>()
                    .into(),
            ),
        }
    }

    pub(crate) fn same(&self, items: Vec<Value>) -> Value {
        Seq::rebuild(self.kind, items)
    }

    /// An empty sequence of this kind — `""` for a string, not `[]`.
    fn empty(&self) -> Value {
        Seq::rebuild(self.kind, Vec::new())
    }
}

fn builtin_first(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "first")?;
    Ok(seq.items.first().cloned().unwrap_or(Value::None))
}

fn builtin_last(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "last")?;
    Ok(seq.items.last().cloned().unwrap_or(Value::None))
}

fn builtin_rest(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "rest")?;
    if seq.items.is_empty() {
        return Ok(seq.empty());
    }
    Ok(seq.same(seq.items[1..].to_vec()))
}

fn builtin_init(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "init")?;
    if seq.items.is_empty() {
        return Ok(seq.empty());
    }
    Ok(seq.same(seq.items[..seq.items.len() - 1].to_vec()))
}

fn builtin_take(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let seq = Seq::of(it.next(), "take")?;
    // pipeline: xs → take(n) passes the sequence first, then n from the stage args
    let n = it.next().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
    Ok(seq.same(seq.items.iter().take(n).cloned().collect()))
}

fn builtin_drop(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let seq = Seq::of(it.next(), "drop")?;
    let n = it.next().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
    Ok(seq.same(seq.items.iter().skip(n).cloned().collect()))
}

fn builtin_reverse(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "reverse")?;
    Ok(seq.same(seq.items.iter().rev().cloned().collect()))
}

fn builtin_concat(args: Vec<Value>) -> Result<Value, EvalError> {
    // Bytes concatenate into bytes. Assembling a packet from a header and a body
    // is most of what authoring bytes is for, and collecting them into a list of
    // two byte strings would be useless.
    if matches!(args.first(), Some(Value::Bytes(_))) {
        let mut out: Vec<u8> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            out.extend_from_slice(&bytes_arg(Some(a), "concat").map_err(|_| {
                EvalError::Message(format!(
                    "concat started with bytes, so argument {i} must be bytes or a string, got {}",
                    a.type_name()
                ))
            })?);
        }
        return Ok(Value::Bytes(out.into()));
    }
    let mut out = im::Vector::new();
    for a in args {
        match a {
            Value::List(xs) => out.extend(xs),
            other => out.push_back(other),
        }
    }
    Ok(Value::List(out))
}

fn builtin_flatten(args: Vec<Value>) -> Result<Value, EvalError> {
    let xs = list_arg(args.into_iter().next(), "flatten")?;
    let mut out = im::Vector::new();
    for x in xs {
        match x {
            Value::List(inner) => out.extend(inner),
            other => out.push_back(other),
        }
    }
    Ok(Value::List(out))
}

/// Sums a list of numbers, or bytes — a byte is a number, so a checksum is just
/// `sum`.
///
/// Non-numbers are refused rather than skipped. `sum(["1", "2"])` used to answer
/// `0`, and so did `sum("abc")`, `sum(none)` and `sum(⟨a: 1⟩)` — the same zero a
/// correct empty list gives, which is the one answer that cannot be told apart
/// from a right one.
fn builtin_sum(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "sum")?;
    let mut sum_i: i64 = 0;
    let mut sum_f: f64 = 0.0;
    let mut used_float = false;
    for x in &seq.items {
        match x {
            Value::Int(n) => {
                sum_i = sum_i
                    .checked_add(*n)
                    .ok_or_else(|| EvalError::Message("integer overflow in sum".into()))?;
            }
            Value::Float(f) => {
                used_float = true;
                sum_f += f;
            }
            other => {
                return Err(EvalError::Message(format!(
                    "sum expects numbers, got {}",
                    other.type_name()
                )));
            }
        }
    }
    if used_float {
        Ok(Value::Float(sum_f + sum_i as f64))
    } else {
        Ok(Value::Int(sum_i))
    }
}

/// The smallest or largest element, by the same ordering `sort` uses — so
/// `min("cba")` is `"a"` and `min` of bytes is the smallest byte.
fn builtin_min_max(args: Vec<Value>, is_min: bool) -> Result<Value, EvalError> {
    let who = if is_min { "min" } else { "max" };
    let seq = Seq::of(args.into_iter().next(), who)?;
    let mut best: Option<Value> = None;
    for x in seq.items {
        match &best {
            None => best = Some(x),
            Some(b) => {
                let cmp = try_compare_values(&x, b)?;
                if (is_min && cmp.is_lt()) || (!is_min && cmp.is_gt()) {
                    best = Some(x);
                }
            }
        }
    }
    // Empty is still `none`: there is no smallest element of nothing, and that is
    // a different situation from being handed the wrong type, which now raises.
    Ok(best.unwrap_or(Value::None))
}

/// `sort(seq)` with no comparator: the language's own order.
pub(crate) fn sort_by_natural_order(seq: Seq) -> Result<Value, EvalError> {
    let mut v = seq.items.clone();
    // `sort_by` cannot fail, so the first incomparable pair is remembered and
    // raised afterwards. The sort still runs to completion — its output is
    // discarded, which is the point: a half-ordered list must not escape.
    let mut failure: Option<EvalError> = None;
    v.sort_by(|a, b| match try_compare_values(a, b) {
        Ok(o) => o,
        Err(e) => {
            failure.get_or_insert(e);
            std::cmp::Ordering::Equal
        }
    });
    match failure {
        Some(e) => Err(e),
        None => Ok(seq.same(v)),
    }
}
fn builtin_unique(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "unique")?;
    let mut out: Vec<Value> = Vec::new();
    for x in &seq.items {
        if !out.iter().any(|y: &Value| y.structural_eq(x)) {
            out.push(x.clone());
        }
    }
    Ok(seq.same(out))
}

fn builtin_range(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let start = it.next().and_then(|v| v.as_int()).unwrap_or(0);
    let end = it.next().and_then(|v| v.as_int()).unwrap_or(start);
    let step = it.next().and_then(|v| v.as_int()).unwrap_or(1);
    if step == 0 {
        return Err(EvalError::Message("range step cannot be zero".into()));
    }
    let mut xs = im::Vector::new();
    let mut i = start;
    while if step > 0 { i < end } else { i > end } {
        xs.push_back(Value::Int(i));
        // Stepping past i64::MIN/MAX ends the range instead of panicking.
        match i.checked_add(step) {
            Some(next) => i = next,
            None => break,
        }
    }
    Ok(Value::List(xs))
}

fn builtin_range_incl(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let start = it.next().and_then(|v| v.as_int()).unwrap_or(0);
    let end = it.next().and_then(|v| v.as_int()).unwrap_or(start);
    let step = it.next().and_then(|v| v.as_int()).unwrap_or(1);
    if step == 0 {
        return Err(EvalError::Message("range step cannot be zero".into()));
    }
    let mut xs = im::Vector::new();
    let mut i = start;
    while if step > 0 { i <= end } else { i >= end } {
        xs.push_back(Value::Int(i));
        match i.checked_add(step) {
            Some(next) => i = next,
            None => break,
        }
    }
    Ok(Value::List(xs))
}

/// Splitting text is a string operation, and a non-string is a mistake rather than
/// an empty document: both of these answered `[]` for a list, which reads exactly
/// like a file that happened to be empty.
fn builtin_lines(args: Vec<Value>) -> Result<Value, EvalError> {
    let s = str_arg(args.into_iter().next(), "lines")?;
    let lines: Vec<Value> = s.lines().map(Value::string).collect();
    Ok(Value::list(lines))
}

fn builtin_words(args: Vec<Value>) -> Result<Value, EvalError> {
    let s = str_arg(args.into_iter().next(), "words")?;
    let words: Vec<Value> = s.split_whitespace().map(Value::string).collect();
    Ok(Value::list(words))
}

fn builtin_join(args: Vec<Value>, atoms: &AtomInterner) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let seq = Seq::of(it.next(), "join")?;
    let sep = it
        .next()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "".into());
    let parts: Vec<String> = seq.items.iter().map(|v| v.to_display(atoms)).collect();
    Ok(Value::string(parts.join(&sep)))
}

/// Records only. `keys("abc")` answered `[]` — a string has no keys, and an empty
/// list is indistinguishable from a record that has none.
fn record_arg(v: Option<Value>, who: &str) -> Result<IndexMap<Key, Value>, EvalError> {
    match v {
        Some(Value::Record(r)) => Ok(r),
        other => Err(EvalError::Message(format!(
            "{who} expects a record, got {}",
            other
                .map(|v| v.type_name().to_string())
                .unwrap_or_else(|| "none".into())
        ))),
    }
}

fn builtin_keys(args: Vec<Value>) -> Result<Value, EvalError> {
    let r = record_arg(args.into_iter().next(), "keys")?;
    Ok(Value::list(
        r.keys()
            .map(|k| Value::string(k.as_str()))
            .collect::<Vec<_>>(),
    ))
}

fn builtin_values(args: Vec<Value>) -> Result<Value, EvalError> {
    let r = record_arg(args.into_iter().next(), "values")?;
    Ok(Value::list(r.values().cloned().collect::<Vec<_>>()))
}

fn builtin_abs(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::Int(n)) => n
            .checked_abs()
            .map(Value::Int)
            .ok_or_else(|| EvalError::Message("integer overflow".into())),
        Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
        _ => Err(EvalError::Message("abs expects a number".into())),
    }
}

fn builtin_clamp(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let v = it.next().unwrap_or(Value::None);
    let lo = it.next().unwrap_or(Value::None);
    let hi = it.next().unwrap_or(Value::None);
    // `Ord::clamp`/`f64::clamp` assert `min <= max` — report it instead of panicking.
    if let (Some(a), Some(b)) = (lo.as_float(), hi.as_float()) {
        if a > b || a.is_nan() || b.is_nan() {
            return Err(EvalError::Message(
                "clamp expects a lower bound not greater than the upper bound".into(),
            ));
        }
    }
    match (&v, &lo, &hi) {
        (Value::Int(n), Value::Int(a), Value::Int(b)) => Ok(Value::Int((*n).clamp(*a, *b))),
        (Value::Float(n), Value::Float(a), Value::Float(b)) => Ok(Value::Float(n.clamp(*a, *b))),
        _ => {
            let n = match v {
                Value::Int(n) => n as f64,
                Value::Float(f) => f,
                _ => return Ok(v),
            };
            let a = match lo {
                Value::Int(n) => n as f64,
                Value::Float(f) => f,
                _ => n,
            };
            let b = match hi {
                Value::Int(n) => n as f64,
                Value::Float(f) => f,
                _ => n,
            };
            Ok(Value::Float(n.clamp(a, b)))
        }
    }
}

fn builtin_pow(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let base = it.next().unwrap_or(Value::Int(0));
    let exp = it.next().unwrap_or(Value::Int(0));
    match (&base, &exp) {
        // `i64::pow` panics on overflow; fall through to the float result like `b > 32` does.
        (Value::Int(a), Value::Int(b)) if *b >= 0 && *b <= 32 => Ok(a
            .checked_pow(*b as u32)
            .map(Value::Int)
            .unwrap_or_else(|| Value::Float((*a as f64).powf(*b as f64)))),
        (Value::Int(a), Value::Int(b)) => Ok(Value::Float((*a as f64).powf(*b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64).powf(*b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powf(*b as f64))),
        _ => Err(EvalError::Message("pow expects numbers".into())),
    }
}

fn builtin_idiv(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let a = it.next().and_then(|v| v.as_int()).unwrap_or(0);
    let b = it.next().and_then(|v| v.as_int()).unwrap_or(0);
    if b == 0 {
        return Err(EvalError::Message("division by zero".into()));
    }
    // `i64::MIN / -1` overflows.
    a.checked_div(b)
        .map(Value::Int)
        .ok_or_else(|| EvalError::Message("integer overflow".into()))
}

fn builtin_xor(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let a = it.next().map(|v| v.is_truthy()).unwrap_or(false);
    let b = it.next().map(|v| v.is_truthy()).unwrap_or(false);
    Ok(Value::Bool(a ^ b))
}

fn builtin_or_else(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let r = it.next().unwrap_or(Value::None);
    let fallback = it.next().unwrap_or(Value::None);
    match r {
        Value::Result(ResultValue::Ok(v)) => Ok(*v),
        Value::Result(ResultValue::Err(_)) => Ok(fallback),
        Value::None => Ok(fallback),
        other => Ok(other),
    }
}

fn builtin_is_ok(args: Vec<Value>) -> Result<Value, EvalError> {
    Ok(Value::Bool(matches!(
        args.into_iter().next(),
        Some(Value::Result(ResultValue::Ok(_)))
    )))
}

fn builtin_is_err(args: Vec<Value>) -> Result<Value, EvalError> {
    Ok(Value::Bool(matches!(
        args.into_iter().next(),
        Some(Value::Result(ResultValue::Err(_)))
    )))
}

fn builtin_unwrap_or(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let r = it.next().unwrap_or(Value::None);
    let d = it.next().unwrap_or(Value::None);
    match r {
        Value::Result(ResultValue::Ok(v)) => Ok(*v),
        Value::Result(ResultValue::Err(_)) => Ok(d),
        Value::None => Ok(d),
        other => Ok(other),
    }
}

fn builtin_repeat(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let val = it.next().unwrap_or(Value::None);
    let n = it.next().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
    // `str::repeat` aborts on capacity overflow and the list loop would spin for ages;
    // refuse absurd counts instead.
    const MAX_REPEAT: usize = 1 << 26;
    let unit = match &val {
        Value::String(s) => s.len().max(1),
        Value::List(xs) => xs.len().max(1),
        _ => 1,
    };
    if unit.checked_mul(n).is_none_or(|total| total > MAX_REPEAT) {
        return Err(EvalError::Message("repeat count too large".into()));
    }
    match val {
        Value::String(s) => Ok(Value::string(s.repeat(n))),
        Value::List(xs) => {
            let mut out = im::Vector::new();
            for _ in 0..n {
                out.extend(xs.clone());
            }
            Ok(Value::List(out))
        }
        other => Ok(Value::list(
            std::iter::repeat_n(other, n).collect::<Vec<_>>(),
        )),
    }
}

fn builtin_contains(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let container = it.next().unwrap_or(Value::None);
    let item = it.next().unwrap_or(Value::None);
    Ok(Value::Bool(membership(&item, &container)))
}

fn builtin_enumerate(args: Vec<Value>) -> Result<Value, EvalError> {
    let seq = Seq::of(args.into_iter().next(), "enumerate")?;
    // Always a list of pairs, whatever went in: the pairs are not characters, so
    // there is no string to rebuild.
    let out: Vec<Value> = seq
        .items
        .into_iter()
        .enumerate()
        .map(|(i, x)| Value::list(vec![Value::Int(i as i64), x]))
        .collect();
    Ok(Value::list(out))
}

/// Lists only, and it says so.
///
/// `zip` and `flatten` are about the *structure* of a list of lists, which a string
/// does not have — pairing two strings character by character is a different
/// operation wearing the same name. They used to answer `[]` for a string, which is
/// the wrong answer rather than a refusal.
fn list_arg(v: Option<Value>, who: &str) -> Result<im::Vector<Value>, EvalError> {
    match v {
        Some(Value::List(xs)) => Ok(xs),
        other => Err(EvalError::Message(format!(
            "{who} expects a list, got {}",
            other
                .map(|v| v.type_name().to_string())
                .unwrap_or_else(|| "none".into())
        ))),
    }
}

fn builtin_zip(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let a = list_arg(it.next(), "zip")?;
    let b = list_arg(it.next(), "zip")?;
    let out: Vec<Value> = a
        .into_iter()
        .zip(b)
        .map(|(x, y)| Value::list(vec![x, y]))
        .collect();
    Ok(Value::list(out))
}

fn builtin_chunk(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let seq = Seq::of(it.next(), "chunk")?;
    let size = it.next().and_then(|v| v.as_int()).unwrap_or(1).max(1) as usize;
    // The pieces keep the kind they came from — chunking a string gives strings —
    // but the chunks themselves are a list, since a list of strings is not a string.
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for x in seq.items.iter() {
        cur.push(x.clone());
        if cur.len() == size {
            out.push(seq.same(std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        out.push(seq.same(cur));
    }
    Ok(Value::list(out))
}

fn builtin_collect_results(args: Vec<Value>) -> Result<Value, EvalError> {
    // `ok([])` for a non-list said "every one of your results succeeded" about a
    // thing that was never a list of results.
    let xs = list_arg(args.into_iter().next(), "collect_results")?;
    let mut out = Vec::new();
    for x in xs {
        match x {
            Value::Result(ResultValue::Ok(v)) => out.push(*v),
            Value::Result(ResultValue::Err(e)) => return Ok(Value::err(*e)),
            other => out.push(other),
        }
    }
    Ok(Value::ok(Value::list(out)))
}

fn builtin_require(args: Vec<Value>) -> Result<Value, EvalError> {
    // require(record, field) or used as pipeline: value → require .name is member
    let mut it = args.into_iter();
    let val = it.next().unwrap_or(Value::None);
    if matches!(val, Value::None) {
        return Ok(Value::err(Value::string("required value is none")));
    }
    Ok(val)
}

fn builtin_expect(args: Vec<Value>) -> Result<Value, EvalError> {
    // expect actual = expected  is binary in syntax; here expect(actual, expected) or expect(bool)
    if args.len() == 1 {
        if args[0].is_truthy() {
            return Ok(Value::Bool(true));
        }
        return Err(EvalError::Message("expectation failed".into()));
    }
    if args.len() >= 2 && args[0].structural_eq(&args[1]) {
        return Ok(Value::Bool(true));
    }
    Err(EvalError::Message(format!(
        "expectation failed: {} != {}",
        args.first().unwrap_or(&Value::None),
        args.get(1).unwrap_or(&Value::None)
    )))
}

/// Order two values, or say why they cannot be ordered.
///
/// Ordering used to be total by fiat: every pair this did not understand answered
/// `Equal`. That made `"a" <= 1` and `"a" >= 1` both true while `"a" = 1` was
/// false, and it made `sort` on a mixed list answer the list back unchanged —
/// a plausible result that is not sorted, which is the one kind of wrong answer
/// that never announces itself. The comparator was not transitive either, so the
/// order it did produce was unspecified rather than merely surprising.
///
/// What is ordered, and how:
///
/// * numbers, including `int` against `float`, numerically;
/// * strings, by Unicode scalar order;
/// * `bool`, `false` before `true`;
/// * `bytes`, lexicographically;
/// * lists, lexicographically — element by element, then by length, so ordering a
///   list of lists is defined exactly when ordering their elements is.
///
/// Everything else is an error: two different kinds, two atoms (symbols carry no
/// order), two records (field order is insertion order, so any answer would be an
/// artefact of how they were built), functions, handles, results, and `NaN`, which
/// is unordered against everything including itself.
///
/// Equality is untouched and stays total: `"a" = 1` is `false`, not an error.
pub fn try_compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, EvalError> {
    use std::cmp::Ordering;
    fn nope(a: &Value, b: &Value) -> EvalError {
        EvalError::Message(if a.type_name() == b.type_name() {
            format!("cannot order two {} values", a.type_name())
        } else {
            format!("cannot order {} against {}", a.type_name(), b.type_name())
        })
    }
    fn floats(x: f64, y: f64, a: &Value, b: &Value) -> Result<Ordering, EvalError> {
        x.partial_cmp(&y)
            .ok_or_else(|| EvalError::Message(format!("cannot order {} against {}", a, b)))
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => floats(*x, *y, a, b),
        (Value::Int(x), Value::Float(y)) => floats(*x as f64, *y, a, b),
        (Value::Float(x), Value::Int(y)) => floats(*x, *y as f64, a, b),
        (Value::String(x), Value::String(y)) => Ok(x.as_ref().cmp(y.as_ref())),
        (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),
        (Value::Bytes(x), Value::Bytes(y)) => Ok(x.as_ref().cmp(y.as_ref())),
        (Value::List(x), Value::List(y)) => {
            for (xa, xb) in x.iter().zip(y.iter()) {
                match try_compare_values(xa, xb)? {
                    Ordering::Equal => continue,
                    other => return Ok(other),
                }
            }
            Ok(x.len().cmp(&y.len()))
        }
        _ => Err(nope(a, b)),
    }
}

pub fn merge_records(
    left: &IndexMap<Key, Value>,
    right: &IndexMap<Key, Value>,
) -> IndexMap<Key, Value> {
    let mut out = left.clone();
    for (k, v) in right {
        out.insert(k.clone(), v.clone());
    }
    out
}

pub fn list_remove_first(list: &im::Vector<Value>, item: &Value) -> im::Vector<Value> {
    let mut out = im::Vector::new();
    let mut removed = false;
    for x in list {
        if !removed && x.structural_eq(item) {
            removed = true;
            continue;
        }
        out.push_back(x.clone());
    }
    out
}

pub fn membership(item: &Value, container: &Value) -> bool {
    match container {
        Value::List(xs) => xs.iter().any(|x| x.structural_eq(item)),
        Value::Record(r) => match item {
            Value::String(s) => r.contains_key(&Key::String(s.to_string())),
            Value::Atom(_) => {
                // atom membership by name handled externally
                false
            }
            _ => r.values().any(|v| v.structural_eq(item)),
        },
        Value::String(s) => item.as_str().map(|sub| s.contains(sub)).unwrap_or(false),
        _ => false,
    }
}

// ── Strings ──────────────────────────────────────────────────────────────────
//
// Every one of these is character-indexed, not byte-indexed, because `count`
// already counts characters — `count("δ")` is 1. A byte-indexed `slice` next to
// a character-counting `count` would be a trap that only shows up on non-ASCII
// input, which is exactly when it is hardest to debug.

fn str_arg(v: Option<Value>, what: &str) -> Result<String, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.to_string()),
        Some(other) => Err(EvalError::Message(format!(
            "{what} expects a string, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message(format!("{what} expects a string"))),
    }
}

fn builtin_split(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let s = str_arg(it.next(), "split")?;
    let sep = it.next().and_then(|v| v.as_str().map(|s| s.to_string()));
    let parts: Vec<Value> = match sep.as_deref() {
        // Splitting on "" yields the characters, which is the useful reading and
        // avoids Rust's empty-string behaviour of emitting leading/trailing "".
        None | Some("") => s.chars().map(|c| Value::string(c.to_string())).collect(),
        Some(sep) => s.split(sep).map(Value::string).collect(),
    };
    Ok(Value::list(parts))
}

fn builtin_trim(args: Vec<Value>, start: bool, end: bool) -> Result<Value, EvalError> {
    let s = str_arg(args.into_iter().next(), "trim")?;
    let out = match (start, end) {
        (true, true) => s.trim(),
        (true, false) => s.trim_start(),
        (false, true) => s.trim_end(),
        (false, false) => &s[..],
    };
    Ok(Value::string(out))
}

fn builtin_replace(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let s = str_arg(it.next(), "replace")?;
    let from = str_arg(it.next(), "replace")?;
    let to = str_arg(it.next(), "replace")?;
    if from.is_empty() {
        // Rust would splice `to` between every character; refuse instead of
        // quietly producing something nobody asked for.
        return Err(EvalError::Message(
            "replace needs a non-empty string to look for".into(),
        ));
    }
    Ok(Value::string(s.replace(&from, &to)))
}

fn builtin_affix(args: Vec<Value>, start: bool) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let name = if start { "starts_with" } else { "ends_with" };
    let s = str_arg(it.next(), name)?;
    let part = str_arg(it.next(), name)?;
    Ok(Value::Bool(if start {
        s.starts_with(&part)
    } else {
        s.ends_with(&part)
    }))
}

fn builtin_case(args: Vec<Value>, upper: bool) -> Result<Value, EvalError> {
    let s = str_arg(
        args.into_iter().next(),
        if upper { "upper" } else { "lower" },
    )?;
    Ok(Value::string(if upper {
        s.to_uppercase()
    } else {
        s.to_lowercase()
    }))
}

fn builtin_pad(args: Vec<Value>, start: bool) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let name = if start { "pad_start" } else { "pad_end" };
    let s = str_arg(it.next(), name)?;
    let width = it
        .next()
        .and_then(|v| v.as_int())
        .ok_or_else(|| EvalError::Message(format!("{name} expects a width")))?;
    let fill = it
        .next()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| " ".into());
    let fill_char = fill.chars().next().unwrap_or(' ');
    let len = s.chars().count() as i64;
    if width <= len {
        return Ok(Value::string(s));
    }
    let pad: String = std::iter::repeat_n(fill_char, (width - len) as usize).collect();
    Ok(Value::string(if start {
        format!("{pad}{s}")
    } else {
        format!("{s}{pad}")
    }))
}

/// `slice(s, start)` or `slice(s, start, end)` — character indices, end exclusive.
/// A negative index counts from the end; out-of-range clamps rather than failing,
/// which keeps it usable on untrusted input.
fn builtin_slice(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let first = it.next();
    // Lists slice too — the same operation, and refusing would be arbitrary.
    if let Some(Value::List(xs)) = &first {
        let len = xs.len() as i64;
        let start = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(0), len);
        let end = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(len), len);
        let mut out = im::Vector::new();
        for i in start..end.max(start) {
            if let Some(v) = xs.get(i as usize) {
                out.push_back(v.clone());
            }
        }
        return Ok(Value::List(out));
    }
    if let Some(Value::Bytes(b)) = &first {
        let len = b.len() as i64;
        let start = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(0), len);
        let end = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(len), len);
        return Ok(Value::Bytes(
            b[start as usize..end.max(start) as usize].into(),
        ));
    }
    let s = str_arg(first, "slice")?;
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    let start = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(0), len);
    let end = resolve_index(it.next().and_then(|v| v.as_int()).unwrap_or(len), len);
    Ok(Value::string(
        chars[start as usize..end.max(start) as usize]
            .iter()
            .collect::<String>(),
    ))
}

fn resolve_index(i: i64, len: i64) -> i64 {
    if i < 0 {
        (len + i).clamp(0, len)
    } else {
        i.clamp(0, len)
    }
}

/// Character index of the first occurrence, or `none` when absent.
///
/// `none` rather than `-1`: a sentinel that is also a valid index is how
/// off-by-one bugs get written, and `??` already handles the absent case.
fn builtin_index_of(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let haystack = it.next();
    let needle = it.next().unwrap_or(Value::None);

    // A string keeps looking for a *substring*, which is not the same question as
    // "where is this element" — `index_of("abc", "bc")` is 1, and reading the
    // string as a sequence of characters would make that a miss.
    if let Some(Value::String(s)) = &haystack {
        let needle = str_arg(Some(needle), "index_of")?;
        return Ok(match s.find(&needle) {
            Some(byte_idx) => Value::Int(s[..byte_idx].chars().count() as i64),
            None => Value::None,
        });
    }

    // Lists and bytes look for an element. This used to raise
    // `index_of expects a string, got list` — the one member of the family that
    // refused a list, while `contains` answered the same question happily.
    let seq = Seq::of(haystack, "index_of")?;
    Ok(
        match seq.items.iter().position(|x| x.structural_eq(&needle)) {
            Some(i) => Value::Int(i as i64),
            None => Value::None,
        },
    )
}

// ── Numbers ──────────────────────────────────────────────────────────────────

fn num_arg(v: Option<Value>, what: &str) -> Result<f64, EvalError> {
    match v {
        Some(Value::Int(i)) => Ok(i as f64),
        Some(Value::Float(f)) => Ok(f),
        Some(other) => Err(EvalError::Message(format!(
            "{what} expects a number, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message(format!("{what} expects a number"))),
    }
}

/// `round`, `floor` and `ceil` answer with an `int`, because that is what the
/// caller wanted one for. `round` is half-away-from-zero, so `round(-0.5)` is
/// `-1` rather than Rust's `-0`.
fn builtin_round_family(args: Vec<Value>, mode: u8) -> Result<Value, EvalError> {
    let name = match mode {
        0 => "round",
        1 => "floor",
        _ => "ceil",
    };
    let n = num_arg(args.into_iter().next(), name)?;
    let out = match mode {
        0 => n.round(),
        1 => n.floor(),
        _ => n.ceil(),
    };
    if out.is_finite() && out.abs() < i64::MAX as f64 {
        Ok(Value::Int(out as i64))
    } else {
        Ok(Value::Float(out))
    }
}

fn builtin_sqrt(args: Vec<Value>) -> Result<Value, EvalError> {
    let n = num_arg(args.into_iter().next(), "sqrt")?;
    if n < 0.0 {
        return Err(EvalError::Message(
            "sqrt of a negative number is not a number".into(),
        ));
    }
    Ok(Value::Float(n.sqrt()))
}

/// Parsing answers with a Result, because the input is usually untrusted and
/// `?` is how the rest of the language handles that.
fn builtin_parse_number(args: Vec<Value>, int: bool) -> Result<Value, EvalError> {
    let name = if int { "parse_int" } else { "parse_float" };
    let s = str_arg(args.into_iter().next(), name)?;
    let t = s.trim();
    if int {
        match t.parse::<i64>() {
            Ok(i) => Ok(Value::ok(Value::Int(i))),
            Err(_) => Ok(Value::err(Value::string(format!(
                "`{t}` is not a whole number"
            )))),
        }
    } else {
        match t.parse::<f64>() {
            Ok(f) => Ok(Value::ok(Value::Float(f))),
            Err(_) => Ok(Value::err(Value::string(format!("`{t}` is not a number")))),
        }
    }
}

// ── Bytes ────────────────────────────────────────────────────────────────────
//
// `Value::Bytes` existed but could only be counted and compared: `@udp.recv_from`
// and `@fs.read_bytes` handed one back and nothing could look inside it or build
// one. A program could relay bytes and not author them, which is the difference
// between echoing a datagram and writing a DNS query.
//
// `@crypto.hex_decode` looks like the way in and is not — it answers a *string*
// and rejects anything that is not valid UTF-8, which most binary is. These are
// the byte-oriented pair, and they close the gap for `@udp`, `@fs.read_bytes` and
// `@http` bodies at once rather than three times.

fn bytes_arg(v: Option<&Value>, what: &str) -> Result<std::sync::Arc<[u8]>, EvalError> {
    match v {
        Some(Value::Bytes(b)) => Ok(b.clone()),
        // A string is bytes with an encoding; taking its UTF-8 is the obvious reading.
        Some(Value::String(s)) => Ok(s.as_bytes().into()),
        Some(other) => Err(EvalError::Message(format!(
            "{what} expects bytes, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message(format!("{what} expects bytes"))),
    }
}

/// `bytes(x)` — from a list of 0..=255, from a string's UTF-8, or bytes unchanged.
fn builtin_bytes(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::Bytes(b)) => Ok(Value::Bytes(b)),
        Some(Value::String(s)) => Ok(Value::Bytes(s.as_bytes().into())),
        Some(Value::List(xs)) => {
            let mut out = Vec::with_capacity(xs.len());
            for (i, v) in xs.iter().enumerate() {
                let n = v.as_int().ok_or_else(|| {
                    EvalError::Message(format!(
                        "bytes expects whole numbers, but element {i} is {}",
                        v.type_name()
                    ))
                })?;
                // Refuse rather than truncate: a silently wrapped 0x1ff is a packet
                // that goes out wrong and is debugged at the far end.
                if !(0..=255).contains(&n) {
                    return Err(EvalError::Message(format!(
                        "bytes expects 0 to 255, but element {i} is {n}"
                    )));
                }
                out.push(n as u8);
            }
            Ok(Value::Bytes(out.into()))
        }
        Some(other) => Err(EvalError::Message(format!(
            "bytes expects a list, string or bytes, got {}",
            other.type_name()
        ))),
        None => Ok(Value::Bytes(Vec::new().into())),
    }
}

/// `from_hex(s)` — a Result, because the input is usually untrusted.
///
/// Unlike `@crypto.hex_decode` this places no constraint on what the bytes mean,
/// so `from_hex("ff")` is a byte rather than an error.
fn builtin_from_hex(args: Vec<Value>) -> Result<Value, EvalError> {
    let s = str_arg(args.into_iter().next(), "from_hex")?;
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !cleaned.len().is_multiple_of(2) {
        return Ok(Value::err(Value::string(format!(
            "hex needs an even number of digits, got {}",
            cleaned.len()
        ))));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let chars: Vec<char> = cleaned.chars().collect();
    for pair in chars.chunks(2) {
        let hi = pair[0].to_digit(16);
        let lo = pair[1].to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
            _ => {
                return Ok(Value::err(Value::string(format!(
                    "`{}{}` is not a hex byte",
                    pair[0], pair[1]
                ))))
            }
        }
    }
    Ok(Value::ok(Value::Bytes(out.into())))
}

fn builtin_to_hex(args: Vec<Value>) -> Result<Value, EvalError> {
    let b = bytes_arg(args.first(), "to_hex")?;
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b.iter() {
        s.push_str(&format!("{byte:02x}"));
    }
    Ok(Value::string(s))
}

/// `to_text(b)` — a Result: arbitrary bytes are not always text, and pretending
/// otherwise is how replacement characters end up in a database.
fn builtin_to_text(args: Vec<Value>) -> Result<Value, EvalError> {
    let b = bytes_arg(args.first(), "to_text")?;
    Ok(match std::str::from_utf8(&b) {
        Ok(s) => Value::ok(Value::string(s)),
        Err(e) => Value::err(Value::string(format!(
            "bytes are not valid UTF-8 at offset {}",
            e.valid_up_to()
        ))),
    })
}

/// `byte_at(b, i)` — the byte as an int, or `none` past the end. Negative counts
/// from the end, matching `slice`.
fn builtin_byte_at(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let first = it.next();
    let b = bytes_arg(first.as_ref(), "byte_at")?;
    let i = it
        .next()
        .and_then(|v| v.as_int())
        .ok_or_else(|| EvalError::Message("byte_at expects an index".into()))?;
    let len = b.len() as i64;
    let idx = if i < 0 { len + i } else { i };
    Ok(match (0..len).contains(&idx) {
        true => Value::Int(b[idx as usize] as i64),
        false => Value::None,
    })
}
