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
        "flatten" => builtin_flatten(args),
        "sum" => builtin_sum(args),
        "min" => builtin_min_max(args, true),
        "max" => builtin_min_max(args, false),
        "sort" => builtin_sort(args),
        "unique" => builtin_unique(args),
        "range" => builtin_range(args),
        "lines" => builtin_lines(args),
        "zip" => builtin_zip(args),
        "chunk" => builtin_chunk(args),
        "collect_results" => builtin_collect_results(args),
        "require" => builtin_require(args),
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
    let mut xs = im::Vector::new();
    let mut i = start;
    while i < end {
        xs.push_back(Value::Int(i));
        i += 1;
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

pub fn merge_records(left: &IndexMap<Key, Value>, right: &IndexMap<Key, Value>) -> IndexMap<Key, Value> {
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
        Value::String(s) => item
            .as_str()
            .map(|sub| s.contains(sub))
            .unwrap_or(false),
        _ => false,
    }
}
