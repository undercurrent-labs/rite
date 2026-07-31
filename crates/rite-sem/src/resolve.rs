//! Lexical resolver: scopes, bindings, effect checks, imports.

use crate::ir::*;
use rite_core::{
    simple_error, Diagnostics, SourceFile, Span, E020_UNDEFINED_NAME, E021_EFFECT_REQUIRED,
    E022_DUPLICATE_BINDING, E023_IMMUTABLE_ASSIGN, E029_NON_EXHAUSTIVE_MATCH,
};
use rite_syntax::{
    Block, EventDecl, Expr, FunctionDecl, Item, LitKind, Pattern, Program, ResultPatKind, Stmt,
    UnaryOp,
};
use std::collections::{HashMap, HashSet};

/// The canonical effect table: every host capability function, and whether
/// calling it requires an explicit `!` / `do` marker (diagnostic `E021`).
///
/// # This is the single source of truth
///
/// `rite-caps` depends on `rite-sem`, so this crate cannot read the
/// `NativeFunctionDescriptor`s that carry the same information for
/// `rite capabilities` and the generated docs. The dependency is inverted
/// instead: this table is authoritative, and
/// `crates/rite-caps/tests/effect_parity.rs` fails if any descriptor's
/// `effectful:` flag disagrees with it, in either direction. A new host
/// function must be added here in the same change that adds its descriptor.
///
/// # Classification rule
///
/// A host function is effectful iff calling it observes or changes state
/// **outside the program**: the filesystem, the process environment,
/// subprocesses, network sockets, the terminal, the wall clock, or the
/// entropy source. Pure host helpers (`@json.encode`, `@clock.format`) are
/// deterministic functions of their arguments and need no marker.
///
/// Reading is an effect, not just writing: `@fs.read` and `@env.get` are as
/// unpredictable across runs as `@clock.now`, which this table has always
/// treated as effectful.
///
/// State that a capability owns *in process* (`@game`'s world, `@store`'s
/// map) is the one exception: writes to it are effectful because another
/// read can observe them, but reading it is like reading a mutable binding,
/// which needs no marker. Hence `@game.look` and `@store.get` are pure here
/// while `@game.go` and `@store.set` are not.
pub const HOST_EFFECTS: &[(&str, bool)] = &[
    // @console — the terminal.
    ("console.print", true),
    ("console.println", true),
    ("console.warn", true),
    ("console.error", true),
    ("console.inspect", true),
    ("console.read_line", true),
    // @fs — the filesystem. Reads observe it; writes change it.
    ("fs.read", true),
    ("fs.read_bytes", true),
    ("fs.write", true),
    ("fs.append", true),
    ("fs.lines", true),
    ("fs.exists", true),
    ("fs.metadata", true),
    ("fs.glob", true),
    ("fs.mkdir", true),
    ("fs.remove", true),
    ("fs.copy", true),
    ("fs.move", true),
    // Open handles. Every one touches the file behind the handle — even `close`,
    // which flushes — so every one takes a marker.
    ("fs.open", true),
    ("fs.read_chunk", true),
    ("fs.read_line", true),
    ("fs.write_chunk", true),
    ("fs.seek", true),
    ("fs.flush", true),
    ("fs.close", true),
    // @json — encode/decode are pure string transforms; read/write touch disk.
    ("json.decode", false),
    ("json.encode", false),
    ("json.encode_pretty", false),
    ("json.read", true),
    ("json.write", true),
    // @csv — as @json.
    ("csv.decode", false),
    ("csv.encode", false),
    ("csv.read", true),
    ("csv.write", true),
    // @crypto — value transforms. A digest, an HMAC and an encoding are functions
    // of their arguments alone: the same input gives the same answer on every run
    // and nothing outside the program is touched, so they take no marker. Only
    // `random_bytes` reads the OS entropy pool, which is state outside the program
    // and different every call — effectful for the same reason `@clock.now` is.
    ("crypto.sha256", false),
    ("crypto.sha512", false),
    ("crypto.hmac_sha256", false),
    ("crypto.random_bytes", true),
    ("crypto.constant_time_eq", false),
    ("crypto.base64_encode", false),
    ("crypto.base64_decode", false),
    ("crypto.hex_encode", false),
    ("crypto.hex_decode", false),
    // @clock — `now`/`sleep` consult the wall clock; the rest are value math.
    ("clock.now", true),
    ("clock.parse", false),
    ("clock.format", false),
    ("clock.add", false),
    ("clock.diff", false),
    ("clock.sleep", true),
    ("clock.duration", false),
    // @env — the process environment.
    ("env.get", true),
    ("env.require", true),
    ("env.all", true),
    // @process — subprocesses. `which` probes PATH and the filesystem.
    ("process.run", true),
    ("process.which", true),
    // `args` observes what the invoker typed — outside the program, and different
    // between runs, so it takes a marker for the same reason `@clock.now` does.
    ("process.args", true),
    // `exit` ends the process. Nothing is more effectful than that.
    ("process.exit", true),
    // @random — the entropy source (`seed` mutates it).
    ("random.int", true),
    ("random.float", true),
    ("random.choose", true),
    ("random.shuffle", true),
    ("random.uuid", true),
    ("random.seed", true),
    // @http — `listen` binds a socket. The others build values: `response`
    // is a record constructor, `log`/`recover` are middleware markers named
    // in `use @http.log` rather than called for their effect.
    ("http.listen", true),
    // Outbound requests reach the network.
    ("http.get", true),
    ("http.post", true),
    ("http.request", true),
    ("http.response", false),
    ("http.log", false),
    ("http.recover", false),
    // @udp — datagram sockets. Every one of these touches the socket: `bind` claims
    // a port, `local_addr` asks the OS which one it got, and the two transfers move
    // bytes on and off the wire.
    ("udp.bind", true),
    ("udp.local_addr", true),
    ("udp.send_to", true),
    ("udp.recv_from", true),
    ("udp.close", true),
    // @tcp — byte streams. `connect` dials out, `listen` claims a port and then
    // serves, and the two transfers move bytes on and off the wire. `close` releases
    // a file descriptor. There is nothing pure here.
    ("tcp.connect", true),
    ("tcp.send", true),
    ("tcp.recv", true),
    ("tcp.peer_addr", true),
    ("tcp.local_addr", true),
    ("tcp.close", true),
    ("tcp.listen", true),
    // @game — in-process world state: writes marked, reads not.
    ("game.register_item", true),
    ("game.register_room", true),
    ("game.register_world", true),
    ("game.say", true),
    ("game.reveal", true),
    ("game.go", true),
    ("game.take", true),
    ("game.drop", true),
    ("game.look", false),
    ("game.inventory", false),
    ("game.save", false),
    ("game.load", true),
    ("game.start", true),
    ("game.command", true),
    ("game.messages", false),
    ("game.state", false),
    // @store — in-process key/value state: writes marked, reads not.
    ("store.get", false),
    ("store.set", true),
    ("store.delete", true),
    // @db — an external database, including `query`.
    ("db.open", true),
    ("db.close", true),
    ("db.exec", true),
    ("db.query", true),
    ("db.prepare", true),
    ("db.query_prepared", true),
    ("db.exec_prepared", true),
    ("db.close_stmt", true),
    ("db.begin", true),
    ("db.commit", true),
    ("db.rollback", true),
];

/// Pure builtins predefined in every scope. Single list — `Resolver::new`
/// inserts all of these into `functions`, which is what the undefined-name
/// check consults, so there is no second copy to drift from.
///
/// Every entry must lex as one identifier, or the name can never be looked up:
/// `number?` used to be listed here and was unreachable, because the lexer
/// splits it into `number` and `?`.
/// The one list of builtin names. `rite-runtime` reads it too, so the resolver and
/// the interpreter cannot disagree about which bare names resolve to a builtin.
/// Builtins that reach the host. `print`/`println` write to the terminal just as
/// `@console.print` does, so they take a marker for the same reason.
pub const EFFECTFUL_BUILTINS: &[&str] = &["print", "println"];

pub const BUILTIN_NAMES: &[&str] = &[
    "map",
    "keep",
    "reject",
    "reduce",
    "each",
    "flatten",
    "count",
    "first",
    "last",
    "rest",
    "tail",
    "init",
    "butlast",
    "take",
    "drop",
    "reverse",
    "concat",
    "find",
    "any",
    "all",
    "sum",
    "min",
    "max",
    "sort",
    "unique",
    "zip",
    "chunk",
    "parallel",
    "ok",
    "err",
    "panic",
    "expect",
    "fail",
    "str",
    "len",
    "type_of",
    "require",
    "collect_results",
    "group",
    "lines",
    "words",
    "join",
    "range",
    "range_incl",
    "keys",
    "values",
    "abs",
    "clamp",
    "pow",
    "idiv",
    "split",
    "trim",
    "trim_start",
    "trim_end",
    "replace",
    "starts_with",
    "ends_with",
    "upper",
    "lower",
    "pad_start",
    "pad_end",
    "slice",
    "index_of",
    "round",
    "floor",
    "ceil",
    "sqrt",
    "parse_int",
    "parse_float",
    "bytes",
    "from_hex",
    "to_hex",
    "to_text",
    "byte_at",
    "xor",
    "and_then",
    "or_else",
    "is_ok",
    "is_err",
    "unwrap_or",
    "repeat",
    "contains",
    "enumerate",
    "with_index",
    "compose",
    "while_loop",
    "print",
    "println",
];

#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub ast: Program,
    pub functions: HashMap<String, FunctionMeta>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FunctionMeta {
    pub name: String,
    pub arity: usize,
    pub is_pub: bool,
    pub span: Span,
    /// Inferred: this function performs a host effect, directly or through
    /// something it calls. Computed by [`Resolver::infer_effects`] after the
    /// bodies have been walked, so it accounts for the whole call graph.
    pub effectful: bool,
    /// Declared with `◆!` / `def!`.
    pub declares_effect: bool,
}

pub struct Resolver {
    scopes: Vec<Scope>,
    next_local: u32,
    diagnostics: Diagnostics,
    functions: HashMap<String, FunctionMeta>,
    /// Names bound by `use` — `math` for `use math`, `m` for `use math as m`.
    /// A member access through one of these is a call into a module, and can be
    /// checked here rather than failing at runtime.
    import_qualifiers: HashSet<String>,
    /// The function whose body is being walked; `None` at top level.
    current_fn: Option<String>,
    /// The file diagnostics are attributed to, kept for checks that run after the
    /// walk (effect inference) rather than during it.
    file_for_effects: rite_core::FileId,
    /// Functions that perform a host effect in their own body.
    direct_effects: HashSet<String>,
    /// Every effect seen while walking, marked or not. Snapshotting this around
    /// a call's arguments is how an effectful lambda argument is detected.
    effects_seen: usize,
    /// Who calls whom, so effect-ness can be closed over the call graph.
    call_edges: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Default)]
struct Scope {
    bindings: HashMap<String, BindingInfo>,
}

#[derive(Debug, Clone)]
struct BindingInfo {
    #[allow(dead_code)]
    local: LocalId,
    mutable: bool,
    #[allow(dead_code)]
    span: Span,
}

pub fn resolve(program: &Program, file: &SourceFile) -> (ResolvedProgram, Diagnostics) {
    // Diagnostics are attributed to `program.file`, which the parser stamped from
    // the same `SourceFile` the caller passes here. Catch a mismatched pair in
    // debug builds rather than reporting spans against the wrong file.
    debug_assert_eq!(
        program.file, file.id,
        "resolve() called with a SourceFile that did not produce this Program"
    );
    let mut r = Resolver::new();
    r.resolve_program(program);
    r.infer_effects();
    let resolved = ResolvedProgram {
        ast: program.clone(),
        functions: r.functions.clone(),
        warnings: vec![],
    };
    (resolved, r.diagnostics)
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver {
    pub fn new() -> Self {
        let mut r = Self {
            scopes: vec![Scope::default()],
            next_local: 0,
            diagnostics: Diagnostics::new(),
            functions: HashMap::new(),
            import_qualifiers: HashSet::new(),
            current_fn: None,
            file_for_effects: rite_core::FileId(0),
            direct_effects: HashSet::new(),
            effects_seen: 0,
            call_edges: HashMap::new(),
        };
        // Predefine pure builtins
        for name in BUILTIN_NAMES {
            r.functions.insert(
                (*name).into(),
                FunctionMeta {
                    name: (*name).into(),
                    arity: 0,
                    is_pub: true,
                    span: Span::DUMMY,
                    // `print`/`println` reach the terminal exactly as
                    // `@console.print` does; the rest are value functions.
                    effectful: EFFECTFUL_BUILTINS.contains(name),
                    declares_effect: EFFECTFUL_BUILTINS.contains(name),
                },
            );
        }
        r
    }

    fn resolve_program(&mut self, program: &Program) {
        self.file_for_effects = program.file;
        // First pass: collect function declarations (may shadow builtins)
        for item in &program.items {
            if let Item::Function(f) = item {
                if let Some(existing) = self.functions.get(&f.name.name) {
                    // Builtins are pre-inserted with DUMMY span; real redefinition of a
                    // user function is an error. Shadowing a builtin is allowed.
                    if existing.span != Span::DUMMY {
                        self.diagnostics.push(simple_error(
                            E022_DUPLICATE_BINDING,
                            format!("duplicate function `{}`", f.name.name),
                            program.file,
                            f.name.span,
                            "already defined",
                        ));
                    }
                }
                self.functions.insert(
                    f.name.name.clone(),
                    FunctionMeta {
                        name: f.name.name.clone(),
                        arity: f.params.len(),
                        is_pub: f.is_pub,
                        span: f.span,
                        effectful: false,
                        declares_effect: f.is_effectful,
                    },
                );
            }
            // Bind the name an import qualifies with, so `m.fn` resolves (desugar
            // rewrites it to `m__fn`). `use math` binds `math` just as `use math as m`
            // binds `m`: qualifying is how two modules exporting the same name stay
            // usable, and requiring an alias for that would be busywork.
            if let Item::Import(imp) = item {
                match &imp.alias {
                    Some(alias) => {
                        self.define(&alias.name, false, alias.span, program.file);
                        self.import_qualifiers.insert(alias.name.clone());
                    }
                    None => {
                        if let Some(seg) = imp.path.segments.last() {
                            self.define(&seg.name, false, seg.span, program.file);
                            self.import_qualifiers.insert(seg.name.clone());
                        }
                    }
                }
            }
        }
        // Second pass: walk for effect checks and undefined names
        for item in &program.items {
            self.resolve_item(item, program.file);
        }
    }

    fn resolve_item(&mut self, item: &Item, file: rite_core::FileId) {
        match item {
            Item::Function(f) => self.resolve_function(f, file),
            Item::Statement(s) => self.resolve_stmt(s, file),
            Item::Test(t) => {
                self.push_scope();
                self.resolve_block(&t.body, file);
                self.pop_scope();
            }
            Item::Event(e) => self.resolve_event(e, file),
            Item::Import(_) => {}
            // `◆ Cfg ⟨a: 1⟩` lowers to a binding of the record, so the name has to be
            // defined here or every use reports E020. Bound *in order* rather than
            // hoisted like a function: desugar emits it as a top-level statement, so a
            // use above the declaration genuinely has no value yet. Declaring it in the
            // first pass instead would let the resolver accept that and leave it to fail
            // at runtime. Fields resolve before the name is bound, so a field cannot
            // reference the declaration itself.
            Item::Data(d) => {
                for entry in &d.fields {
                    self.resolve_expr(&entry.value, file, false);
                }
                self.define(&d.name.name, false, d.name.span, file);
            }
        }
    }

    fn resolve_function(&mut self, f: &FunctionDecl, file: rite_core::FileId) {
        // Remember which body we are inside, so host calls and calls to other
        // functions can be attributed to it and closed over afterwards.
        let outer = self.current_fn.replace(f.name.name.clone());
        self.push_scope();
        for p in &f.params {
            self.define(&p.name.name, false, p.span, file);
        }
        self.resolve_block(&f.body, file);
        self.pop_scope();
        self.current_fn = outer;
    }

    /// Attribute a host effect to the body currently being walked.
    fn note_effect(&mut self) {
        self.effects_seen += 1;
        if let Some(name) = self.current_fn.clone() {
            self.direct_effects.insert(name);
        }
    }

    /// Record `current → callee`, the edge effect-ness travels along.
    fn note_call(&mut self, callee: &str) {
        if let Some(name) = self.current_fn.clone() {
            self.call_edges
                .entry(name)
                .or_default()
                .insert(callee.to_string());
        }
    }

    /// Close direct effects over the call graph.
    ///
    /// A function is effectful if its own body performs an effect or if anything
    /// it calls is effectful. Recursion and mutual recursion make this a
    /// least-fixed-point rather than a walk, so iterate until nothing changes;
    /// the set only grows, so it terminates in at most one round per function.
    fn infer_effects(&mut self) {
        // Seeded from both what bodies do and what declarations promise, so a
        // caller of a declared-effectful function is itself effectful even when
        // its own body touches no capability directly.
        let mut effectful = self.direct_effects.clone();
        for (name, meta) in &self.functions {
            if meta.declares_effect {
                effectful.insert(name.clone());
            }
        }
        loop {
            let mut grew = false;
            for (caller, callees) in &self.call_edges {
                if effectful.contains(caller) {
                    continue;
                }
                if callees.iter().any(|c| effectful.contains(c)) {
                    effectful.insert(caller.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        for (name, meta) in self.functions.iter_mut() {
            meta.effectful = effectful.contains(name);
        }

        // A body that performs effects has to say so. Reported at the declaration
        // rather than at each call, so the fix is one edit in one place instead of
        // a marker on every caller.
        let mut undeclared: Vec<(String, Span)> = self
            .functions
            .values()
            .filter(|m| m.effectful && !m.declares_effect && m.span != Span::DUMMY)
            .map(|m| (m.name.clone(), m.span))
            .collect();
        undeclared.sort_by_key(|(_, span)| span.start);
        for (name, span) in undeclared {
            let reason = if self.direct_effects.contains(&name) {
                "its body performs a host effect"
            } else {
                "it calls a function that performs host effects"
            };
            self.diagnostics.push(
                rite_core::Diagnostic::error(
                    E021_EFFECT_REQUIRED,
                    format!("`{name}` performs host effects but is not declared `◆!`"),
                )
                .with_primary(
                    rite_core::SourceSpan::new(self.file_for_effects, span),
                    reason,
                )
                .with_help(format!(
                    "declare it `◆! {name}(…)` (ASCII `def! {name}(…)`), then callers mark the call with `!`"
                )),
            );
        }
    }

    fn resolve_event(&mut self, e: &EventDecl, file: rite_core::FileId) {
        self.push_scope();
        self.resolve_block(&e.body, file);
        self.pop_scope();
    }

    fn resolve_block(&mut self, block: &Block, file: rite_core::FileId) {
        self.push_scope();
        for p in &block.params {
            self.define(&p.name.name, false, p.span, file);
        }
        // Nested `◆` / `def` bind in the enclosing block (docs: local helpers).
        // Pre-declare so later statements and sibling helpers can reference them.
        for item in &block.body {
            if let Item::Function(f) = item {
                self.define(&f.name.name, false, f.name.span, file);
            }
        }
        for item in &block.body {
            self.resolve_item(item, file);
        }
        self.pop_scope();
    }

    fn resolve_stmt(&mut self, stmt: &Stmt, file: rite_core::FileId) {
        match stmt {
            Stmt::Binding(b) => {
                self.resolve_expr(&b.value, file, false);
                self.define_pattern(&b.pattern, b.mutable, file);
            }
            Stmt::Assign(a) => {
                if let Some(info) = self.lookup(&a.name.name) {
                    if !info.mutable {
                        self.diagnostics.push(simple_error(
                            E023_IMMUTABLE_ASSIGN,
                            format!("cannot assign to immutable binding `{}`", a.name.name),
                            file,
                            a.span,
                            "use ↢ / <~ for mutable bindings",
                        ));
                    }
                } else {
                    self.diagnostics.push(simple_error(
                        E020_UNDEFINED_NAME,
                        format!("undefined name `{}`", a.name.name),
                        file,
                        a.name.span,
                        "not found in scope",
                    ));
                }
                self.resolve_expr(&a.value, file, false);
            }
            Stmt::Expr(e) => {
                self.resolve_expr(e, file, false);
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.resolve_expr(v, file, false);
                }
            }
        }
    }

    /// Walk an expression, reporting undefined names and missing `!` markers.
    ///
    /// # Effect-marker propagation rule
    ///
    /// `in_effect` is true when an enclosing `!` / `do` licenses an effectful
    /// host call here. One `!` marks **one** call, so the flag propagates only
    /// through *transparent* syntax — forms that yield the value of a single
    /// inner expression without introducing a second computation:
    ///
    /// - `Group` — `! (@fs.write(p, s))`; parentheses change nothing.
    /// - `Try` — `! @fs.write(p, s)?`; `?` unwraps the result of that same call.
    /// - `Unary` / the `Call` callee — how `! @fs.write(…)` works at all.
    /// - The *primary operand* of a form whose other operand is a fallback or a
    ///   projection: `Coalesce.left`, `Pipeline.input`, `Member.object`,
    ///   `Index.object`. In `! @db.query(c, q)?.rows` the call is still the
    ///   subject of the expression; `.rows` only reads from its value.
    ///
    /// Everywhere else the flag resets to false, because a `!` on an outer
    /// expression must not silently license an effectful call buried in an
    /// unrelated subexpression. That means call **arguments**
    /// (`! @console.println(@fs.read(p))` needs its own `!` on the read), both
    /// `Binary` operands (`! a + b` names no single call), `Coalesce.right`,
    /// `Index.index`, pipeline stages, and every block or branch body.
    ///
    /// `!` binds tighter than `??`, `→` and the binary operators (it is parsed
    /// in `parse_unary`), so `! f() ?? d` already parses as
    /// `Coalesce(Unary(Effect, …), d)` and does not depend on propagation. The
    /// `Coalesce.left` / `Pipeline.input` cases exist for the parenthesised
    /// form, `! (@fs.read(p)? ?? "")`.
    fn resolve_expr(&mut self, expr: &Expr, file: rite_core::FileId, in_effect: bool) {
        match expr {
            Expr::Ident(i) => {
                if i.name != "_"
                    && !i.name.starts_with("__") // internal desugar symbols
                    && self.lookup(&i.name).is_none()
                    // `functions` already holds every BUILTIN_NAMES entry (see `new`).
                    && !self.functions.contains_key(&i.name)
                {
                    // Strict undefined-name check when statically knowable
                    self.diagnostics.push(simple_error(
                        E020_UNDEFINED_NAME,
                        format!("undefined name `{}`", i.name),
                        file,
                        i.span,
                        "not found in scope",
                    ));
                }
            }
            // A bare capability reference is not inert: the evaluator invokes it, so
            // `n ← @clock.now` really did read the clock — and skipping the check here
            // meant every zero-arity effectful capability could be used with no marker
            // at all. The resolver and the evaluator have to agree on what evaluating
            // this expression does, so it takes a marker exactly like a call.
            //
            // Pure capabilities are unaffected, which is what keeps `use @http.log` and
            // `⊏ @http.recover` (middleware markers, `effectful: false`) working.
            Expr::Capability(c) => {
                let path = c.path.join(".");
                if is_effectful(&path) {
                    self.note_effect();
                }
                if is_effectful(&path) && !in_effect {
                    self.diagnostics.push(
                        simple_error(
                            E021_EFFECT_REQUIRED,
                            "effectful capability requires `!`",
                            file,
                            c.span,
                            "evaluating this performs an external effect",
                        )
                        .with_help(format!("mark it as an explicit effect: ! @{}", path)),
                    );
                }
            }
            Expr::Call(c) => {
                // `! @fs.write(…)` can attach the marker to the callee rather
                // than to the whole call, depending on how it was written.
                let effect = in_effect || has_effect_marker(c.callee.as_ref());
                if let Expr::Ident(callee) = strip_effect(c.callee.as_ref()) {
                    self.note_call(&callee.name);
                    // Calling something declared effectful needs a marker, exactly
                    // as calling the capability directly would. Driven by the
                    // declaration rather than by inference: the contract is what a
                    // caller can see, and it is known before any body is walked.
                    let declared = self
                        .functions
                        .get(&callee.name)
                        .map(|m| m.declares_effect)
                        .unwrap_or(false);
                    if declared {
                        self.note_effect();
                    }
                    if declared && !effect {
                        self.diagnostics.push(
                            simple_error(
                                E021_EFFECT_REQUIRED,
                                format!("calling `{}` requires `!`", callee.name),
                                file,
                                c.span,
                                "this function performs an external effect",
                            )
                            .with_help(format!("mark the call: ! {}(…)", callee.name)),
                        );
                    }
                }
                if let Expr::Capability(cap) = strip_effect(c.callee.as_ref()) {
                    let path = cap.path.join(".");
                    if is_effectful(&path) {
                        self.note_effect();
                    }
                    if is_effectful(&path) && !effect {
                        self.diagnostics.push(
                            simple_error(
                                E021_EFFECT_REQUIRED,
                                "effectful capability call requires `!`",
                                file,
                                c.span,
                                "this operation performs an external effect",
                            )
                            .with_help(format!(
                                "mark the operation as an explicit effect: ! @{}",
                                path
                            )),
                        );
                    }
                }
                // A capability callee needs no further resolution (its path is just
                // idents) and the check above already owns this call's diagnostic —
                // recursing would report the same effect twice.
                if !matches!(strip_effect(c.callee.as_ref()), Expr::Capability(_)) {
                    self.resolve_expr(&c.callee, file, effect);
                }
                // Arguments are independent computations: reset.
                // Passing an effectful function to another one runs it, so the
                // call performs effects: `each(shout)` writes to the terminal
                // however pure `each` itself is, and nothing on that line says so.
                //
                // Only a *named* effectful function counts. An inline lambda that
                // performs effects already carries its own `!` in plain sight at
                // the call site, so demanding a second marker around it would add
                // noise without adding information. A closure stored in a binding
                // and passed later is not tracked, and cannot be without types.
                for a in &c.args {
                    if let Expr::Ident(arg) = a {
                        let passes_effect = self
                            .functions
                            .get(&arg.name)
                            .map(|m| m.declares_effect)
                            .unwrap_or(false);
                        if passes_effect && !effect {
                            self.note_effect();
                            self.diagnostics.push(
                                simple_error(
                                    E021_EFFECT_REQUIRED,
                                    format!("passing `{}` here requires `!` on the call", arg.name),
                                    file,
                                    c.span,
                                    "the function passed performs an external effect when run",
                                )
                                .with_help("mark the call, or the whole pipeline: ! (… → …)"),
                            );
                        }
                    }
                    self.resolve_expr(a, file, false);
                }
            }
            Expr::Unary(u) => {
                // `!` licenses the operand; `-`/`not` carry the ambient context
                // into their single operand unchanged.
                let eff = u.op == UnaryOp::Effect || in_effect;
                self.resolve_expr(&u.expr, file, eff);
            }
            // Neither operand is *the* subject of `a + b`: both reset.
            Expr::Binary(b) => {
                self.resolve_expr(&b.left, file, false);
                self.resolve_expr(&b.right, file, false);
            }
            Expr::Pipeline(p) => {
                // The input is the head computation; each stage is its own — but a
                // marker on the whole pipeline covers all of it, or `! (xs → each
                // (shout))` would have no way to be written.
                self.resolve_expr(&p.input, file, in_effect);
                for s in &p.stages {
                    self.resolve_expr(s, file, in_effect);
                }
            }
            Expr::If(i) => {
                self.resolve_expr(&i.condition, file, false);
                self.resolve_block(&i.then_branch, file);
                if let Some(e) = &i.else_branch {
                    self.resolve_block(e, file);
                }
            }
            Expr::Match(m) => {
                self.resolve_expr(&m.scrutinee, file, false);
                for arm in &m.arms {
                    self.push_scope();
                    self.define_pattern(&arm.pattern, false, file);
                    self.resolve_expr(&arm.body, file, false);
                    self.pop_scope();
                }
                if !m.arms.is_empty() && !covers_all_inputs(&m.arms) {
                    self.diagnostics.push(
                        rite_core::Diagnostic::warning(
                            E029_NON_EXHAUSTIVE_MATCH,
                            "match may not cover every input",
                        )
                        .with_primary(
                            rite_core::SourceSpan::new(file, m.span),
                            "no arm matches unconditionally; an unmatched value fails at runtime",
                        )
                        .with_help("add `_ → …` (or a binding arm) to handle the rest"),
                    );
                }
            }
            Expr::Block(b) => self.resolve_block(b, file),
            Expr::List(l) => {
                for e in &l.elements {
                    self.resolve_expr(e, file, false);
                }
            }
            Expr::Record(r) => {
                for e in &r.entries {
                    self.resolve_expr(&e.value, file, false);
                }
            }
            // `.field` projects from the object; the object stays the subject.
            Expr::Member(m) => {
                // `math.square` is a call into a module, not a field read. Desugar
                // rewrites it to the global `math__square`; if that does not exist
                // the program used to fail at runtime with the mangled name in the
                // message. Check it here, and say it in the source's own terms.
                if let Expr::Ident(obj) = m.object.as_ref() {
                    if self.is_module_qualifier(&obj.name) {
                        let mangled = format!("{}__{}", obj.name, m.field.name);
                        if !self.functions.contains_key(&mangled) {
                            let mut exports: Vec<&str> = self
                                .functions
                                .keys()
                                .filter_map(|k| k.strip_prefix(&format!("{}__", obj.name)))
                                .collect();
                            exports.sort_unstable();
                            let help = if exports.is_empty() {
                                format!("`{}` exports nothing public", obj.name)
                            } else {
                                format!("`{}` exports: {}", obj.name, exports.join(", "))
                            };
                            self.diagnostics.push(
                                rite_core::Diagnostic::error(
                                    E020_UNDEFINED_NAME,
                                    format!(
                                        "module `{}` has no public `{}`",
                                        obj.name, m.field.name
                                    ),
                                )
                                .with_primary(
                                    rite_core::SourceSpan::new(file, m.span),
                                    "not exported by that module",
                                )
                                .with_help(help),
                            );
                        }
                        return;
                    }
                }
                self.resolve_expr(&m.object, file, in_effect)
            }
            Expr::Index(i) => {
                self.resolve_expr(&i.object, file, in_effect);
                // The subscript is an independent computation.
                self.resolve_expr(&i.index, file, false);
            }
            // `?` unwraps the result of the very call the `!` marks.
            Expr::Try(t) => self.resolve_expr(&t.expr, file, in_effect),
            Expr::Coalesce(c) => {
                // `x ?? fallback`: `x` is the subject, the fallback is not.
                self.resolve_expr(&c.left, file, in_effect);
                self.resolve_expr(&c.right, file, false);
            }
            Expr::HttpListen(h) => {
                self.resolve_expr(&h.addr, file, false);
                self.resolve_block(&h.body, file);
            }
            Expr::Route(r) => {
                self.push_scope();
                for p in &r.params {
                    self.define(&p.name.name, false, p.span, file);
                }
                self.resolve_block(&r.body, file);
                self.pop_scope();
            }
            // Parentheses are transparent.
            Expr::Group(g) => self.resolve_expr(&g.expr, file, in_effect),
            Expr::Literal(_) | Expr::Atom(_) | Expr::Placeholder(_) => {}
        }
    }

    fn define_pattern(&mut self, pat: &Pattern, mutable: bool, file: rite_core::FileId) {
        match pat {
            Pattern::Ident(i) => {
                self.define(&i.name, mutable, i.span, file);
            }
            Pattern::List(l) => {
                for e in &l.elements {
                    self.define_pattern(e, mutable, file);
                }
                if let Some(r) = &l.rest {
                    self.define_pattern(r, mutable, file);
                }
            }
            Pattern::Record(r) => {
                for f in &r.fields {
                    if let Some(p) = &f.pattern {
                        self.define_pattern(p, mutable, file);
                    } else {
                        self.define(&f.name.name, mutable, f.span, file);
                    }
                }
            }
            Pattern::Result(r) => {
                if let Some(b) = &r.binding {
                    self.define_pattern(b, mutable, file);
                }
            }
            Pattern::Typed(t) => self.define_pattern(&t.pattern, mutable, file),
            Pattern::Atom(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => {}
        }
    }

    fn define(&mut self, name: &str, mutable: bool, span: Span, file: rite_core::FileId) {
        if name == "_" {
            return;
        }
        let local = LocalId(self.next_local);
        self.next_local += 1;
        let scope = self.scopes.last_mut().unwrap();
        if scope.bindings.contains_key(name) {
            self.diagnostics.push(simple_error(
                E022_DUPLICATE_BINDING,
                format!("duplicate binding `{}`", name),
                file,
                span,
                "already bound in this scope",
            ));
        }
        scope.bindings.insert(
            name.to_string(),
            BindingInfo {
                local,
                mutable,
                span,
            },
        );
    }

    /// True when this name refers to an imported module rather than a value.
    ///
    /// The import binds its qualifier in the root scope, so a binding of the same
    /// name in any inner scope shadows it — `◆ f(math) ⟦ ^ math.x ⟧` reads a field
    /// of the parameter, whatever `use math` did at the top of the file.
    fn is_module_qualifier(&self, name: &str) -> bool {
        if !self.import_qualifiers.contains(name) {
            return false;
        }
        !self
            .scopes
            .iter()
            .skip(1)
            .any(|scope| scope.bindings.contains_key(name))
    }

    fn lookup(&self, name: &str) -> Option<&BindingInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.bindings.get(name) {
                return Some(b);
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

/// Does calling the host function at `path` (e.g. `"fs.read"`) require `!`?
///
/// Looks the path up in [`HOST_EFFECTS`], the single source of truth. Paths
/// that are not in the table — a typo, or a capability an embedder registered
/// at runtime — are not diagnosed here; an unknown capability is reported as
/// `E042` when it is called.
pub fn is_effectful(path: &str) -> bool {
    HOST_EFFECTS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, effectful)| *effectful)
        .unwrap_or(false)
}

/// Can this arm set match every possible scrutinee?
///
/// Rite is dynamically typed, so exhaustiveness is only provable in two ways:
/// an arm that matches unconditionally, or an arm set that saturates one of
/// the finite value domains. Anything else can fall through to
/// `match failure: no arm matched` at runtime, which is what E029 warns about.
fn covers_all_inputs(arms: &[rite_syntax::MatchArm]) -> bool {
    if arms.iter().any(|a| is_irrefutable(&a.pattern)) {
        return true;
    }
    // `true` / `false` — the whole boolean domain.
    let mut saw_true = false;
    let mut saw_false = false;
    // `ok` / `err` and `some` / `none` — the whole result and option domains.
    let mut saw_ok = false;
    let mut saw_err = false;
    let mut saw_some = false;
    let mut saw_none = false;
    let mut saw_lit_none = false;
    for arm in arms {
        match &arm.pattern {
            Pattern::Literal(l) => match l.kind {
                LitKind::Bool(true) => saw_true = true,
                LitKind::Bool(false) => saw_false = true,
                LitKind::None => saw_lit_none = true,
                _ => {}
            },
            Pattern::Result(r) => match r.kind {
                ResultPatKind::Ok => saw_ok = true,
                ResultPatKind::Err => saw_err = true,
                ResultPatKind::Some => saw_some = true,
                ResultPatKind::None => saw_none = true,
            },
            _ => {}
        }
    }
    (saw_true && saw_false) || (saw_ok && saw_err) || (saw_some && (saw_none || saw_lit_none))
}

/// A pattern that matches any value, binding nothing (`_`) or everything (`x`).
///
/// Every other pattern is refutable: literals and atoms compare, lists check
/// length, records check fields, `ok`/`err` check the result tag, and a typed
/// pattern checks the type.
fn is_irrefutable(pat: &Pattern) -> bool {
    matches!(pat, Pattern::Wildcard(_) | Pattern::Ident(_))
}

fn strip_effect(expr: &Expr) -> &Expr {
    match expr {
        Expr::Unary(u) if u.op == UnaryOp::Effect => strip_effect(&u.expr),
        other => other,
    }
}

fn has_effect_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Unary(u) if u.op == UnaryOp::Effect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_core::{FileId, Severity};
    use rite_syntax::parse_file;

    /// Parse and resolve `src`, returning only the resolver's diagnostics.
    fn check(src: &str) -> Diagnostics {
        let file = SourceFile::new(FileId(0), "test.rite", src);
        let (program, pdiags) = parse_file(&file);
        let program = program.expect("parse failed");
        assert!(
            !pdiags.has_errors(),
            "test source does not parse:\n{:#?}",
            pdiags.into_vec()
        );
        resolve(&program, &file).1
    }

    fn codes(src: &str) -> Vec<String> {
        check(src)
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    }

    #[track_caller]
    fn assert_ok(src: &str) {
        let diags = check(src);
        assert!(
            !diags.has_errors(),
            "expected `{}` to check cleanly, got {:#?}",
            src.trim(),
            diags.into_vec()
        );
    }

    #[track_caller]
    fn assert_effect_error(src: &str) {
        let diags = check(src);
        assert!(
            diags
                .iter()
                .any(|d| d.code == E021_EFFECT_REQUIRED && d.severity == Severity::Error),
            "expected E021 for `{}`, got {:#?}",
            src.trim(),
            diags.into_vec()
        );
    }

    // ---- data declarations -------------------------------------------------

    #[test]
    fn data_declaration_binds_its_name() {
        // `Item::Data` used to be skipped entirely, so any use of the name was E020
        // even though desugar emitted a binding for it.
        assert_ok("◆ Cfg ⟨a: 1, b: \"x\"⟩\n! @console.println(str(Cfg))");
        assert_ok("def Cfg <<n: 5>>\ndef f() [[ ^ Cfg.n * 2 ]]\n! @console.println(str(f()))");
    }

    #[test]
    fn data_declaration_fields_are_still_resolved() {
        assert_eq!(
            codes("◆ Cfg ⟨a: missing_name⟩"),
            vec![E020_UNDEFINED_NAME.to_string()]
        );
    }

    #[test]
    fn data_declaration_is_bound_in_order_not_hoisted() {
        // Desugar emits the binding as a top-level statement, so a use above the
        // declaration has no value yet. Reject it here rather than letting it reach
        // the runtime as "undefined name" — functions hoist, records do not.
        assert_eq!(
            codes("! @console.println(str(Later))\n◆ Later ⟨v: 7⟩"),
            vec![E020_UNDEFINED_NAME.to_string()]
        );
    }

    // ---- the effect table --------------------------------------------------

    #[test]
    fn effect_table_is_exact_lookup_not_prefix_match() {
        assert!(is_effectful("console.println"));
        assert!(is_effectful("fs.write"));
        // The old catch-all `path.starts_with("console.")` classified names
        // that no capability registers.
        assert!(!is_effectful("console.printlnx"));
        assert!(!is_effectful("fs.write.extra"));
        assert!(!is_effectful("unknown.thing"));
    }

    #[test]
    fn effect_table_has_no_duplicate_entries() {
        let mut seen = std::collections::HashSet::new();
        for (path, _) in HOST_EFFECTS {
            assert!(seen.insert(*path), "duplicate HOST_EFFECTS entry {}", path);
        }
    }

    #[test]
    fn effect_table_paths_are_cap_dot_function() {
        for (path, _) in HOST_EFFECTS {
            assert_eq!(
                path.split('.').count(),
                2,
                "HOST_EFFECTS path `{}` is not `cap.function`",
                path
            );
        }
    }

    #[test]
    fn db_calls_now_require_a_marker() {
        // Regression: `@db.*` was absent from the resolver's list entirely, so
        // this checked clean despite `effectful: true` on every db descriptor.
        assert_effect_error("conn ← @db.open()?\n");
        assert_effect_error("r ← @db.exec(conn, \"CREATE TABLE t(a INT)\")?\n");
        assert_ok("conn ← ! @db.open()?\n");
    }

    #[test]
    fn reads_are_effects_too() {
        assert_effect_error("s ← @fs.read(\"./x.txt\")?\n");
        assert_effect_error("rows ← @csv.read(\"./x.csv\")?\n");
        assert_effect_error("v ← @env.get(\"HOME\")\n");
        assert_ok("s ← ! @fs.read(\"./x.txt\")?\n");
    }

    #[test]
    fn pure_host_helpers_need_no_marker() {
        assert_ok("t ← @json.encode(⟨a: 1⟩)\n");
        assert_ok("r ← @csv.decode(\"a,b\")?\n");
        // In-process capability state: reads are unmarked, writes are not.
        assert_ok("v ← @store.get(\"k\")\n");
        assert_effect_error("v ← @store.set(\"k\", 1)\n");
    }

    #[test]
    fn a_bare_capability_reference_is_a_call() {
        // This test used to assert the opposite — that naming a host function performs
        // no effect — but capability references are not first-class values. The
        // evaluator invokes a bare mention as a zero-argument call, so
        //
        //     f ← @clock.now
        //
        // binds the *timestamp*, not the function: `f()` afterwards fails with "cannot
        // call value of type string". While the resolver believed a bare mention was
        // inert, every zero-arity effectful capability could be used with no marker at
        // all, and `f ← @fs.write` silently called `fs.write`.
        assert_effect_error("f ← @fs.write\n");
        assert_effect_error("n ← @clock.now\n");
        assert_ok("n ← ! @clock.now\n");
        // Pure capabilities are still free to name — this is what `use @http.log` and
        // `⊏ @http.recover` rely on.
        assert_ok("m ← @http.log\n");
        assert_ok("r ← @http.response(200)\n");
    }

    // ---- effect-marker propagation (one test per arm) -----------------------

    #[test]
    fn marker_survives_try() {
        // The bug: `Try` hardcoded `false`, so the `!` right there was lost.
        assert_ok("! @fs.write(\"./o.txt\", \"hi\")?\n");
        assert_ok("conn ← ! @db.open()?\nn ← ! @db.query(conn, \"SELECT 1\")?\n");
    }

    #[test]
    fn marker_survives_parentheses() {
        assert_ok("! (@fs.write(\"./o.txt\", \"hi\"))\n");
        assert_ok("! (@fs.write(\"./o.txt\", \"hi\")?)\n");
    }

    #[test]
    fn try_outside_the_marker_still_works() {
        // Proves the marker itself was never the problem.
        assert_ok("(! @fs.write(\"./o.txt\", \"hi\"))?\n");
    }

    #[test]
    fn marker_reaches_the_primary_operand_of_coalesce() {
        // `!` binds tighter than `??`, so this parses as Coalesce(Unary, d) …
        assert_ok("s ← ! @fs.read(\"./x.txt\") ?? \"default\"\n");
        // … and the parenthesised form relies on Coalesce.left propagating.
        assert_ok("s ← ! (@fs.read(\"./x.txt\") ?? \"default\")\n");
    }

    #[test]
    fn marker_does_not_leak_into_a_coalesce_fallback() {
        assert_effect_error("s ← ! (\"x\" ?? @fs.read(\"./x.txt\"))\n");
    }

    #[test]
    fn marker_reaches_a_pipeline_input_but_not_its_stages() {
        assert_ok("n ← ! (@fs.read(\"./x.txt\")? → lines)\n");
        assert_effect_error("n ← ! ([1] → map { |x| @fs.read(\"./x.txt\") })\n");
    }

    #[test]
    fn marker_reaches_a_member_or_index_object_only() {
        assert_ok("n ← ! @fs.metadata(\"./x.txt\")?.size\n");
        assert_ok("n ← ! @fs.glob(\"*.txt\")?[0]\n");
        // The subscript is its own computation.
        assert_effect_error("n ← ! ([1][@env.get(\"I\")])\n");
    }

    #[test]
    fn marker_does_not_leak_across_a_binary_operator() {
        // Neither operand is *the* marked call, so both need their own marker.
        assert_effect_error("n ← ! (1 + @clock.now())\n");
        assert_ok("n ← 1 + ! @clock.now()\n");
    }

    #[test]
    fn marker_does_not_leak_into_call_arguments() {
        // A `!` on the outer call must not license an unrelated inner effect.
        assert_effect_error("! @console.println(@fs.read(\"./x.txt\"))\n");
        assert_ok("! @console.println(! @fs.read(\"./x.txt\"))\n");
    }

    #[test]
    fn marker_does_not_leak_into_a_block_or_branch() {
        assert_effect_error("n ← ! (if true ⟦ @clock.now() ⟧ else ⟦ 0 ⟧)\n");
        assert_ok("n ← ! (if true ⟦ ! @clock.now() ⟧ else ⟦ 0 ⟧)\n");
    }

    #[test]
    fn an_unmarked_effectful_call_is_still_an_error() {
        // The control case: no `!` anywhere.
        assert_effect_error("@fs.write(\"./o.txt\", \"hi\")?\n");
        assert_effect_error("@console.println(\"x\")\n");
    }

    // ---- E029 exhaustiveness ----------------------------------------------

    #[test]
    fn e029_fires_when_no_arm_matches_unconditionally() {
        // Previously silent: the check only ran when *every* arm was an atom,
        // so a mixed literal/atom match warned about nothing.
        assert!(codes("m ← ~ x ⟦\n  1 → \"a\"\n  #b → \"b\"\n⟧\n").contains(&"E029".to_string()));
        assert!(codes("m ← ~ x ⟦\n  #a → 1\n  #b → 2\n⟧\n").contains(&"E029".to_string()));
    }

    #[test]
    fn e029_is_silent_when_an_arm_matches_unconditionally() {
        assert!(!codes("m ← ~ x ⟦\n  #a → 1\n  _ → 0\n⟧\n").contains(&"E029".to_string()));
        // A bare binding arm is just as total as `_`.
        assert!(!codes("m ← ~ x ⟦\n  #a → 1\n  other → other\n⟧\n").contains(&"E029".to_string()));
    }

    #[test]
    fn e029_is_silent_for_a_saturated_finite_domain() {
        // Exhaustive without a wildcard — warning here would be noise.
        assert!(!codes("m ← ~ b ⟦\n  true → 1\n  false → 0\n⟧\n").contains(&"E029".to_string()));
        assert!(
            !codes("m ← ~ r ⟦\n  ok v → v\n  err e → 0\n⟧\n").contains(&"E029".to_string()),
            "ok/err covers the result domain"
        );
    }

    #[test]
    fn e029_is_a_warning_not_an_error() {
        let diags = check("m ← ~ x ⟦\n  #a → 1\n⟧\n");
        let e029: Vec<_> = diags
            .iter()
            .filter(|d| d.code == E029_NON_EXHAUSTIVE_MATCH)
            .collect();
        assert_eq!(e029.len(), 1, "{:#?}", diags.iter().collect::<Vec<_>>());
        assert_eq!(e029[0].severity, Severity::Warning);
    }

    // ---- builtin names ----------------------------------------------------

    #[test]
    fn builtins_resolve_without_a_second_list() {
        // `is_builtin_name` used to duplicate this list and disagree with it.
        let r = Resolver::new();
        for name in BUILTIN_NAMES {
            assert!(
                r.functions.contains_key(*name),
                "builtin `{}` is not predefined",
                name
            );
        }
        assert_ok("n ← [1, 2] → count\n");
    }

    #[test]
    fn every_builtin_name_lexes_as_one_identifier() {
        // `number?` was listed and unreachable: the lexer splits the `?` off,
        // so the resolver looked up `number` and reported it undefined.
        for name in BUILTIN_NAMES {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "builtin `{}` cannot lex as a single identifier, so it can never resolve",
                name
            );
        }
    }

    #[test]
    fn resolver_default_matches_new() {
        assert_eq!(Resolver::default().functions.len(), BUILTIN_NAMES.len());
    }
}
