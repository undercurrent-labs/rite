//! wasm-bindgen surface for hosted Studio.

use crate::{
    analyze, complete, convert, emit_rust, format, hover, map_position, parse, run_blocking,
    semantic_ir, syntax_tree, ExecutionResult, FormatResultDto, ParseResult, RunOptions,
    SourceMapDto,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// JSON-compatible objects (no Map) so `JSON.stringify` in the SPA works.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn wasm_parse(source: &str) -> Result<JsValue, JsValue> {
    to_js(&parse(source))
}

#[wasm_bindgen]
pub fn wasm_analyze(source: &str) -> Result<JsValue, JsValue> {
    to_js(&analyze(source))
}

#[wasm_bindgen]
pub fn wasm_format(source: &str, dialect: &str) -> Result<JsValue, JsValue> {
    match format(source, dialect) {
        Ok(r) => to_js(&r),
        Err(e) => Err(JsValue::from_str(&e)),
    }
}

#[wasm_bindgen]
pub fn wasm_convert(source: &str, dialect: &str) -> Result<JsValue, JsValue> {
    match convert(source, dialect) {
        Ok(r) => to_js(&r),
        Err(e) => Err(JsValue::from_str(&e)),
    }
}

#[wasm_bindgen]
pub fn wasm_run(source: &str, allow_all: bool) -> Result<JsValue, JsValue> {
    let r = run_blocking(
        source,
        RunOptions {
            allow_all,
            browser_safe: true,
            timeout_ms: Some(3000),
            seed: Some(42),
            files: Default::default(),
        },
    );
    to_js(&r)
}

#[wasm_bindgen]
pub fn wasm_emit_rust(source: &str) -> Result<JsValue, JsValue> {
    to_js(&emit_rust(source))
}

#[wasm_bindgen]
pub fn wasm_syntax_tree(source: &str) -> Result<JsValue, JsValue> {
    to_js(&syntax_tree(source))
}

#[wasm_bindgen]
pub fn wasm_semantic_ir(source: &str) -> Result<JsValue, JsValue> {
    to_js(&semantic_ir(source))
}

#[wasm_bindgen]
pub fn wasm_complete(source: &str, line: u32, character: u32) -> Result<JsValue, JsValue> {
    to_js(&complete(source, line, character))
}

#[wasm_bindgen]
pub fn wasm_hover(source: &str, line: u32, character: u32) -> Result<JsValue, JsValue> {
    to_js(&hover(source, line, character))
}

#[wasm_bindgen]
pub fn wasm_map_position(
    map_json: &str,
    old_source: &str,
    new_source: &str,
    line: u32,
    character: u32,
) -> Result<JsValue, JsValue> {
    let map: SourceMapDto =
        serde_json::from_str(map_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let (l, c) = map_position(&map, old_source, new_source, line, character);
    to_js(&serde_json::json!({"line": l, "character": c}))
}

// Re-export types for TypeScript consumers via serde
#[allow(dead_code)]
fn _types(_: ParseResult, _: FormatResultDto, _: ExecutionResult) {}
