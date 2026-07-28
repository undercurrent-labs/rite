//! Pure standard-library builtins.

use crate::value::{Key, ResultValue, Value};
use crate::EvalError;
use indexmap::IndexMap;

pub fn call_builtin(name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    match name {
        "ok" => Ok(Value::ok(args.into_iter().next().unwrap_or(Value::None))),
        "err" => Ok(Value::err(args.into_iter().next().unwrap_or(Value::None))),
        "str" => Ok(Value::string(format!(
            "{}",
            args.first().unwrap_or(&Value::None)
        ))),
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
        "sort" => builtin_sort(args),
        "unique" => builtin_unique(args),
        "range" => builtin_range(args),
        "range_incl" => builtin_range_incl(args),
        "lines" => builtin_lines(args),
        "words" => builtin_words(args),
        "join" => builtin_join(args),
        "zip" => builtin_zip(args),
        "chunk" => builtin_chunk(args),
        "keys" => builtin_keys(args),
        "values" => builtin_values(args),
        "abs" => builtin_abs(args),
        "clamp" => builtin_clamp(args),
        "pow" => builtin_pow(args),
        "idiv" => builtin_idiv(args),
        "xor" => builtin_xor(args),
        "and_then" => builtin_and_then(args),
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
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| "panic".into()),
        )),
        "expect" => builtin_expect(args),
        "fail" => Err(EvalError::Message(
            args.first()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "test failed".into()),
        )),
        // Higher-order functions handled in evaluator with closures
        "map" | "keep" | "reject" | "reduce" | "each" | "find" | "any" | "all" | "group"
        | "parallel" => Err(EvalError::Message(format!(
            "builtin `{}` requires evaluator dispatch",
            name
        ))),
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

fn builtin_first(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) => Ok(xs.front().cloned().unwrap_or(Value::None)),
        _ => Ok(Value::None),
    }
}

fn builtin_last(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) => Ok(xs.back().cloned().unwrap_or(Value::None)),
        _ => Ok(Value::None),
    }
}

fn builtin_rest(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) if !xs.is_empty() => {
            let mut out = xs;
            out.pop_front();
            Ok(Value::List(out))
        }
        Some(Value::List(_)) => Ok(Value::list(Vec::<Value>::new())),
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
}

fn builtin_init(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) if !xs.is_empty() => {
            let mut out = xs;
            out.pop_back();
            Ok(Value::List(out))
        }
        Some(Value::List(_)) => Ok(Value::list(Vec::<Value>::new())),
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
}

fn builtin_take(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let list = match it.next() {
        Some(Value::List(xs)) => xs,
        _ => return Ok(Value::list(Vec::<Value>::new())),
    };
    // pipeline: xs → take(n) passes list first then n from stage args
    let n = it.next().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
    Ok(Value::list(list.into_iter().take(n).collect::<Vec<_>>()))
}

fn builtin_drop(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let list = match it.next() {
        Some(Value::List(xs)) => xs,
        _ => return Ok(Value::list(Vec::<Value>::new())),
    };
    let n = it.next().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
    Ok(Value::list(list.into_iter().skip(n).collect::<Vec<_>>()))
}

fn builtin_reverse(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) => Ok(Value::list(xs.into_iter().rev().collect::<Vec<_>>())),
        Some(Value::String(s)) => Ok(Value::string(s.chars().rev().collect::<String>())),
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
}

fn builtin_concat(args: Vec<Value>) -> Result<Value, EvalError> {
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
    let mut out = im::Vector::new();
    if let Some(Value::List(xs)) = args.into_iter().next() {
        for x in xs {
            match x {
                Value::List(inner) => out.extend(inner),
                other => out.push_back(other),
            }
        }
    }
    Ok(Value::List(out))
}

fn builtin_sum(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut sum_i: i64 = 0;
    let mut sum_f: f64 = 0.0;
    let mut used_float = false;
    if let Some(Value::List(xs)) = args.into_iter().next() {
        for x in xs {
            match x {
                Value::Int(n) => {
                    sum_i = sum_i
                        .checked_add(n)
                        .ok_or_else(|| EvalError::Message("integer overflow in sum".into()))?;
                }
                Value::Float(f) => {
                    used_float = true;
                    sum_f += f;
                }
                _ => {}
            }
        }
    }
    if used_float {
        Ok(Value::Float(sum_f + sum_i as f64))
    } else {
        Ok(Value::Int(sum_i))
    }
}

fn builtin_min_max(args: Vec<Value>, is_min: bool) -> Result<Value, EvalError> {
    let Some(Value::List(xs)) = args.into_iter().next() else {
        return Ok(Value::None);
    };
    let mut best: Option<Value> = None;
    for x in xs {
        match &best {
            None => best = Some(x),
            Some(b) => {
                let cmp = compare_values(&x, b);
                if is_min && cmp < 0 || !is_min && cmp > 0 {
                    best = Some(x);
                }
            }
        }
    }
    Ok(best.unwrap_or(Value::None))
}

fn builtin_sort(args: Vec<Value>) -> Result<Value, EvalError> {
    let Some(Value::List(xs)) = args.into_iter().next() else {
        return Ok(Value::list(Vec::<Value>::new()));
    };
    let mut v: Vec<Value> = xs.into_iter().collect();
    v.sort_by(|a, b| compare_values(a, b).cmp(&0));
    Ok(Value::list(v))
}

fn builtin_unique(args: Vec<Value>) -> Result<Value, EvalError> {
    let Some(Value::List(xs)) = args.into_iter().next() else {
        return Ok(Value::list(Vec::<Value>::new()));
    };
    let mut out = Vec::new();
    for x in xs {
        if !out.iter().any(|y: &Value| y.structural_eq(&x)) {
            out.push(x);
        }
    }
    Ok(Value::list(out))
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
    if step > 0 {
        while i < end {
            xs.push_back(Value::Int(i));
            i += step;
        }
    } else {
        while i > end {
            xs.push_back(Value::Int(i));
            i += step;
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
    if step > 0 {
        while i <= end {
            xs.push_back(Value::Int(i));
            i += step;
        }
    } else {
        while i >= end {
            xs.push_back(Value::Int(i));
            i += step;
        }
    }
    Ok(Value::List(xs))
}

fn builtin_lines(args: Vec<Value>) -> Result<Value, EvalError> {
    let s = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let lines: Vec<Value> = s.lines().map(Value::string).collect();
    Ok(Value::list(lines))
}

fn builtin_words(args: Vec<Value>) -> Result<Value, EvalError> {
    let s = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let words: Vec<Value> = s.split_whitespace().map(Value::string).collect();
    Ok(Value::list(words))
}

fn builtin_join(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let list = match it.next() {
        Some(Value::List(xs)) => xs,
        Some(Value::String(s)) => return Ok(Value::String(s)),
        _ => return Ok(Value::string("")),
    };
    let sep = it
        .next()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "".into());
    let parts: Vec<String> = list.iter().map(|v| format!("{}", v)).collect();
    Ok(Value::string(parts.join(&sep)))
}

fn builtin_keys(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::Record(r)) => Ok(Value::list(
            r.keys()
                .map(|k| Value::string(k.as_str()))
                .collect::<Vec<_>>(),
        )),
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
}

fn builtin_values(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::Record(r)) => Ok(Value::list(r.values().cloned().collect::<Vec<_>>())),
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
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
        (Value::Int(a), Value::Int(b)) if *b >= 0 && *b <= 32 => {
            Ok(Value::Int(a.pow(*b as u32)))
        }
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
    Ok(Value::Int(a / b))
}

fn builtin_xor(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let a = it.next().map(|v| v.is_truthy()).unwrap_or(false);
    let b = it.next().map(|v| v.is_truthy()).unwrap_or(false);
    Ok(Value::Bool(a ^ b))
}

fn builtin_and_then(args: Vec<Value>) -> Result<Value, EvalError> {
    // and_then(result, value_if_we_could_call) — without HO, pass through ok/err
    // Prefer pipeline with function in evaluator; pure form: if ok return ok else err
    match args.into_iter().next() {
        Some(Value::Result(ResultValue::Ok(v))) => Ok(Value::ok(*v)),
        Some(Value::Result(ResultValue::Err(e))) => Ok(Value::err(*e)),
        other => Ok(other.unwrap_or(Value::None)),
    }
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
    match val {
        Value::String(s) => Ok(Value::string(s.repeat(n))),
        Value::List(xs) => {
            let mut out = im::Vector::new();
            for _ in 0..n {
                out.extend(xs.clone());
            }
            Ok(Value::List(out))
        }
        other => Ok(Value::list(std::iter::repeat(other).take(n).collect::<Vec<_>>())),
    }
}

fn builtin_contains(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let container = it.next().unwrap_or(Value::None);
    let item = it.next().unwrap_or(Value::None);
    Ok(Value::Bool(membership(&item, &container)))
}

fn builtin_enumerate(args: Vec<Value>) -> Result<Value, EvalError> {
    match args.into_iter().next() {
        Some(Value::List(xs)) => {
            let out: Vec<Value> = xs
                .into_iter()
                .enumerate()
                .map(|(i, x)| Value::list(vec![Value::Int(i as i64), x]))
                .collect();
            Ok(Value::list(out))
        }
        _ => Ok(Value::list(Vec::<Value>::new())),
    }
}

fn builtin_zip(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let a = match it.next() {
        Some(Value::List(xs)) => xs,
        _ => return Ok(Value::list(Vec::<Value>::new())),
    };
    let b = match it.next() {
        Some(Value::List(xs)) => xs,
        _ => return Ok(Value::list(Vec::<Value>::new())),
    };
    let out: Vec<Value> = a
        .into_iter()
        .zip(b)
        .map(|(x, y)| Value::list(vec![x, y]))
        .collect();
    Ok(Value::list(out))
}

fn builtin_chunk(args: Vec<Value>) -> Result<Value, EvalError> {
    let mut it = args.into_iter();
    let list = match it.next() {
        Some(Value::List(xs)) => xs,
        _ => return Ok(Value::list(Vec::<Value>::new())),
    };
    let size = it.next().and_then(|v| v.as_int()).unwrap_or(1).max(1) as usize;
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for x in list {
        cur.push(x);
        if cur.len() == size {
            out.push(Value::list(std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        out.push(Value::list(cur));
    }
    Ok(Value::list(out))
}

fn builtin_collect_results(args: Vec<Value>) -> Result<Value, EvalError> {
    let Some(Value::List(xs)) = args.into_iter().next() else {
        return Ok(Value::ok(Value::list(Vec::<Value>::new())));
    };
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

pub fn compare_values(a: &Value, b: &Value) -> i32 {
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.as_ref().cmp(y.as_ref()),
        _ => Ordering::Equal,
    };
    match ord {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
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
