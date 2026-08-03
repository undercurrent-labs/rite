//! Cant graph → canonical ASCII Rite.
//!
//! This is the execution boundary. `cant expand` prints exactly what `cant run`
//! executes, and everything below is ordinary Rite that Rite's own parser,
//! resolver and runtime handle — see
//! `docs/adr/0002-cant-lowers-through-rite.md`.
//!
//! # Effects stay visible
//!
//! Every generated function that performs a host call contains that call in its
//! own body, marked, and is declared `def!`. Nothing is passed to an opaque
//! higher-order helper. That is not a style preference: Rite's effect analysis
//! cannot see through a function value, so a lowering that handed an orbit body
//! to a generic `orbit(seed, body_fn, …)` would let a Cant program perform
//! `@fs.read` with no marker and no grant. Effect-ness is computed over the
//! generated call graph here, so a `def!` and its call sites always agree, and
//! Rite's resolver re-derives it independently and rejects us if we got it wrong
//! (`E021`).
//!
//! # Hygiene
//!
//! Names combine a reserved prefix, a hash of the Cant source, and the node
//! number: `cant_1f4a9c2b_n3`. A prefix alone is not enough — two different Cant
//! programs would generate the same helper names, which matters the moment one
//! is `use`d from the other.

use std::collections::{BTreeMap, HashSet};

use cant_syntax::{lex, CantTokenKind as K, Spelling};
use rite_core::{FileId, SourceFile, Span};

use crate::graph::{CantProgram, LeafExpr, NodeKind, SubgraphId};
use crate::NodeId;

/// What the expansion produced.
#[derive(Debug, Clone)]
pub struct Expansion {
    /// Canonical ASCII Rite. Deterministic for a given source and tool version.
    pub rite: String,
    /// Cant ↔ Rite span pairs, in generated order.
    pub map: SourceMap,
    /// The hygienic prefix every generated name carries.
    pub prefix: String,
}

/// One Cant span and the region of generated Rite it produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub cant: Span,
    pub rite: Span,
    pub node: NodeId,
    /// A leaf maps precisely; a structural region maps to the control flow the
    /// operator generated. When a Rite span falls inside both, the leaf wins —
    /// it is the more specific answer, and the one a user wrote.
    pub precise: bool,
}

/// Bidirectional Cant ↔ generated-Rite span mapping.
///
/// Built as the text is emitted rather than recovered afterwards: the generator
/// is the only thing that knows which bytes came from which node, and
/// reconstructing it by searching the output would be a second implementation
/// of the same knowledge, free to disagree.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    mappings: Vec<Mapping>,
}

impl SourceMap {
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// The Cant span responsible for a position in the generated Rite.
    ///
    /// Prefers the smallest containing region, and a precise (leaf) mapping over
    /// a structural one. A diagnostic that lands on generated scaffolding with no
    /// leaf under it still resolves to the operator that produced the
    /// scaffolding, which is what §8.5 asks for: "generated helper-only spans map
    /// to the nearest owning Cant operator".
    pub fn to_cant(&self, rite: Span) -> Option<Mapping> {
        let pos = rite.start.as_usize();
        self.mappings
            .iter()
            .filter(|m| {
                m.rite.start.as_usize() <= pos
                    && pos < m.rite.end.as_usize().max(m.rite.start.as_usize() + 1)
            })
            .min_by_key(|m| (!m.precise, m.rite.len()))
            .copied()
    }

    /// Where a Cant span ended up in the generated Rite.
    pub fn to_rite(&self, cant: Span) -> Option<Span> {
        self.mappings
            .iter()
            .filter(|m| m.cant.start == cant.start && m.precise)
            .map(|m| m.rite)
            .next()
            .or_else(|| {
                self.mappings
                    .iter()
                    .filter(|m| m.cant.contains_span(cant))
                    .min_by_key(|m| m.cant.len())
                    .map(|m| m.rite)
            })
    }
}

/// Options for expansion. Only the things a caller can reasonably vary.
#[derive(Debug, Clone)]
pub struct ExpandOptions {
    /// Name used in the generated header comment.
    pub source_name: String,
    /// `use` lines to emit before the generated functions, verbatim.
    pub imports: Vec<String>,
}

impl Default for ExpandOptions {
    fn default() -> Self {
        Self {
            source_name: "program.cant".to_string(),
            imports: Vec::new(),
        }
    }
}

pub fn expand(graph: &CantProgram, source: &str, options: &ExpandOptions) -> Expansion {
    let prefix = format!("cant_{}", short_hash(source));
    let mut w = Writer {
        graph,
        prefix: prefix.clone(),
        out: String::new(),
        map: SourceMap::default(),
        effectful: HashSet::new(),
    };
    w.compute_effects();
    w.program(options);
    Expansion {
        rite: w.out,
        map: w.map,
        prefix,
    }
}

/// A short, stable, dependency-free digest of the source.
///
/// FNV-1a rather than SHA-256: this is a collision-avoidance token between two
/// generated programs, not a security boundary, and 64 bits of it is far more
/// than enough for names that only ever coexist inside one compilation. Written
/// out rather than pulled in so `cant-sem` keeps its four dependencies.
fn short_hash(source: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in source.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")[..8].to_string()
}

struct Writer<'a> {
    graph: &'a CantProgram,
    prefix: String,
    out: String,
    map: SourceMap,
    effectful: HashSet<NodeId>,
}

impl Writer<'_> {
    // ---- names

    fn node_fn(&self, id: NodeId) -> String {
        format!("{}_n{}", self.prefix, id.0)
    }

    fn chain_fn(&self, id: SubgraphId) -> String {
        format!("{}_s{}", self.prefix, id.0)
    }

    fn main_fn(&self) -> String {
        format!("{}_main", self.prefix)
    }

    fn boundary_fn(&self) -> String {
        format!("{}_boundary", self.prefix)
    }

    // ---- effects

    /// Which generated functions perform a host call.
    ///
    /// Computed over the *generated* call graph, not guessed: a fork or orbit is
    /// effectful exactly when something inside it is, and it is the enclosing
    /// `def!` that Rite will check. Iterated to a fixed point for the same reason
    /// Rite's resolver does — nesting means one pass is not enough.
    fn compute_effects(&mut self) {
        for node in &self.graph.nodes {
            if node.kind.leaf().is_some_and(|l| l.effectful) {
                self.effectful.insert(node.id);
            }
        }
        loop {
            let mut changed = false;
            for node in &self.graph.nodes {
                if self.effectful.contains(&node.id) {
                    continue;
                }
                let subgraphs: Vec<SubgraphId> = match &node.kind {
                    NodeKind::Fork { branches } => branches.clone(),
                    NodeKind::Orbit { body, .. } => vec![*body],
                    _ => continue,
                };
                let inner = subgraphs.iter().any(|id| {
                    self.graph
                        .subgraph(*id)
                        .map(|s| s.nodes.iter().any(|n| self.effectful.contains(n)))
                        .unwrap_or(false)
                });
                if inner {
                    self.effectful.insert(node.id);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn node_is_effectful(&self, id: NodeId) -> bool {
        self.effectful.contains(&id)
    }

    fn subgraph_is_effectful(&self, id: SubgraphId) -> bool {
        self.graph
            .subgraph(id)
            .map(|s| s.nodes.iter().any(|n| self.effectful.contains(n)))
            .unwrap_or(false)
    }

    fn any_effect(&self) -> bool {
        !self.effectful.is_empty()
    }

    /// `def!` or `def`, and the `!` a caller needs.
    fn def_kw(&self, effectful: bool) -> &'static str {
        if effectful {
            "def!"
        } else {
            "def"
        }
    }

    fn call_marker(&self, effectful: bool) -> &'static str {
        if effectful {
            "! "
        } else {
            ""
        }
    }

    // ---- emitting

    fn push(&mut self, text: &str) {
        self.out.push_str(text);
    }

    /// Emit `text`, recording that it came from `cant`.
    fn push_mapped(&mut self, text: &str, cant: Span, node: NodeId, precise: bool) {
        let start = self.out.len();
        self.out.push_str(text);
        self.map.mappings.push(Mapping {
            cant,
            rite: Span::from_range(start, self.out.len()),
            node,
            precise,
        });
    }

    fn region<F: FnOnce(&mut Self)>(&mut self, cant: Span, node: NodeId, body: F) {
        let start = self.out.len();
        body(self);
        self.map.mappings.push(Mapping {
            cant,
            rite: Span::from_range(start, self.out.len()),
            node,
            precise: false,
        });
    }

    // ---- the program

    fn program(&mut self, options: &ExpandOptions) {
        let prefix = self.prefix.clone();
        self.push(&format!(
            "// Generated from {} by cant {}. Do not edit.\n\
             //\n\
             // This is ordinary Rite, and it is exactly what `cant run` executes.\n\
             // Every generated name carries the prefix `{}`, which combines a\n\
             // reserved word, a hash of the Cant source, and a node number.\n",
            options.source_name,
            env!("CARGO_PKG_VERSION"),
            prefix,
        ));

        if !options.imports.is_empty() {
            self.push("\n");
            for import in &options.imports {
                self.push(import);
                self.push("\n");
            }
        }

        // Node functions, in identifier order, so the output reads top to bottom
        // in the order the nodes were written.
        let ids: Vec<NodeId> = {
            let mut ids: Vec<NodeId> = self.graph.nodes.iter().map(|n| n.id).collect();
            ids.sort();
            ids
        };
        for id in ids {
            self.push("\n");
            self.node(id);
        }

        // Subgraph chains.
        let subgraphs: Vec<SubgraphId> = {
            let mut ids: Vec<SubgraphId> = self.graph.subgraphs.iter().map(|s| s.id).collect();
            ids.sort();
            ids
        };
        for id in subgraphs {
            self.push("\n");
            self.chain(id);
        }

        self.push("\n");
        self.boundary();
        self.push("\n");
        self.main();
    }

    fn node(&mut self, id: NodeId) {
        let Some(node) = self.graph.node(id).cloned() else {
            return;
        };
        let span = node.span;
        self.region(span, id, |w| match &node.kind {
            NodeKind::Source { expr } => w.source_node(id, expr),
            NodeKind::Stage { expr } => w.stage_node(id, expr),
            NodeKind::Scatter => w.scatter_node(id, span),
            NodeKind::Collect => w.collect_node(id),
            NodeKind::Ward { predicate } => w.ward_node(id, predicate),
            NodeKind::Fork { branches } => w.fork_node(id, branches),
            NodeKind::Orbit {
                body,
                identity,
                max_items,
            } => w.orbit_node(id, *body, identity.as_ref(), *max_items, span),
        });
    }

    /// The first stage: it ignores its input and emits one value.
    fn source_node(&mut self, id: NodeId, expr: &LeafExpr) {
        let (kw, name) = (self.def_kw(self.node_is_effectful(id)), self.node_fn(id));
        self.push(&format!(
            "// {}: the source\n{kw} {name}(__in) [[\n  __ignored <- __in\n  ^ [ ",
            id
        ));
        self.push_mapped(&expr.text, expr.span, id, true);
        self.push(" ]\n]]\n");
    }

    fn stage_node(&mut self, id: NodeId, expr: &LeafExpr) {
        let (kw, name) = (self.def_kw(self.node_is_effectful(id)), self.node_fn(id));
        let applied = apply_to(expr, "__e");
        self.push(&format!(
            "// {}: a stage, once per emission\n{kw} {name}(__in) [[\n  \
             __out <~ []\n  for __e in __in [[\n    __out := concat(__out, [ ",
            id
        ));
        self.push_mapped(&applied, expr.span, id, true);
        self.push(" ])\n  ]]\n  ^ __out\n]]\n");
    }

    /// Scatter checks the type itself so the failure names the `*` rather than
    /// landing somewhere inside `for`.
    fn scatter_node(&mut self, id: NodeId, span: Span) {
        let name = self.node_fn(id);
        let where_ = self.location(span);
        self.push(&format!(
            "// {id}: scatter — one emission per element\n\
             def {name}(__in) [[\n  \
             __out <~ []\n  \
             for __e in __in [[\n    \
             if (type_of(__e) != \"list\") [[\n      \
             ^ panic(\"CANT-R003: scatter expected a list at {where_}, got \" + type_of(__e))\n    \
             ]]\n    \
             for __x in __e [[\n      __out := concat(__out, [ __x ])\n    ]]\n  \
             ]]\n  ^ __out\n]]\n"
        ));
    }

    fn collect_node(&mut self, id: NodeId) {
        let name = self.node_fn(id);
        self.push(&format!(
            "// {id}: collect — every emission becomes one list\n\
             def {name}(__in) [[\n  ^ [ __in ]\n]]\n"
        ));
    }

    fn ward_node(&mut self, id: NodeId, predicate: &LeafExpr) {
        let name = self.node_fn(id);
        let condition = apply_to(predicate, "__e");
        self.push(&format!(
            "// {id}: ward — the input unchanged, or nothing\n\
             def {name}(__in) [[\n  __out <~ []\n  for __e in __in [[\n    if ("
        ));
        self.push_mapped(&condition, predicate.span, id, true);
        self.push(") [[\n      __out := concat(__out, [ __e ])\n    ]]\n  ]]\n  ^ __out\n]]\n");
    }

    fn fork_node(&mut self, id: NodeId, branches: &[SubgraphId]) {
        let effectful = self.node_is_effectful(id);
        let (kw, name) = (self.def_kw(effectful), self.node_fn(id));
        self.push(&format!(
            "// {id}: fork — every branch sees the same input, results concatenated\n\
             {kw} {name}(__in) [[\n  __out <~ []\n  for __e in __in [[\n"
        ));
        for branch in branches {
            let chain = self.chain_fn(*branch);
            let marker = self.call_marker(self.subgraph_is_effectful(*branch));
            self.push(&format!(
                "    __out := concat(__out, {marker}{chain}([ __e ]))\n"
            ));
        }
        self.push("  ]]\n  ^ __out\n]]\n");
    }

    fn orbit_node(
        &mut self,
        id: NodeId,
        body: SubgraphId,
        identity: Option<&LeafExpr>,
        max_items: u64,
        span: Span,
    ) {
        let effectful = self.node_is_effectful(id);
        let (kw, name) = (self.def_kw(effectful), self.node_fn(id));
        let chain = self.chain_fn(body);
        let marker = self.call_marker(self.subgraph_is_effectful(body));
        let where_ = self.location(span);

        self.push(&format!(
            "// {id}: orbit — breadth-first, deduplicated, at most {max_items} candidates\n\
             {kw} {name}(__in) [[\n  \
             __work <~ __in\n  \
             __seen <~ []\n  \
             __out <~ []\n  \
             __accepted <~ 0\n  \
             while (count(__work) > 0) [[\n    \
             __c <- first(__work)\n    \
             __work := rest(__work)\n    \
             __k <- "
        ));
        match identity {
            Some(identity) => {
                let applied = apply_to(identity, "__c");
                self.push_mapped(&applied, identity.span, id, true);
            }
            // No `:by`: structural value identity, which in Rite is the value.
            None => self.push("__c"),
        }
        self.push(&format!(
            "\n    if (not contains(__seen, __k)) [[\n      \
             __seen := concat(__seen, [ __k ])\n      \
             __accepted := __accepted + 1\n      \
             if (__accepted > {max_items}) [[\n        \
             ^ panic(\"CANT-O002: orbit at {where_} accepted its limit of {max_items} candidates\")\n      \
             ]]\n      \
             __out := concat(__out, [ __c ])\n      \
             __work := concat(__work, {marker}{chain}([ __c ]))\n    \
             ]]\n  ]]\n  ^ __out\n]]\n"
        ));
    }

    /// A subgraph's chain: its nodes applied in order.
    fn chain(&mut self, id: SubgraphId) {
        let Some(subgraph) = self.graph.subgraph(id).cloned() else {
            return;
        };
        let effectful = self.subgraph_is_effectful(id);
        let (kw, name) = (self.def_kw(effectful), self.chain_fn(id));
        let owner = subgraph.owner;
        self.push(&format!(
            "// {id}: the flow inside {owner}\n{kw} {name}(__in) [[\n  __v <~ __in\n"
        ));
        for node in &subgraph.nodes {
            let call = self.node_fn(*node);
            let marker = self.call_marker(self.node_is_effectful(*node));
            self.push(&format!("  __v := {marker}{call}(__v)\n"));
        }
        self.push("  ^ __v\n]]\n");
    }

    /// Program-boundary normalization: zero → `none`, one → the value, many → a
    /// list preserving emission order.
    fn boundary(&mut self) {
        let name = self.boundary_fn();
        self.push(&format!(
            "// The program boundary: zero emissions become `none`, one becomes\n\
             // that value, and many become a list in emission order.\n\
             def {name}(__in) [[\n  \
             __n <- count(__in)\n  \
             if (__n = 0) [[ ^ none ]]\n  \
             if (__n = 1) [[ ^ first(__in) ]]\n  \
             ^ __in\n]]\n"
        ));
    }

    fn main(&mut self) {
        let effectful = self.any_effect();
        let (kw, name) = (self.def_kw(effectful), self.main_fn());
        let boundary = self.boundary_fn();

        // Top-level nodes only: everything else runs inside its subgraph's chain.
        let top: Vec<NodeId> = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.subgraph.is_none())
            .map(|n| n.id)
            .collect();

        self.push(&format!(
            "// The top-level flow.\n{kw} {name}() [[\n  __v <~ [ none ]\n"
        ));
        for node in &top {
            let call = self.node_fn(*node);
            let marker = self.call_marker(self.node_is_effectful(*node));
            self.push(&format!("  __v := {marker}{call}(__v)\n"));
        }
        self.push(&format!("  ^ {boundary}(__v)\n]]\n\n"));
        self.push(&format!("{}{name}()\n", self.call_marker(effectful)));
    }

    /// `name:line:col` for a runtime message, computed from the Cant source.
    fn location(&self, span: Span) -> String {
        // Only the byte offset is available here; the CLI renders the real
        // line and column from the span when it remaps the diagnostic. This is
        // the fallback that appears inside a `panic` string, where there is no
        // diagnostic machinery — so it names the file and the offset, which is
        // enough to find with an editor and honest about what it knows.
        format!("{}+{}", self.graph.source.name, span.start)
    }
}

/// Apply a leaf to the current emission, bound to `var`.
///
/// Three shapes, because Rite treats them differently and the differences were
/// established by experiment, not assumption:
///
/// * **`$` present** — substitute. A Rite pipeline cannot carry `$` outside a
///   call (`5 -> ($ > 2)` is a runtime error, `3 -> $ + 1` is `E015`), so a leaf
///   like `$ % 2 = 0` has to become `__e % 2 = 0` textually.
/// * **A capability call** — insert the receiver explicitly. Rite's pipeline does
///   **not** insert into `@cap.fn`: `"[1]" -> @json.decode` fails with "expects
///   string" because nothing was passed. And an effect marker cannot appear
///   inside a pipeline stage at all, so `x -> ! @fs.read` does not parse. Both
///   force the direct-call form — which is also what ADR 0002 wants, since a
///   direct call is maximally visible to Rite's effect analysis.
/// * **Anything else** — a Rite pipeline, `var -> leaf`, which gets Rite's own
///   first-argument insertion for free and so cannot drift from it.
///
/// Substitution re-lexes with the Cant lexer rather than scanning text, so a `$`
/// inside a string stays a `$` inside a string.
pub fn apply_to(leaf: &LeafExpr, var: &str) -> String {
    if leaf.placeholder {
        return substitute_placeholder(&leaf.text, var);
    }
    match capability_call(&leaf.text) {
        Some(insert_at) => {
            let mut out = String::with_capacity(leaf.text.len() + var.len() + 4);
            out.push_str(&leaf.text[..insert_at.at]);
            if insert_at.has_parens {
                out.push('(');
                out.push_str(var);
                if !insert_at.empty_args {
                    out.push_str(", ");
                }
                out.push_str(&leaf.text[insert_at.at + 1..]);
            } else {
                out.push('(');
                out.push_str(var);
                out.push(')');
                out.push_str(&leaf.text[insert_at.at..]);
            }
            out
        }
        None => format!("{var} -> {}", leaf.text),
    }
}

/// Replace every `$` token — and only real ones — with `var`.
fn substitute_placeholder(text: &str, var: &str) -> String {
    let file = SourceFile::new(FileId(0), "leaf.cant", text);
    let (tokens, _) = lex(&file);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for token in &tokens {
        if token.kind != K::Dollar || token.spelling != Spelling::Ascii {
            continue;
        }
        let start = token.span.start.as_usize().min(text.len());
        let end = token.span.end.as_usize().min(text.len());
        if start < cursor {
            continue;
        }
        out.push_str(&text[cursor..start]);
        out.push_str(var);
        cursor = end;
    }
    out.push_str(&text[cursor.min(text.len())..]);
    out
}

struct CapabilityInsert {
    /// Byte offset where the receiver goes: the `(` of an existing call, or the
    /// end of the capability path when there is none.
    at: usize,
    has_parens: bool,
    empty_args: bool,
}

/// Operators that may trail a capability call and still leave it one.
///
/// `?` is the one that matters: `@fs.read` returns a result, so `!@fs.read?` is
/// how anyone would actually write the stage, and Rite accepts `! @fs.read(p)?`.
/// Without this the leaf fell through to the pipeline path and did not parse,
/// because an effect marker cannot appear inside a pipeline stage.
const TRAILING_OPS: &[&str] = &["?"];

/// Is this leaf `[!] @path.to.fn [ ( args ) ]`, and where does the receiver go?
///
/// The test is the *outermost* form. `@json.decode(@fs.read(p))` qualifies — it
/// is a capability call whose argument happens to be another one — and the
/// receiver goes in front of the existing argument, which is exactly the stage
/// rule: "the current emission as its first argument unless it contains `$`".
///
/// `f(@fs.read(p))` does not qualify: the outermost call is an ordinary one, so
/// it takes the pipeline path and Rite does the insertion.
fn capability_call(text: &str) -> Option<CapabilityInsert> {
    let file = SourceFile::new(FileId(0), "leaf.cant", text);
    let (tokens, _) = lex(&file);
    let significant: Vec<_> = tokens
        .iter()
        .filter(|t| !t.kind.is_trivia() && t.kind != K::Eof)
        .collect();

    let mut i = 0;
    if significant.first().map(|t| t.kind) == Some(K::Bang) {
        i += 1;
    }
    if significant.get(i).map(|t| t.kind) != Some(K::At) {
        return None;
    }
    i += 1;
    if significant.get(i).map(|t| t.kind) != Some(K::Ident) {
        return None;
    }
    i += 1;
    while significant.get(i).map(|t| t.kind) == Some(K::Dot)
        && significant.get(i + 1).map(|t| t.kind) == Some(K::Ident)
    {
        i += 2;
    }
    let path_end = significant.get(i - 1)?.span.end.as_usize();

    // Anything trailing must be one of the postfix operators that leave the leaf
    // a capability call. `!@fs.read?` still is one; `!@fs.read + 1` is not.
    let trailing_ok = |from: usize| {
        significant[from..]
            .iter()
            .all(|t| t.kind == K::Op && TRAILING_OPS.contains(&t.text.as_str()))
    };

    match significant.get(i) {
        None => Some(CapabilityInsert {
            at: path_end,
            has_parens: false,
            empty_args: true,
        }),
        Some(open) if open.kind == K::LParen => {
            // Find the `)` that closes it, then allow only trailing postfix
            // operators after it — otherwise this is a capability call embedded
            // in a larger expression, which belongs on the pipeline path.
            let mut depth = 0usize;
            let mut close = None;
            for (offset, token) in significant[i..].iter().enumerate() {
                match token.kind {
                    K::LParen => depth += 1,
                    K::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(i + offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close?;
            if !trailing_ok(close + 1) {
                return None;
            }
            let empty_args = close == i + 1;
            Some(CapabilityInsert {
                at: open.span.start.as_usize(),
                has_parens: true,
                empty_args,
            })
        }
        Some(_) if trailing_ok(i) => Some(CapabilityInsert {
            at: path_end,
            has_parens: false,
            empty_args: true,
        }),
        _ => None,
    }
}

/// Remap a Rite diagnostic onto the Cant source that produced it.
///
/// The primary label moves to `.cant`; the Rite code, title and generated span
/// travel as [`cant_syntax::RiteOrigin`]. A user must never be shown
/// `cant_1f4a9c2b_n3` as the location of their mistake — §2.4.
pub fn remap_diagnostic(
    diagnostic: &rite_core::Diagnostic,
    map: &SourceMap,
    cant_file: FileId,
) -> cant_syntax::CantDiagnostic {
    use cant_syntax::diagnostic::*;

    let generated = diagnostic.primary_span();
    let mapping = generated.and_then(|s| map.to_cant(s.span));

    let code = match diagnostic.code.0 {
        // Rite's own grouping: E00x lex, E01x parse, E02x resolve, E03x runtime,
        // E04x permission.
        //
        // A lex or parse error in *generated* Rite is not a syntax error in the
        // Cant source — that already parsed. It means a leaf Cant carried through
        // verbatim is not valid Rite, which is a semantic problem with the
        // program and exits 4 rather than 3.
        c if c < 20 => CANT_S004_LEAF_NOT_VALID_RITE,
        21 => CANT_S001_EFFECT_REQUIRED,
        20 => CANT_S002_UNDEFINED_NAME,
        c if (20..30).contains(&c) => CANT_S003_RITE_SEMANTIC,
        c if (40..50).contains(&c) => CANT_R002_PERMISSION_DENIED,
        _ => CANT_R001_RITE_RUNTIME,
    };

    let mut out = CantDiagnostic::error(code, diagnostic.title.clone());
    match mapping {
        Some(mapping) => {
            out = out.with_primary(
                rite_core::SourceSpan::new(cant_file, mapping.cant),
                diagnostic
                    .labels
                    .iter()
                    .find(|l| l.primary)
                    .map(|l| l.message.clone())
                    .unwrap_or_default(),
            );
        }
        None => {
            // No mapping: say so rather than inventing a location. A diagnostic
            // pointing at the wrong line is worse than one pointing nowhere.
            out = out.with_note(
                "this came from generated Rite that maps to no Cant source; \
                 run `cant expand` to see it",
            );
        }
    }
    if code == CANT_S004_LEAF_NOT_VALID_RITE {
        out = out.with_note(
            "a Cant stage is Rite expression text, passed through unchanged; this is Rite's parser reading it",
        );
    }
    for note in &diagnostic.notes {
        out = out.with_note(note.clone());
    }
    if let Some(help) = &diagnostic.help {
        out = out.with_help(help.clone());
    }
    out.with_rite_origin(RiteOrigin {
        code: diagnostic.code.to_string(),
        span: generated,
        title: diagnostic.title.clone(),
    })
}

/// Drop the diagnostics that are only consequences of another one.
///
/// Rite reports an unmarked host call three times: at the call site, at the
/// generated function containing it, and at the generated `main` that calls
/// *that*. Only the first is something a user wrote or can act on. The other two
/// name identifiers Cant invented — and §2.4 is explicit that a generated
/// implementation detail must not be what someone is shown.
///
/// The test is whether the diagnostic *names* a generated identifier, not where
/// it points: the second one in that cascade maps to a perfectly good Cant span,
/// because the generated function's body is where the user's leaf ended up. Only
/// the text gives it away.
///
/// When every diagnostic names a generated identifier, they are all kept. That
/// means Cant generated something Rite rejected with no user error behind it —
/// a bug in this crate — and hiding it would turn that into a mystery.
pub fn collapse_cascades(
    diagnostics: Vec<cant_syntax::CantDiagnostic>,
    prefix: &str,
) -> Vec<cant_syntax::CantDiagnostic> {
    let mentions_generated = |d: &cant_syntax::CantDiagnostic| {
        d.title.contains(prefix)
            || d.help.as_deref().is_some_and(|h| h.contains(prefix))
            || d.notes.iter().any(|n| n.contains(prefix))
    };
    if diagnostics.iter().all(mentions_generated) {
        return diagnostics;
    }
    diagnostics
        .into_iter()
        .filter(|d| !mentions_generated(d))
        .collect()
}

/// Every Cant span the expansion emitted, for a caller that wants to check
/// coverage.
pub fn covered_spans(map: &SourceMap) -> BTreeMap<u32, u32> {
    map.mappings()
        .iter()
        .map(|m| (m.cant.start.0, m.cant.end.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(text: &str) -> LeafExpr {
        let has_dollar = {
            let file = SourceFile::new(FileId(0), "l.cant", text);
            let (tokens, _) = lex(&file);
            tokens.iter().any(|t| t.kind == K::Dollar)
        };
        LeafExpr {
            text: text.to_string(),
            span: Span::from_range(0, text.len()),
            effectful: text.contains('!'),
            placeholder: has_dollar,
        }
    }

    #[test]
    fn a_leaf_with_a_placeholder_is_substituted() {
        assert_eq!(apply_to(&leaf("$ % 2 = 0"), "__e"), "__e % 2 = 0");
        assert_eq!(
            apply_to(&leaf("$.level = :error"), "__e"),
            "__e.level = :error"
        );
        assert_eq!(
            apply_to(&leaf("join([\"a\"], $)"), "__e"),
            "join([\"a\"], __e)"
        );
    }

    #[test]
    fn a_dollar_inside_a_string_is_not_a_placeholder() {
        assert_eq!(
            apply_to(&leaf("replace($, \"$\", \"x\")"), "__e"),
            "replace(__e, \"$\", \"x\")"
        );
    }

    #[test]
    fn a_capability_call_gets_the_receiver_inserted() {
        assert_eq!(apply_to(&leaf("!@fs.read"), "__e"), "!@fs.read(__e)");
        assert_eq!(
            apply_to(&leaf("!@fs.write(contents)"), "__e"),
            "!@fs.write(__e, contents)"
        );
        assert_eq!(apply_to(&leaf("@json.decode"), "__e"), "@json.decode(__e)");
        assert_eq!(apply_to(&leaf("@clock.now()"), "__e"), "@clock.now(__e)");
    }

    #[test]
    fn a_nested_capability_call_still_takes_the_receiver_first() {
        // The outermost form is what decides. This is a capability call whose
        // argument is another one, so the emission goes in front of it — the
        // ordinary stage rule.
        assert_eq!(
            apply_to(&leaf("@json.decode(@fs.read(p))"), "__e"),
            "@json.decode(__e, @fs.read(p))"
        );
    }

    #[test]
    fn an_ordinary_call_wrapping_a_capability_uses_the_pipeline() {
        // The outermost call is not a capability, so Rite's own insertion applies
        // and Cant does not second-guess it.
        assert_eq!(
            apply_to(&leaf("decode(@fs.read(p))"), "__e"),
            "__e -> decode(@fs.read(p))"
        );
    }

    #[test]
    fn an_ordinary_leaf_becomes_a_rite_pipeline() {
        assert_eq!(apply_to(&leaf("square"), "__e"), "__e -> square");
        assert_eq!(apply_to(&leaf(".message"), "__e"), "__e -> .message");
        assert_eq!(
            apply_to(&leaf("replace(\"hay\", \"x\")"), "__e"),
            "__e -> replace(\"hay\", \"x\")"
        );
    }

    #[test]
    fn the_hash_is_stable_and_differs_between_sources() {
        assert_eq!(short_hash("a -> b"), short_hash("a -> b"));
        assert_ne!(short_hash("a -> b"), short_hash("a -> c"));
        assert_eq!(short_hash("a -> b").len(), 8);
    }
}
