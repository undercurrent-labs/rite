//! Reader for `grammar/cant/operators.toml`, the one place a Cant operator's
//! spelling is written down.
//!
//! The file is embedded with `include_str!` and parsed once. Nothing else in the
//! tree hard-codes an operator spelling, and
//! `crates/cant-syntax/tests/manifest_sync.rs` fails if the manifest and
//! [`CantTokenKind`](crate::token::CantTokenKind) disagree in either direction.
//!
//! # Why a hand-written reader
//!
//! The manifest uses a small, fixed subset of TOML — top-level `key = "value"`
//! pairs and repeated `[[operator]]` tables — and the workspace has no `toml`
//! dependency. This repository already reads `grammar/keywords.toml` by hand in
//! `crates/rite-cli/tests/editor_grammar_sync.rs`, so the precedent is set. The
//! file stays valid TOML and any TOML tool can read it; what is *not* supported
//! here (arrays, inline tables, multi-line strings, dotted keys, numbers) is
//! rejected with a message naming the line rather than quietly misread.

use std::sync::OnceLock;

/// The manifest source, embedded at compile time so a released binary carries
/// the same operator table the tests checked.
const MANIFEST_SOURCE: &str = include_str!("../../../grammar/cant/operators.toml");

/// One structural operator: its concept, its spellings, and the token both
/// spellings normalize to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorSpec {
    /// Stable name for the operator, used in docs and diagnostics.
    pub concept: String,
    /// Name of the `CantTokenKind` variant both spellings produce.
    pub token: String,
    /// The canonical spelling. Always ASCII — Cant must be typeable.
    pub ascii: String,
    /// The optional glyph spelling. Presentation only.
    pub glyph: Option<String>,
    /// The ASCII spelling also means something else inside a leaf expression, so
    /// the parser resolves it from position. See `docs/cant/internals.md`.
    pub ambiguous: bool,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorManifest {
    pub version: String,
    pub language_version: String,
    pub operators: Vec<OperatorSpec>,
}

impl OperatorManifest {
    pub fn by_token(&self, token: &str) -> Option<&OperatorSpec> {
        self.operators.iter().find(|o| o.token == token)
    }

    pub fn by_concept(&self, concept: &str) -> Option<&OperatorSpec> {
        self.operators.iter().find(|o| o.concept == concept)
    }
}

/// The embedded manifest, parsed once.
///
/// Panics if the embedded file is malformed. That is a broken build, not bad
/// user input: the file ships inside the binary and is covered by
/// `manifest_sync`. Nothing a `.cant` source can contain reaches this.
pub fn manifest() -> &'static OperatorManifest {
    static CELL: OnceLock<OperatorManifest> = OnceLock::new();
    CELL.get_or_init(|| {
        parse_manifest(MANIFEST_SOURCE)
            .unwrap_or_else(|e| panic!("grammar/cant/operators.toml is malformed: {e}"))
    })
}

/// Parse the restricted TOML subset the manifest uses.
///
/// Public so the sync test can point it at the file on disk and prove the
/// embedded copy is the same one.
pub fn parse_manifest(source: &str) -> Result<OperatorManifest, String> {
    let mut version = None;
    let mut language_version = None;
    let mut operators: Vec<OperatorSpec> = Vec::new();
    // `None` while reading top-level keys, `Some(index)` once inside an
    // `[[operator]]` table.
    let mut current: Option<PartialOperator> = None;

    for (n, raw) in source.lines().enumerate() {
        let line_no = n + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[operator]]" {
            if let Some(p) = current.take() {
                operators.push(p.finish(line_no)?);
            }
            current = Some(PartialOperator::default());
            continue;
        }
        if line.starts_with('[') {
            return Err(format!("line {line_no}: unsupported table header `{line}`"));
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {line_no}: expected `key = value`, found `{line}`"))?;
        let key = key.trim();
        let value =
            parse_value(value.trim()).map_err(|e| format!("line {line_no}: {e} (key `{key}`)"))?;

        match &mut current {
            None => match (key, value) {
                ("version", Value::Str(s)) => version = Some(s),
                ("language_version", Value::Str(s)) => language_version = Some(s),
                _ => return Err(format!("line {line_no}: unknown top-level key `{key}`")),
            },
            Some(op) => op.set(key, value, line_no)?,
        }
    }
    if let Some(p) = current.take() {
        operators.push(p.finish(source.lines().count())?);
    }

    if operators.is_empty() {
        return Err("no [[operator]] entries".to_string());
    }
    Ok(OperatorManifest {
        version: version.ok_or("missing top-level `version`")?,
        language_version: language_version.ok_or("missing top-level `language_version`")?,
        operators,
    })
}

enum Value {
    Str(String),
    Bool(bool),
}

#[derive(Default)]
struct PartialOperator {
    concept: Option<String>,
    token: Option<String>,
    ascii: Option<String>,
    glyph: Option<String>,
    ambiguous: bool,
    description: Option<String>,
}

impl PartialOperator {
    fn set(&mut self, key: &str, value: Value, line_no: usize) -> Result<(), String> {
        let dup = |field: &Option<String>, key: &str| {
            if field.is_some() {
                Err(format!("line {line_no}: duplicate key `{key}`"))
            } else {
                Ok(())
            }
        };
        match (key, value) {
            ("concept", Value::Str(s)) => {
                dup(&self.concept, key)?;
                self.concept = Some(s);
            }
            ("token", Value::Str(s)) => {
                dup(&self.token, key)?;
                self.token = Some(s);
            }
            ("ascii", Value::Str(s)) => {
                dup(&self.ascii, key)?;
                self.ascii = Some(s);
            }
            ("glyph", Value::Str(s)) => {
                dup(&self.glyph, key)?;
                self.glyph = Some(s);
            }
            ("description", Value::Str(s)) => {
                dup(&self.description, key)?;
                self.description = Some(s);
            }
            ("ambiguous", Value::Bool(b)) => self.ambiguous = b,
            _ => return Err(format!("line {line_no}: unknown operator key `{key}`")),
        }
        Ok(())
    }

    fn finish(self, line_no: usize) -> Result<OperatorSpec, String> {
        let need = |field: Option<String>, name: &str| {
            field.ok_or_else(|| format!("operator ending at line {line_no}: missing `{name}`"))
        };
        let ascii = need(self.ascii, "ascii")?;
        if ascii.is_empty() || !ascii.is_ascii() {
            return Err(format!(
                "operator ending at line {line_no}: `ascii` must be non-empty ASCII, got {ascii:?}"
            ));
        }
        Ok(OperatorSpec {
            concept: need(self.concept, "concept")?,
            token: need(self.token, "token")?,
            ascii,
            glyph: self.glyph,
            ambiguous: self.ambiguous,
            description: need(self.description, "description")?,
        })
    }
}

/// Drop a trailing `#` comment, but not one inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_value(text: &str) -> Result<Value, String> {
    match text {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }
    let inner = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .ok_or_else(|| format!("expected a quoted string or a boolean, found `{text}`"))?;
    if inner.contains('"') {
        return Err(format!("string contains an unescaped quote: `{text}`"));
    }
    Ok(Value::Str(inner.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_manifest_parses() {
        let m = manifest();
        assert_eq!(m.version, "0");
        assert!(m.operators.len() >= 12, "{}", m.operators.len());
    }

    #[test]
    fn flow_has_both_spellings() {
        let flow = manifest().by_concept("flow").expect("flow operator");
        assert_eq!(flow.ascii, "->");
        assert_eq!(flow.glyph.as_deref(), Some("→"));
        assert_eq!(flow.token, "Flow");
    }

    #[test]
    fn every_ascii_spelling_is_typeable() {
        for op in &manifest().operators {
            assert!(
                op.ascii.is_ascii() && !op.ascii.is_empty(),
                "{} is not ASCII-typeable: {:?}",
                op.concept,
                op.ascii
            );
        }
    }

    #[test]
    fn a_comment_after_a_value_is_dropped_but_a_hash_in_a_string_is_kept() {
        assert_eq!(
            strip_comment("ascii = \"->\"  # flow").trim(),
            "ascii = \"->\""
        );
        assert_eq!(strip_comment("glyph = \"#\"").trim(), "glyph = \"#\"");
    }

    #[test]
    fn unsupported_toml_is_rejected_rather_than_misread() {
        for bad in [
            "version = \"0\"\nlanguage_version = \"0\"\n[[operator]]\nascii = [\"->\"]\n",
            "version = \"0\"\nlanguage_version = \"0\"\n[operator]\n",
            "version = \"0\"\nlanguage_version = \"0\"\n[[operator]]\nascii = \"->\"\n",
        ] {
            assert!(parse_manifest(bad).is_err(), "should reject: {bad:?}");
        }
    }

    #[test]
    fn a_non_ascii_canonical_spelling_is_rejected() {
        let bad = concat!(
            "version = \"0\"\nlanguage_version = \"0\"\n[[operator]]\n",
            "concept = \"flow\"\ntoken = \"Flow\"\nascii = \"→\"\ndescription = \"d\"\n"
        );
        let err = parse_manifest(bad).unwrap_err();
        assert!(err.contains("ASCII"), "{err}");
    }
}
