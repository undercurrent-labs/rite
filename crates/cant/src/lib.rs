//! Public API for Cant.
//!
//! Small on purpose. Cant reuses Rite's source, span, diagnostic, permission and
//! execution types wherever they are stable, and wraps rather than duplicates
//! them where Cant needs to add something of its own.
//!
//! # Features
//!
//! `native` (default) adds [`run`] and [`build`], which need Rite's runtime,
//! capabilities and compiler. Without it the crate stops at [`expand`] — the
//! whole of Cant that needs no host, and exactly what a browser build can use.
//! The functions are absent rather than present and failing, so a caller cannot
//! bind to something that cannot work where it is being called.
//!
//! # Boundaries
//!
//! Nothing in `rite-*` depends on this crate, and nothing here adds a construct
//! to Rite's grammar, IR, dialect enum, or capability namespace. Cant executes
//! by generating canonical ASCII Rite and passing it through Rite's ordinary
//! front end — `docs/adr/0002-cant-lowers-through-rite.md`.

#[cfg(feature = "native")]
pub mod run;

pub use cant_sem as sem;
pub use cant_syntax as syntax;
pub use rite_core as core;

pub use cant_sem::GRAPH_SCHEMA_VERSION;
pub use cant_sem::{
    remap_diagnostic, to_dot, Analysis, CantProgram, Edge, EdgeRole, ExpandOptions, Expansion,
    LayoutHint, LeafExpr, Mapping, Node, NodeId, NodeKind, PortKind, PortRef, Subgraph, SubgraphId,
};
pub use cant_syntax::{
    convert, detect_dialect, format, CantDiagnostic, CantDiagnostics, CantProgramAst, Dialect,
    FormatError, FormatOptions, FormatResult, ParseResult, CANT_LANGUAGE_VERSION,
};

#[cfg(feature = "native")]
pub use run::{build, run, BuildOptions, BuildResult, ExecutionResult, RunOptions};

use rite_core::{FileId, SourceFile, SourceMap};

/// The `cant` tool version, from this crate's package version.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Rite version Cant was built against.
///
/// Read from `rite-core` rather than from this crate: Cant versions
/// independently, so its own number says nothing about the Rite on the other
/// side of an expansion — and that is the number someone debugging generated
/// code needs.
pub const RITE_VERSION: &str = rite_core::VERSION;

/// Parse a Cant source file.
pub fn parse(file: &SourceFile) -> ParseResult {
    cant_syntax::parse(file)
}

/// Parse a named source string, with a [`SourceMap`] for rendering diagnostics.
pub fn parse_source(name: &str, text: &str) -> (ParseResult, SourceMap) {
    cant_syntax::parse_source(name, text)
}

/// What analysis produced: the graph, and everything wrong with the program.
///
/// The graph is present even when there are errors. That is deliberate — a
/// diagnostic points *at* the graph, and `cant graph` on a broken program is
/// usually how someone works out what went wrong.
pub struct AnalyzeResult {
    pub parse: ParseResult,
    /// `None` only when there was nothing to parse.
    pub graph: Option<CantProgram>,
    pub diagnostics: CantDiagnostics,
    pub sources: SourceMap,
}

impl AnalyzeResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// The flows the program names, in source order.
    ///
    /// Read from the AST because the graph does not carry them: a definition is
    /// spliced into the flow that used it (ADR 0011).
    pub fn definition_names(&self) -> Vec<String> {
        self.parse
            .program
            .as_ref()
            .map(|ast| ast.defs.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Rendered diagnostics, ready for a terminal.
    pub fn render(&self) -> String {
        self.diagnostics.render_all(&self.sources)
    }
}

/// Parse, lower to the graph, and validate.
///
/// This is what `cant check` runs. Syntax diagnostics come first, then graph
/// ones, so [`CantDiagnostics::rejection_exit_code`] reports the earliest thing
/// that went wrong rather than the last.
pub fn analyze(name: &str, text: &str) -> AnalyzeResult {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, text);
    let file = sources.get(id).expect("just added").clone();
    let parse = cant_syntax::parse(&file);

    let mut diagnostics = parse.diagnostics.clone();
    let graph = parse.program.as_ref().map(|ast| {
        let analysis = cant_sem::analyze(ast, file.id, name, text.len());
        diagnostics.extend(analysis.diagnostics);
        analysis.graph
    });

    AnalyzeResult {
        parse,
        graph,
        diagnostics,
        sources,
    }
}

/// The graph for a source, or the diagnostics that stopped it being built.
pub fn graph(name: &str, text: &str) -> AnalyzeResult {
    analyze(name, text)
}

/// A full check: syntax, graph, **and** what Rite makes of the generated code.
///
/// The third step is the one that matters. Cant does not resolve names, check
/// arity, or enforce the effect discipline — Rite does, and the only way to ask
/// it is to hand it the program. A leaf like `[[1, 2], [3]]` parses as Cant and
/// is not valid Rite; without this step it would fail at run time, in generated
/// code, pointing at a line the user never wrote.
///
/// Diagnostics from Rite are remapped onto `.cant` spans, carrying the Rite code
/// and generated span as related metadata — §2.4.
pub struct CheckResult {
    pub analysis: AnalyzeResult,
    pub expansion: Option<Expansion>,
    /// Cant diagnostics, including everything remapped from Rite.
    pub diagnostics: CantDiagnostics,
}

impl CheckResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn render(&self) -> String {
        self.diagnostics.render_all(&self.analysis.sources)
    }

    pub fn exit_code(&self) -> u8 {
        self.diagnostics.rejection_exit_code()
    }
}

/// What a program is checked and run *in*.
///
/// Not part of the language: a Cant program is one flow, and none of this
/// changes what it means. These are the things a *host* supplies around it —
/// the REPL's session bindings, the modules a command line made available, and
/// where modules are looked for.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// Verbatim Rite lines emitted after the imports and before the generated
    /// functions — the REPL's session bindings (`x <- 5`).
    pub preamble: Vec<String>,
    /// Modules made available to every program without it saying `use`.
    ///
    /// Emitted as `use NAME` above the generated functions, *after* the
    /// program's own imports and skipping any it already declares: two `use`
    /// lines for one module is a Rite duplicate-binding error, and it would
    /// point at generated code.
    pub uses: Vec<String>,
    /// Directories `use` searches, in order, before Rite's working-directory
    /// fallback. The program's own directory belongs here first.
    pub module_roots: Vec<std::path::PathBuf>,
}

impl Environment {
    /// The `use` lines to emit above the generated functions: the program's
    /// own first, then anything the host added that the program did not
    /// already name.
    pub fn imports(&self, declared: &[String]) -> Vec<String> {
        let mut out: Vec<String> = declared.iter().map(|u| format!("use {u}")).collect();
        for extra in &self.uses {
            if !declared.iter().any(|d| d == extra) {
                out.push(format!("use {extra}"));
            }
        }
        out.extend(self.preamble.iter().cloned());
        out
    }
}

/// Parse, build the graph, expand, and run the result through Rite's front end.
pub fn check(name: &str, text: &str) -> CheckResult {
    check_with(name, text, &Environment::default())
}

/// [`check`], in a host-supplied [`Environment`].
///
/// The module roots matter: check compiles the generated Rite, and a `use
/// helpers` in it must find `helpers.rite` beside the *program*, not beside
/// whatever the process's working directory happens to be. Without them,
/// checking a module-importing program from any other directory reported
/// "module not found" for a module that was right there — and `cant run`
/// checks before it runs, so the run failed too.
pub fn check_with(name: &str, text: &str, env: &Environment) -> CheckResult {
    let (expansion, analysis) = expand_with(name, text, env);
    let mut diagnostics = analysis.diagnostics.clone();

    if let Some(expansion) = &expansion {
        let file = analysis
            .sources
            .files()
            .first()
            .map(|f| f.id)
            .unwrap_or(FileId(0));
        let generated = SourceFile::new(FileId(u32::MAX - 1), "<generated>.rite", &expansion.rite);
        let (_, rite_diagnostics) =
            rite_sem::compile_to_ir_with_roots(&generated, None, &env.module_roots);
        let remapped: Vec<_> = rite_diagnostics
            .iter()
            .map(|d| cant_sem::remap_diagnostic(d, &expansion.map, file))
            .collect();
        // Rite reports an unmarked host call three times — at the call, at the
        // generated function holding it, and at the generated `main`. Only the
        // first names something the user wrote.
        diagnostics.extend(cant_sem::expand::collapse_cascades(
            remapped,
            &expansion.prefix,
        ));
    }

    CheckResult {
        analysis,
        expansion,
        diagnostics,
    }
}

/// Canonical ASCII Rite for a source, plus the span map that ties it back.
///
/// `None` when the program could not be analyzed. Expansion is *not* attempted
/// on a graph with errors: generated Rite from a program Cant has already
/// rejected would be a guess, and printing it as though it were the program is
/// how an audit tool starts lying.
pub fn expand(name: &str, text: &str) -> (Option<Expansion>, AnalyzeResult) {
    expand_with(name, text, &Environment::default())
}

/// [`expand`], in a host-supplied [`Environment`].
pub fn expand_with(
    name: &str,
    text: &str,
    env: &Environment,
) -> (Option<Expansion>, AnalyzeResult) {
    let analysis = analyze(name, text);
    if analysis.has_errors() {
        return (None, analysis);
    }
    let expansion = analysis.graph.as_ref().map(|g| {
        cant_sem::expand(
            g,
            text,
            &ExpandOptions {
                source_name: name.to_string(),
                imports: env.imports(&g.uses),
                trace: false,
            },
        )
    });
    (expansion, analysis)
}

/// What `cant version` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub tool: &'static str,
    pub language: &'static str,
    pub graph_schema: &'static str,
    pub rite: &'static str,
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        tool: TOOL_VERSION,
        language: CANT_LANGUAGE_VERSION,
        graph_schema: GRAPH_SCHEMA_VERSION,
        rite: RITE_VERSION,
    }
}

impl VersionInfo {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cant": self.tool,
            "cant_language_version": self.language,
            "cant_graph_schema_version": self.graph_schema,
            "rite": self.rite,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_names_both_languages() {
        let v = version_info();
        assert!(!v.tool.is_empty());
        assert_eq!(v.language, "1");
        // Independent numbers, and this asserts the *source* rather than the
        // values so it keeps meaning something if they ever coincide.
        assert_eq!(v.rite, rite_core::VERSION);
        assert_eq!(v.tool, TOOL_VERSION);
        assert_eq!(v.to_json()["cant_language_version"], serde_json::json!("1"));
    }

    #[test]
    fn the_facade_parses() {
        let (result, _) = parse_source("t.cant", "5 -> |{ $ + 1 ; $ * 2 } -> []");
        assert!(!result.has_errors());
    }
}
