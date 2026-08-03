//! JSON Schema derived from Rite's declared types.
//!
//! An `@mcp` tool publishes an `inputSchema` so a model host knows how to call it. That
//! schema is not a second, hand-written description of the tool — it is a projection of
//! the annotations the author already wrote:
//!
//! ```text
//! tool "add" |a: int, b: [string]| ⟦ … ⟧
//!   → {"type":"object",
//!      "properties":{"a":{"type":"integer"},
//!                    "b":{"type":"array","items":{"type":"string"}}},
//!      "required":["a","b"]}
//! ```
//!
//! This lives next to [`crate::ops::value_matches_type`] on purpose. "What does this
//! type accept?" and "how is this type advertised?" are two views of one answer, and
//! keeping them in separate crates is how they drift.
//!
//! Two rules govern every entry in the table below, and they are the whole policy:
//!
//! 1. **Never emit a schema stricter than [`crate::ops::value_matches_type`] accepts.**
//!    A schema that rejects a value the runtime would have taken turns a working call
//!    into a client-side error the server never sees. Where a type has no faithful
//!    JSON Schema, emit the empty schema `{}` — which permits anything — rather than
//!    inventing a constraint. The contract check still rejects the value, and the
//!    caller gets the runtime's own message.
//! 2. **The schema is advisory; the contract is normative.** Clients may ignore a
//!    schema entirely, so validation is never left to it. Arguments are checked by the
//!    same `FnContract` machinery that checks an ordinary typed Rite call.

use rite_sem::TypeExpr;
use serde_json::{json, Map, Value as Json};

/// The JSON Schema for one declared type. `None` — an unannotated parameter — is the
/// empty schema, which accepts any JSON value.
pub fn json_schema_for_type(ty: Option<&TypeExpr>) -> Json {
    let Some(ty) = ty else {
        return json!({});
    };
    match ty {
        TypeExpr::Any(_) => json!({}),
        TypeExpr::List(inner) => json!({
            "type": "array",
            "items": json_schema_for_type(Some(inner)),
        }),
        // `Value::to_json` encodes a result as `{"ok": …}` or `{"err": …}`, and
        // `value_matches_type` leaves the `err` payload unconstrained because it
        // carries a failure rather than a `T`. Both facts are mirrored here.
        TypeExpr::Result(inner) => json!({
            "oneOf": [
                {"type": "object",
                 "properties": {"ok": json_schema_for_type(Some(inner))},
                 "required": ["ok"]},
                {"type": "object",
                 "properties": {"err": {}},
                 "required": ["err"]},
            ],
        }),
        // Every declared field is required, because `value_matches_type` demands each
        // one be present. Undeclared fields are permitted, because it ignores them.
        TypeExpr::Record(fields) => {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for (name, fty) in fields {
                properties.insert(name.name.clone(), json_schema_for_type(Some(fty)));
                required.push(Json::String(name.name.clone()));
            }
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": true,
            })
        }
        TypeExpr::Named(name) => named_schema(&name.name),
    }
}

/// The named types, matched against the spellings [`crate::value::Value::type_name`]
/// produces — those are what `value_matches_type` compares by string equality.
fn named_schema(name: &str) -> Json {
    match name {
        "int" => json!({"type": "integer"}),
        "float" => json!({"type": "number"}),
        // `number` accepts either numeric type, matching `value_matches_type`.
        "number" => json!({"type": "number"}),
        "string" => json!({"type": "string"}),
        "bool" => json!({"type": "boolean"}),
        "none" => json!({"type": "null"}),
        "list" => json!({"type": "array"}),
        "record" => json!({"type": "object"}),
        // An atom crosses the host boundary as its name — see `Value::to_json` — so a
        // string is the honest advertisement, not an enum we cannot enumerate.
        "atom" => json!({"type": "string"}),
        "bytes" => json!({"type": "string", "contentEncoding": "base64"}),
        "any" => json!({}),
        // A type this function does not know — including `function`, `handle`, and any
        // future named type. Rule 1: say nothing rather than something false.
        _ => json!({}),
    }
}

/// The `inputSchema` object for a declared parameter list.
///
/// Every parameter is `required`, annotated or not: arity is checked before types are,
/// so an unannotated parameter is still a slot that must be filled. `{}` says "any
/// value"; `required` says "you must send one".
pub fn json_schema_for_params<'a>(
    params: impl IntoIterator<Item = (&'a str, Option<&'a TypeExpr>)>,
) -> Json {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, ty) in params {
        properties.insert(name.to_string(), json_schema_for_type(ty));
        required.push(Json::String(name.to_string()));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// The same schema, derived from a function value's declared contract.
///
/// This is what `@mcp.tool_schema(f)` answers, so a script can see exactly what it
/// would publish before it publishes it.
pub fn json_schema_for_contract(contract: &crate::value::FnContract) -> Json {
    json_schema_for_params(contract.param_names.iter().enumerate().map(|(i, name)| {
        (
            name.as_str(),
            contract.param_types.get(i).and_then(|t| t.as_ref()),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_sem::Ident;

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.into(),
            span: rite_core::Span::from_range(0, 0),
        }
    }
    fn named(name: &str) -> TypeExpr {
        TypeExpr::Named(ident(name))
    }

    #[test]
    fn scalars_map_to_their_json_types() {
        for (rite, json_ty) in [
            ("int", "integer"),
            ("float", "number"),
            ("number", "number"),
            ("string", "string"),
            ("bool", "boolean"),
            ("none", "null"),
            ("list", "array"),
            ("record", "object"),
            // An atom is advertised as the string it becomes on the wire.
            ("atom", "string"),
        ] {
            assert_eq!(
                json_schema_for_type(Some(&named(rite))),
                json!({"type": json_ty}),
                "{rite}"
            );
        }
    }

    #[test]
    fn an_unannotated_parameter_is_the_empty_schema() {
        assert_eq!(json_schema_for_type(None), json!({}));
    }

    /// Rule 1: a type with no faithful schema says nothing rather than something false.
    #[test]
    fn an_unknown_named_type_is_the_empty_schema() {
        assert_eq!(json_schema_for_type(Some(&named("function"))), json!({}));
        assert_eq!(json_schema_for_type(Some(&named("Widget"))), json!({}));
        assert_eq!(
            json_schema_for_type(Some(&TypeExpr::Any(rite_core::Span::from_range(0, 0)))),
            json!({})
        );
        assert_eq!(json_schema_for_type(Some(&named("any"))), json!({}));
    }

    #[test]
    fn a_list_carries_its_element_schema() {
        let ty = TypeExpr::List(Box::new(named("string")));
        assert_eq!(
            json_schema_for_type(Some(&ty)),
            json!({"type": "array", "items": {"type": "string"}})
        );
    }

    #[test]
    fn a_record_requires_every_declared_field_and_permits_others() {
        let ty = TypeExpr::Record(vec![
            (ident("name"), named("string")),
            (ident("age"), named("int")),
        ]);
        assert_eq!(
            json_schema_for_type(Some(&ty)),
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}, "age": {"type": "integer"}},
                "required": ["name", "age"],
                "additionalProperties": true,
            })
        );
    }

    /// The encoding matches `Value::to_json`, and the `err` payload is unconstrained
    /// exactly as `value_matches_type` leaves it.
    #[test]
    fn a_result_mirrors_the_wire_encoding() {
        let ty = TypeExpr::Result(Box::new(named("int")));
        assert_eq!(
            json_schema_for_type(Some(&ty)),
            json!({
                "oneOf": [
                    {"type": "object", "properties": {"ok": {"type": "integer"}}, "required": ["ok"]},
                    {"type": "object", "properties": {"err": {}}, "required": ["err"]},
                ]
            })
        );
    }

    #[test]
    fn nesting_recurses_all_the_way_down() {
        let inner = TypeExpr::Record(vec![(
            ident("tags"),
            TypeExpr::List(Box::new(named("string"))),
        )]);
        let ty = TypeExpr::List(Box::new(inner));
        assert_eq!(
            json_schema_for_type(Some(&ty)),
            json!({
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {"tags": {"type": "array", "items": {"type": "string"}}},
                    "required": ["tags"],
                    "additionalProperties": true,
                }
            })
        );
    }

    #[test]
    fn every_parameter_is_required_annotated_or_not() {
        let int = named("int");
        let schema = json_schema_for_params([("a", Some(&int)), ("b", None)]);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {"a": {"type": "integer"}, "b": {}},
                "required": ["a", "b"],
            })
        );
    }

    #[test]
    fn parameters_keep_declaration_order() {
        let int = named("int");
        let schema = json_schema_for_params([("z", Some(&int)), ("a", Some(&int))]);
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required[0], "z");
        assert_eq!(required[1], "a");
    }

    /// Rule 1, stated as a property: the schema never rejects what the runtime accepts.
    #[test]
    fn no_schema_is_stricter_than_the_contract_check() {
        // `number` accepts an int, so its schema must not say "integer".
        assert_eq!(
            json_schema_for_type(Some(&named("number"))),
            json!({"type": "number"}),
            "`number` accepts Int and Float; advertising `integer` would reject a float"
        );
    }
}
