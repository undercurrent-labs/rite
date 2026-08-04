//! wasm-bindgen surface for Cant Studio.
//!
//! Thin by design: every function here forwards to the one above it in `lib.rs`,
//! which is what the tests exercise. A binding with logic in it would be a
//! behaviour only a browser could check.

use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// JSON-compatible objects (no `Map`), so `JSON.stringify` works in the page.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn cant_check(source: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::check(source))
}

#[wasm_bindgen]
pub fn cant_expand(source: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::expand(source))
}

#[wasm_bindgen]
pub fn cant_graph(source: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::graph(source))
}

#[wasm_bindgen]
pub fn cant_dot(source: &str) -> String {
    crate::dot(source)
}

#[wasm_bindgen]
pub fn cant_explain(source: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::explain(source))
}

#[wasm_bindgen]
pub fn cant_format(source: &str, dialect: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::format(source, dialect))
}

#[wasm_bindgen]
pub fn cant_convert(source: &str, dialect: &str) -> String {
    crate::convert(source, dialect)
}

#[wasm_bindgen]
pub fn cant_run(source: &str) -> Result<JsValue, JsValue> {
    to_js(&crate::run(source))
}

#[wasm_bindgen]
pub fn cant_version() -> Result<JsValue, JsValue> {
    to_js(&crate::version())
}
