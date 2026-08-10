//! Lexical resolver: scopes, bindings, effect checks, imports.

use crate::ir::*;
use rite_core::{
    simple_error, Diagnostics, SourceFile, Span, E020_UNDEFINED_NAME, E021_EFFECT_REQUIRED,
    E022_DUPLICATE_BINDING, E023_IMMUTABLE_ASSIGN, E029_NON_EXHAUSTIVE_MATCH,
    E042_UNKNOWN_CAPABILITY,
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
/// Columns: path, effectful (needs `!`), returns a result value (so `?`
/// applies). Cross-validated against the capability descriptors in both
/// directions by `crates/rite-caps/tests/effect_parity.rs`.
pub const HOST_EFFECTS: &[(&str, bool, bool)] = &[
    // @stdin — the process's own input. Reads are effects.
    ("stdin.read", true, false),
    ("stdin.lines", true, false),
    // @console — the terminal.
    ("console.print", true, false),
    ("console.println", true, false),
    ("console.warn", true, false),
    ("console.error", true, false),
    ("console.inspect", true, false),
    ("console.read_line", true, false),
    // @fs — the filesystem. Reads observe it; writes change it.
    ("fs.read", true, true),
    ("fs.read_bytes", true, true),
    ("fs.write", true, true),
    ("fs.append", true, true),
    ("fs.lines", true, true),
    ("fs.exists", true, false),
    ("fs.metadata", true, true),
    ("fs.glob", true, true),
    ("fs.mkdir", true, true),
    ("fs.remove", true, true),
    ("fs.copy", true, true),
    ("fs.move", true, true),
    // Open handles. Every one touches the file behind the handle — even `close`,
    // which flushes — so every one takes a marker.
    ("fs.open", true, true),
    ("fs.read_chunk", true, true),
    ("fs.read_line", true, true),
    ("fs.write_chunk", true, true),
    ("fs.seek", true, true),
    ("fs.flush", true, true),
    ("fs.close", true, true),
    // @json — encode/decode are pure string transforms; read/write touch disk.
    // @regex — pure text transforms; a pattern is data.
    ("regex.is_match", false, true),
    ("regex.find", false, true),
    ("regex.find_all", false, true),
    ("regex.captures", false, true),
    ("regex.replace", false, true),
    ("regex.split", false, true),
    ("json.decode", false, true),
    ("json.encode", false, false),
    ("json.encode_pretty", false, false),
    ("json.read", true, true),
    ("json.write", true, true),
    // @csv — as @json.
    ("csv.decode", false, true),
    ("csv.encode", false, true),
    ("csv.read", true, true),
    ("csv.write", true, true),
    // @crypto — value transforms. A digest, an HMAC and an encoding are functions
    // of their arguments alone: the same input gives the same answer on every run
    // and nothing outside the program is touched, so they take no marker. Only
    // `random_bytes` reads the OS entropy pool, which is state outside the program
    // and different every call — effectful for the same reason `@clock.now` is.
    ("crypto.sha256", false, false),
    ("crypto.sha512", false, false),
    ("crypto.hmac_sha256", false, false),
    ("crypto.random_bytes", true, false),
    ("crypto.constant_time_eq", false, false),
    ("crypto.base64_encode", false, false),
    ("crypto.base64_decode", false, true),
    ("crypto.hex_encode", false, false),
    ("crypto.hex_decode", false, true),
    // @clock — `now`/`sleep` consult the wall clock; the rest are value math.
    ("clock.now", true, false),
    ("clock.parse", false, true),
    ("clock.format", false, true),
    ("clock.add", false, true),
    ("clock.diff", false, true),
    ("clock.sleep", true, false),
    ("clock.duration", false, true),
    // @env — the process environment.
    ("env.get", true, false),
    ("env.require", true, true),
    ("env.all", true, false),
    ("env.set", true, false),
    // @sys — ambient facts about the process and the machine. All effectful:
    // none of them is constant for the life of a run, and a pure function that
    // answers differently on the second call is worse than an effectful one.
    ("sys.cwd", true, false),
    ("sys.home", true, false),
    ("sys.temp_dir", true, false),
    ("sys.os", true, false),
    ("sys.arch", true, false),
    ("sys.pid", true, false),
    ("sys.hostname", true, false),
    // @process — subprocesses. `which` probes PATH and the filesystem.
    ("process.run", true, true),
    ("process.which", true, true),
    // `args` observes what the invoker typed — outside the program, and different
    // between runs, so it takes a marker for the same reason `@clock.now` does.
    ("process.args", true, false),
    // `exit` ends the process. Nothing is more effectful than that.
    ("process.exit", true, false),
    // @random — the entropy source (`seed` mutates it).
    ("random.int", true, false),
    ("random.float", true, false),
    ("random.choose", true, false),
    ("random.shuffle", true, false),
    ("random.uuid", true, false),
    ("random.seed", true, false),
    // @http — `listen` binds a socket. The others build values: `response`
    // is a record constructor, `log`/`recover` are middleware markers named
    // in `use @http.log` rather than called for their effect.
    ("http.listen", true, false),
    // Outbound requests reach the network.
    ("http.get", true, true),
    ("http.post", true, true),
    ("http.request", true, true),
    ("http.response", false, false),
    ("http.file", true, true),
    ("http.log", false, false),
    ("http.recover", false, false),
    // @mcp — `serve` claims a transport (stdin/stdout, or a socket) and then runs
    // script bodies for whoever is on the other end. `progress` writes a notification
    // onto that transport. `log` is a marker named in `use @mcp.log`, and
    // `tool_schema` is a pure derivation from a function's declared types.
    ("mcp.serve", true, false),
    ("mcp.progress", true, false),
    ("mcp.log", false, false),
    ("mcp.tool_schema", false, false),
    // The client half. `connect` starts a subprocess or reaches a host; every call
    // that takes its handle asks another program a question and reads the answer,
    // which is an observation of state outside this one.
    ("mcp.connect", true, true),
    ("mcp.tools", true, true),
    ("mcp.call_tool", true, true),
    ("mcp.resources", true, true),
    ("mcp.read_resource", true, true),
    ("mcp.prompts", true, true),
    ("mcp.get_prompt", true, true),
    ("mcp.close", true, true),
    // @udp — datagram sockets. Every one of these touches the socket: `bind` claims
    // a port, `local_addr` asks the OS which one it got, and the two transfers move
    // bytes on and off the wire.
    ("udp.bind", true, true),
    ("udp.local_addr", true, true),
    ("udp.send_to", true, true),
    ("udp.recv_from", true, true),
    ("udp.close", true, true),
    // @tcp — byte streams. `connect` dials out, `listen` claims a port and then
    // serves, and the two transfers move bytes on and off the wire. `close` releases
    // a file descriptor. There is nothing pure here.
    ("tcp.connect", true, true),
    ("tcp.send", true, true),
    ("tcp.recv", true, true),
    ("tcp.peer_addr", true, true),
    ("tcp.local_addr", true, true),
    ("tcp.close", true, true),
    ("tcp.listen", true, false),
    // @game — in-process world state: writes marked, reads not.
    ("game.register_item", true, false),
    ("game.register_room", true, false),
    ("game.register_world", true, false),
    ("game.say", true, false),
    ("game.reveal", true, false),
    ("game.go", true, false),
    ("game.take", true, false),
    ("game.drop", true, false),
    ("game.look", false, false),
    ("game.inventory", false, false),
    ("game.save", false, true),
    ("game.load", true, true),
    ("game.start", true, false),
    ("game.command", true, false),
    ("game.messages", false, false),
    ("game.state", false, false),
    // @store — in-process key/value state: writes marked, reads not.
    ("store.get", false, true),
    ("store.set", true, true),
    ("store.delete", true, true),
    // @proto — protobuf. Building a schema hands out a handle and so changes
    // state the capability owns; `load_file` also reads the disk. Decoding and
    // encoding are functions of the handle passed to them, and a pool never
    // changes once built, so they are pure in the way `@json.decode` is.
    ("proto.compile", true, true),
    ("proto.compile_all", true, true),
    ("proto.load_file", true, true),
    ("proto.load_set", true, true),
    ("proto.decode", false, true),
    ("proto.encode", false, true),
    ("proto.messages", false, true),
    // @db — an external database, including `query`.
    ("db.open", true, true),
    ("db.close", true, true),
    ("db.exec", true, true),
    ("db.query", true, true),
    ("db.prepare", true, true),
    ("db.query_prepared", true, true),
    ("db.exec_prepared", true, true),
    ("db.close_stmt", true, true),
    ("db.begin", true, true),
    ("db.commit", true, true),
    ("db.rollback", true, true),
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
    "sort_by",
    "min_by",
    "max_by",
    "unique",
    "zip",
    "chunk",
    "window",
    "flat_map",
    "partition",
    "take_while",
    "drop_while",
    "nth",
    "frequencies",
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
    "get",
    "has",
    "entries",
    "merge",
    "update",
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
    /// Names the entry's own imports bind (`use math as m` → `m`). Valid
    /// qualifiers anywhere in the file.
    pub import_qualifiers: HashSet<String>,
    /// Qualifiers the merged modules' imports bind — valid only inside
    /// `injected_functions` (see [`resolve_with_qualifiers`]). Desugar reads
    /// these instead of re-scanning the item list, which misses them.
    pub merged_qualifiers: HashSet<String>,
    /// Names of the function copies `merge_exports_into_entry` injected.
    pub injected_functions: HashSet<String>,
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
    /// Qualifiers bound by the *merged modules'* imports. Valid only inside
    /// `injected_functions`: the copies rely on them, but the entry never
    /// imported them, so entry code using one is still an undefined name.
    merged_qualifiers: HashSet<String>,
    /// Names of the function copies `merge_exports_into_entry` injected.
    injected_functions: HashSet<String>,
    /// Mangled names of injected copies of *private* module functions. They
    /// exist so a module's own exports can call them; qualified access from the
    /// entry (`helper.secret`) is refused against this set.
    private_injected: HashSet<String>,
    /// Which module each injected *bare* name came from, for diagnostics.
    injected_origin: HashMap<String, String>,
    /// Walking the body of an injected copy (or a function nested in one).
    in_injected_fn: bool,
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
    /// Call-like nodes seen while walking: calls, capability references, and
    /// pipeline stages. Snapshotting this around a `!` operand answers "could
    /// anything have happened in there at all?" — which is the question a marker
    /// over nothing has to be judged by, since whether a *particular* call is
    /// effectful is exactly what this analysis cannot always say.
    call_sites_seen: usize,
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
    /// This binding holds something that performs a host effect when called.
    ///
    /// Effect-ness travels along the call graph by *name*, so a function reached
    /// through a binding used to leave the graph entirely: `f ← shout` followed by
    /// `f("hi")` printed from a plain `◆`, with nothing to report it. Naming the
    /// value does not make it inert, so the binding carries the property forward
    /// and calling through it is checked exactly as calling `shout` would be.
    effectful_callable: bool,
}

pub fn resolve(program: &Program, file: &SourceFile) -> (ResolvedProgram, Diagnostics) {
    resolve_with_qualifiers(program, file, HashSet::new(), HashSet::new())
}

/// [`resolve`], with the qualifiers of merged module imports in scope.
///
/// The merged entry contains copies of every module's public functions, but not
/// the modules' own `use` items — so a body copied out of `outer.rite` referred
/// to a qualifier (`i.double`, `@i.double`) the entry never imported and failed
/// as an undefined name. `merge_exports_into_entry` returns those qualifiers
/// and the names of the copies; the qualifiers apply only inside those copies,
/// which is what keeps a module's imports out of the entry's own namespace.
pub fn resolve_with_qualifiers(
    program: &Program,
    file: &SourceFile,
    merged_qualifiers: HashSet<String>,
    injected_functions: HashSet<String>,
) -> (ResolvedProgram, Diagnostics) {
    resolve_with_qualifiers_and_predeclared(
        program,
        file,
        merged_qualifiers,
        injected_functions,
        HashSet::new(),
        HashMap::new(),
        &[],
    )
}

/// [`resolve_with_qualifiers`], with names the *host* has already bound.
///
/// `predeclared` names are defined in the global scope before the walk, so a use of
/// one is not E020. Nothing declares them in the source, and nothing evaluates them:
/// the host is promising it has put a value in the environment under that name.
///
/// The REPL is the caller. A binding that holds a host handle cannot be replayed as
/// source — re-running `c ← ! @mcp.connect(…)` starts a second server, and
/// `h ← ! @fs.open(…)` reopens the file at the top — and a handle has no literal to
/// stand in for it, so the value is carried across lines and named here instead.
pub fn resolve_with_qualifiers_and_predeclared(
    program: &Program,
    file: &SourceFile,
    merged_qualifiers: HashSet<String>,
    injected_functions: HashSet<String>,
    private_injected: HashSet<String>,
    injected_origin: HashMap<String, String>,
    predeclared: &[String],
) -> (ResolvedProgram, Diagnostics) {
    // Diagnostics are attributed to `program.file`, which the parser stamped from
    // the same `SourceFile` the caller passes here. Catch a mismatched pair in
    // debug builds rather than reporting spans against the wrong file.
    debug_assert_eq!(
        program.file, file.id,
        "resolve() called with a SourceFile that did not produce this Program"
    );
    let mut r = Resolver::new();
    r.merged_qualifiers = merged_qualifiers;
    r.injected_functions = injected_functions;
    r.private_injected = private_injected;
    r.injected_origin = injected_origin;
    for name in predeclared {
        r.define(name, false, Span::DUMMY, program.file);
    }
    r.resolve_program(program);
    r.infer_effects();
    let resolved = ResolvedProgram {
        ast: program.clone(),
        functions: r.functions.clone(),
        warnings: vec![],
        import_qualifiers: r.import_qualifiers.clone(),
        merged_qualifiers: r.merged_qualifiers.clone(),
        injected_functions: r.injected_functions.clone(),
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
            merged_qualifiers: HashSet::new(),
            injected_functions: HashSet::new(),
            private_injected: HashSet::new(),
            injected_origin: HashMap::new(),
            in_injected_fn: false,
            current_fn: None,
            file_for_effects: rite_core::FileId(0),
            direct_effects: HashSet::new(),
            effects_seen: 0,
            call_edges: HashMap::new(),
            call_sites_seen: 0,
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
                let bound = match &imp.alias {
                    Some(alias) => Some((alias.name.clone(), alias.span)),
                    None => imp
                        .path
                        .segments
                        .last()
                        .map(|seg| (seg.name.clone(), seg.span)),
                };
                if let Some((name, span)) = bound {
                    // A qualifier that names a capability namespace would make
                    // `@fs.read` mean the module or the host depending on an import
                    // line, so those names are reserved for the host.
                    if capability_namespaces().contains(name.as_str()) {
                        let help = if imp.alias.is_some() {
                            format!(
                                "pick an alias that is not a capability name: use … as {}2",
                                name
                            )
                        } else {
                            format!("alias the import: use {} as …", name)
                        };
                        self.diagnostics.push(
                            simple_error(
                                E022_DUPLICATE_BINDING,
                                format!(
                                    "importing a module as `{}` collides with the capability namespace `@{}`",
                                    name, name
                                ),
                                program.file,
                                span,
                                "this name belongs to a host capability",
                            )
                            .with_help(help),
                        );
                    }
                    self.define(&name, false, span, program.file);
                    self.import_qualifiers.insert(name);
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
        // Sticky: a `def` nested inside an injected copy came from the module
        // too, so the module's qualifiers keep holding inside it.
        let was_injected = self.in_injected_fn;
        self.in_injected_fn = was_injected || self.injected_functions.contains(&f.name.name);
        self.push_scope();
        for p in &f.params {
            self.define(&p.name.name, false, p.span, file);
        }
        self.resolve_block(&f.body, file);
        self.pop_scope();
        self.in_injected_fn = was_injected;
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
            // The sugar's `lowered` form is the semantic truth; the source
            // spelling exists for the formatter alone.
            Stmt::Sugared(s) => self.resolve_stmt(&s.lowered, file),
            Stmt::Binding(b) => {
                // Snapshot around the walk: for a lambda, the difference says whether
                // its *body* performs a host effect, which is the only way to know for
                // a function that has no name to look up. This is what the comment on
                // `effects_seen` has always described.
                let before = self.effects_seen;
                self.resolve_expr(&b.value, file, false);
                let effectful = match &b.value {
                    Expr::Block(_) => self.effects_seen > before,
                    other => self.names_effectful_callable(other),
                };
                self.define_pattern(&b.pattern, b.mutable, file);
                // Only a plain `f ← …` carries the mark. A destructuring pattern
                // binds parts of a value, and which part holds the callable is not
                // something this analysis can say.
                if effectful {
                    if let Pattern::Ident(i) = &b.pattern {
                        self.mark_effectful_callable(&i.name);
                    }
                }
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
                // `@cool.square` — module access through the sigil. Validated like
                // `cool.square` and evaluating it yields the function, so it needs
                // no marker: a *capability* ref is invoked, a module ref is not.
                if self.at_module_qualifier(&c.path) {
                    let (q, f) = (c.path[0].clone(), c.path[1].clone());
                    self.check_module_export(&q, &f, c.span, file);
                    return;
                }
                if self.unknown_capability(&c.path, c.span, file) {
                    return;
                }
                self.call_sites_seen += 1;
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
                self.call_sites_seen += 1;
                // `! @fs.write(…)` can attach the marker to the callee rather
                // than to the whole call, depending on how it was written.
                let effect = in_effect || has_effect_marker(c.callee.as_ref());
                if let Expr::Ident(callee) = strip_effect(c.callee.as_ref()) {
                    self.note_call(&callee.name);
                    // Calling something declared effectful needs a marker, exactly
                    // as calling the capability directly would. Driven by the
                    // declaration rather than by inference: the contract is what a
                    // caller can see, and it is known before any body is walked.
                    // A binding holding an effectful function is checked like the
                    // function itself: `f ← shout` then `f("hi")` reaches the
                    // terminal exactly as `shout("hi")` does, and used to say
                    // nothing. A local shadows a global of the same name, so the
                    // binding is consulted first.
                    let declared = match self.lookup(&callee.name) {
                        Some(b) => b.effectful_callable,
                        None => self
                            .functions
                            .get(&callee.name)
                            .map(|m| m.declares_effect)
                            .unwrap_or(false),
                    };
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
                    if self.at_module_qualifier(&cap.path) {
                        // `@cool.square(…)` — a call into a module. Checked like the
                        // named call above: the mangled global's declaration is the
                        // contract, so an effectful export needs its marker here too.
                        let (q, f) = (cap.path[0].clone(), cap.path[1].clone());
                        if self.check_module_export(&q, &f, cap.span, file) {
                            let mangled = format!("{}__{}", q, f);
                            self.note_call(&mangled);
                            let declared = self
                                .functions
                                .get(&mangled)
                                .map(|m| m.declares_effect)
                                .unwrap_or(false);
                            if declared {
                                self.note_effect();
                            }
                            if declared && !effect {
                                self.diagnostics.push(
                                    simple_error(
                                        E021_EFFECT_REQUIRED,
                                        format!("calling `@{}.{}` requires `!`", q, f),
                                        file,
                                        c.span,
                                        "this function performs an external effect",
                                    )
                                    .with_help(format!("mark the call: ! @{}.{}(…)", q, f)),
                                );
                            }
                        }
                    } else if !self.unknown_capability(&cap.path, cap.span, file) {
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
                        // Same rule as the callee above: a binding that holds an
                        // effectful function passes one. `g ← shout` followed by
                        // `each(xs, g)` is the documented `each(shout)` case with a
                        // rename in front of it, and one line was enough to slip it.
                        let passes_effect = match self.lookup(&arg.name) {
                            Some(b) => b.effectful_callable,
                            None => self
                                .functions
                                .get(&arg.name)
                                .map(|m| m.declares_effect)
                                .unwrap_or(false),
                        };
                        // Recorded whether or not the marker is present, like the
                        // callee checks above. Nesting this inside `!effect` meant
                        // writing the `!` the diagnostic asks for suppressed the
                        // inference: the enclosing function was never marked
                        // effectful, so it was not required to be `◆!` and its own
                        // callers needed no marker. Complying with the discipline
                        // switched it off.
                        if passes_effect {
                            self.note_effect();
                        }
                        if passes_effect && !effect {
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
                if u.op == UnaryOp::Effect {
                    // A marker over an operand containing no call, no capability and
                    // no pipeline stage cannot be marking anything: `! 42` and
                    // `x ← ! (1 + 1)` were accepted in silence, so `!` could not be
                    // read as "something happens here" — the only reason to write it.
                    //
                    // The trap this closes is `println!("one")`. Statements split on
                    // expression boundaries, so that is *two* of them — a discarded
                    // reference to `println`, then `! "one"` — which checked clean and
                    // printed nothing. It is the first thing anyone arriving from Rust
                    // writes.
                    //
                    // Judged on whether anything was *called*, not on whether the call
                    // was effectful: which calls perform effects is exactly what this
                    // analysis cannot always say (see `effects.md`), so `! each(xs, f)`
                    // for a parameter `f` must stay legal. Erring here costs a missed
                    // stray marker; erring the other way rejects the responsible form.
                    let calls_before = self.call_sites_seen;
                    self.resolve_expr(&u.expr, file, eff);
                    if self.call_sites_seen == calls_before {
                        self.diagnostics.push(
                            simple_error(
                                E021_EFFECT_REQUIRED,
                                "`!` marks an expression that performs no effect",
                                file,
                                u.span,
                                "nothing here calls out of the program",
                            )
                            .with_help(
                                "remove the marker; `!` belongs on a host call, \
                                 and `println!(…)` is two statements, not a call",
                            ),
                        );
                    }
                } else {
                    self.resolve_expr(&u.expr, file, eff);
                }
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
                    // A stage applies something to the value even when it is
                    // written as a bare name, so it counts as a call site.
                    self.call_sites_seen += 1;
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
                    if let Pattern::Or(o) = &arm.pattern {
                        self.check_or_pattern_bindings(o, file);
                    }
                    self.define_pattern(&arm.pattern, false, file);
                    if let Some(guard) = &arm.guard {
                        self.resolve_expr(guard, file, false);
                    }
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
                        let obj_name = obj.name.clone();
                        self.check_module_export(&obj_name, &m.field.name, m.span, file);
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
            Expr::Try(t) => {
                self.check_try_target(&t.expr, file);
                self.resolve_expr(&t.expr, file, in_effect)
            }
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
            Expr::McpServe(m) => {
                // Unlike `@http.listen`, which escapes the effect check only because it
                // is not a `Call`, an MCP server is held to it: it binds a transport and
                // runs script bodies on demand for an outside caller, which is as
                // effectful as anything in the language gets. The check is here rather
                // than inherited, so the omission stays visible.
                if !in_effect {
                    self.diagnostics.push(simple_error(
                        E021_EFFECT_REQUIRED,
                        "`@mcp.serve` performs an effect",
                        file,
                        m.span,
                        "mark it as an explicit effect: ! @mcp.serve",
                    ));
                }
                // Serving *is* a call out of the program, so it counts as one for the
                // stray-marker check above. Without this, the `!` this construct now
                // requires would itself be reported as marking nothing — the two rules
                // would contradict each other and no spelling would check clean.
                self.call_sites_seen += 1;
                self.resolve_expr(&m.config, file, false);
                self.resolve_block(&m.body, file);
            }
            Expr::McpDecl(d) => {
                self.push_scope();
                for p in &d.params {
                    self.define(&p.name.name, false, p.span, file);
                }
                self.resolve_block(&d.body, file);
                self.pop_scope();
            }
            // Parentheses are transparent.
            Expr::Group(g) => self.resolve_expr(&g.expr, file, in_effect),
            Expr::Literal(lit) => self.check_interpolation_holes(lit, file),
            Expr::Atom(_) | Expr::Placeholder(_) => {}
        }
    }

    /// Reject interpolation holes that are not a dotted name path.
    ///
    /// A hole is expanded in desugar — after this walk — by splitting its text
    /// on `.` and fabricating a `Global`, so `"{twice(21)}"` built a global
    /// literally named `twice(21)` and died at runtime with ``undefined name
    /// `twice(21)` ``, a message that reads as a typo rather than a language
    /// limit. The same trap caught regex quantifiers: `"{2,3}"` is a hole
    /// named `2,3`. Both are now said plainly at check time.
    ///
    /// The scan mirrors `desugar_interpolation`'s brace rules: doubled braces
    /// are literal (raw strings arrive with every brace doubled), a lone `}`
    /// is literal, an unmatched `{` is literal.
    fn check_interpolation_holes(&mut self, lit: &rite_syntax::Literal, file: rite_core::FileId) {
        let rite_syntax::LitKind::String(s) = &lit.kind else {
            return;
        };
        let mut rest = s.as_str();
        while let Some(start) = rest.find(['{', '}']) {
            let brace = rest.as_bytes()[start];
            let after = &rest[start + 1..];
            if after.as_bytes().first() == Some(&brace) {
                rest = &after[1..];
                continue;
            }
            if brace == b'}' {
                rest = after;
                continue;
            }
            let Some(end) = after.find('}') else {
                rest = after;
                continue;
            };
            let hole = after[..end].trim();
            // What desugar's `parse_interp_expr` can actually expand: an
            // empty hole (renders as nothing), an atom, or a dotted name
            // path — its own trim included.
            let body = hole.strip_prefix('#').unwrap_or(hole);
            let is_path = hole.is_empty()
                || (!body.is_empty()
                    && body.split('.').all(|part| {
                        let mut chars = part.chars();
                        matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
                            && chars.all(|c| c.is_alphanumeric() || c == '_')
                    }));
            if !is_path {
                self.diagnostics.push(
                    simple_error(
                        E020_UNDEFINED_NAME,
                        format!("`{{{hole}}}` does not interpolate — a hole takes a name or a field path"),
                        file,
                        lit.span,
                        "only `{name}` and `{name.field}` are expanded",
                    )
                    .with_help(
                        "bind the value first (`v ← twice(21)` then `\"{v}\"`), build the \
                         string with `+` and `str(…)`, or use a raw string r\"…\" if the \
                         braces are literal",
                    ),
                );
            }
            rest = &after[end + 1..];
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
            // Alternatives bind the same names (checked before this is called),
            // so defining from the first covers whichever alternative matches.
            Pattern::Or(o) => {
                if let Some(first) = o.alternatives.first() {
                    self.define_pattern(first, mutable, file);
                }
            }
            Pattern::Atom(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => {}
        }
    }

    /// Every alternative of `1 | ok x | …` must bind the same names: the arm
    /// body reads them whichever alternative matched, so a name bound by only
    /// one alternative would be `none` on the others with no warning.
    fn check_or_pattern_bindings(&mut self, o: &rite_syntax::OrPattern, file: rite_core::FileId) {
        fn names(p: &Pattern, out: &mut Vec<String>) {
            match p {
                Pattern::Ident(i) => out.push(i.name.clone()),
                Pattern::List(l) => {
                    l.elements.iter().for_each(|e| names(e, out));
                    if let Some(r) = &l.rest {
                        names(r, out);
                    }
                }
                Pattern::Record(r) => {
                    for f in &r.fields {
                        match &f.pattern {
                            Some(p) => names(p, out),
                            None => out.push(f.name.name.clone()),
                        }
                    }
                }
                Pattern::Result(r) => {
                    if let Some(b) = &r.binding {
                        names(b, out);
                    }
                }
                Pattern::Or(o) => {
                    if let Some(first) = o.alternatives.first() {
                        names(first, out);
                    }
                }
                Pattern::Atom(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => {}
            }
        }
        let mut expected: Vec<String> = Vec::new();
        if let Some(first) = o.alternatives.first() {
            names(first, &mut expected);
        }
        expected.sort();
        for alt in o.alternatives.iter().skip(1) {
            let mut got = Vec::new();
            names(alt, &mut got);
            got.sort();
            if got != expected {
                self.diagnostics.push(
                    simple_error(
                        E020_UNDEFINED_NAME,
                        "or-pattern alternatives bind different names",
                        file,
                        o.span,
                        format!(
                            "this alternative binds [{}], the first binds [{}]",
                            got.join(", "),
                            expected.join(", ")
                        ),
                    )
                    .with_help("every `|` alternative must bind the same names, since the arm body runs whichever one matched"),
                );
                return;
            }
        }
    }

    fn define(&mut self, name: &str, mutable: bool, span: Span, file: rite_core::FileId) {
        if name == "_" {
            return;
        }
        // A top-level binding named like an imported function replaces that
        // function for every bare call in the file. scry-core hit this twice
        // (`pending`, `keep`); the failure surfaced as `cannot call value of
        // type int` at whichever call site ran next, forty lines from the
        // binding. The DUMMY-span guard keeps host-predeclared REPL names out.
        if self.scopes.len() == 1 && span != Span::DUMMY && self.injected_functions.contains(name) {
            let origin = self
                .injected_origin
                .get(name)
                .map(|m| format!("module `{}`", m))
                .unwrap_or_else(|| "an imported module".to_string());
            self.diagnostics.push(
                simple_error(
                    E022_DUPLICATE_BINDING,
                    format!(
                        "top-level binding `{}` collides with a function imported from {}",
                        name, origin
                    ),
                    file,
                    span,
                    "this binding would replace the imported function",
                )
                .with_help(format!(
                    "rename the binding, or keep the module behind a qualifier with `use … as …`; \
                     calls to `{}` after this line would find this value instead of the function",
                    name
                )),
            );
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
                effectful_callable: false,
            },
        );
    }

    /// Record that `name`'s binding holds an effectful callable.
    ///
    /// Applied after `define_pattern`, so a destructuring pattern simply does not
    /// get the mark — `⟨go: f⟩ ← r` binds `f` from a field this analysis cannot
    /// follow, and claiming to know what is in it would be worse than admitting
    /// the gap.
    fn mark_effectful_callable(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            if let Some(info) = scope.bindings.get_mut(name) {
                info.effectful_callable = true;
            }
        }
    }

    /// Does binding this expression to a name produce something that performs a
    /// host effect when called?
    ///
    /// Deliberately narrow: a name that resolves to a function declared `◆!`, a
    /// name already carrying the mark, or a parenthesised/marked form of either.
    /// Anything arriving through a record field, a list element, a parameter or
    /// the result of a call answers `false` — see `effects.md` for what that
    /// leaves uncovered.
    fn names_effectful_callable(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(i) => {
                if let Some(b) = self.lookup(&i.name) {
                    return b.effectful_callable;
                }
                self.functions
                    .get(&i.name)
                    .map(|m| m.declares_effect)
                    .unwrap_or(false)
            }
            Expr::Group(g) => self.names_effectful_callable(&g.expr),
            Expr::Unary(u) if u.op == UnaryOp::Effect => self.names_effectful_callable(&u.expr),
            _ => false,
        }
    }

    /// True when this name refers to an imported module rather than a value.
    ///
    /// The import binds its qualifier in the root scope, so a binding of the same
    /// name in any inner scope shadows it — `◆ f(math) ⟦ ^ math.x ⟧` reads a field
    /// of the parameter, whatever `use math` did at the top of the file.
    fn is_module_qualifier(&self, name: &str) -> bool {
        if !self.qualifier_in_scope(name) {
            return false;
        }
        !self
            .scopes
            .iter()
            .skip(1)
            .any(|scope| scope.bindings.contains_key(name))
    }

    /// Entry qualifiers hold everywhere; a merged module's qualifiers hold only
    /// inside the function copies that were merged in with them.
    fn qualifier_in_scope(&self, name: &str) -> bool {
        self.import_qualifiers.contains(name)
            || (self.in_injected_fn && self.merged_qualifiers.contains(name))
    }

    /// True when `@path[0].path[1]` is module access through the sigil.
    ///
    /// Unlike bare `cool.square`, which an inner binding named `cool` shadows
    /// (see [`Self::is_module_qualifier`]), `@cool` always means the module —
    /// the sigil exists to be unambiguous. Import qualifiers cannot collide
    /// with capability namespaces (checked at the import), so there is no
    /// precedence question here.
    fn at_module_qualifier(&self, path: &[String]) -> bool {
        path.len() == 2 && self.qualifier_in_scope(path[0].as_str())
    }

    /// `true` when module `qualifier` exports `field`; otherwise pushes the
    /// E020 "module has no public …" diagnostic naming the real exports.
    /// E017 when `?` is applied to a host call the table says never answers a
    /// result. `@fs.exists(p)?` and `@clock.sleep(ms)?` both read as right —
    /// the neighbouring calls all need `?` — and both used to pass `rite
    /// check` and die on the line's first execution.
    ///
    /// Only capability calls are checked: the builtin surface is where a
    /// caller cannot know the shape without the documentation. A path whose
    /// head is a module qualifier is skipped (module functions may answer
    /// anything), and an unknown path is left for the E042 check.
    fn check_try_target(&mut self, expr: &Expr, file: rite_core::FileId) {
        // Strip the transparent wrappers `?` sees through: `! call?`,
        // `(call)?`.
        let mut inner = expr;
        loop {
            match inner {
                Expr::Group(g) => inner = &g.expr,
                Expr::Unary(u) if u.op == UnaryOp::Effect => inner = &u.expr,
                _ => break,
            }
        }
        let cap = match inner {
            Expr::Call(c) => match c.callee.as_ref() {
                Expr::Capability(cap) => cap,
                _ => return,
            },
            // A bare capability reference is invoked (see `Expr::Capability`
            // below), so `@clock.now?` is the same mistake as `@clock.now()?`.
            Expr::Capability(cap) => cap,
            _ => return,
        };
        if self.at_module_qualifier(&cap.path) {
            return;
        }
        let path = cap.path.join(".");
        if host_returns_result(&path) == Some(false) {
            self.diagnostics.push(
                simple_error(
                    rite_core::E017_TRY_ON_NON_RESULT,
                    format!("`?` on `@{}`, which never answers a result", path),
                    file,
                    cap.span,
                    "this call answers a plain value",
                )
                .with_help(format!(
                    "drop the `?` — the return shape is in the capability \
                     reference (`rite describe capability {}`)",
                    cap.path.first().map(String::as_str).unwrap_or("")
                )),
            );
        }
    }

    fn check_module_export(
        &mut self,
        qualifier: &str,
        field: &str,
        span: Span,
        file: rite_core::FileId,
    ) -> bool {
        let mangled = format!("{}__{}", qualifier, field);
        // A private function's copy is injected under the mangled name so its
        // public siblings can call it; that must not make it reachable as
        // `helper.secret` from the entry.
        if self.functions.contains_key(&mangled) && !self.private_injected.contains(&mangled) {
            return true;
        }
        let mut exports: Vec<&str> = self
            .functions
            .keys()
            .filter(|k| !self.private_injected.contains(*k))
            .filter_map(|k| k.strip_prefix(&format!("{}__", qualifier)))
            .collect();
        exports.sort_unstable();
        let help = if exports.is_empty() {
            format!("`{}` exports nothing public", qualifier)
        } else {
            format!("`{}` exports: {}", qualifier, exports.join(", "))
        };
        self.diagnostics.push(
            rite_core::Diagnostic::error(
                E020_UNDEFINED_NAME,
                format!("module `{}` has no public `{}`", qualifier, field),
            )
            .with_primary(
                rite_core::SourceSpan::new(file, span),
                "not exported by that module",
            )
            .with_help(help),
        );
        false
    }

    /// E042 when `@path` starts with neither a capability namespace nor an
    /// imported module. Returns `true` when the diagnostic was pushed.
    ///
    /// This used to be a runtime-only failure: the registry's fallthrough
    /// raised "unknown capability" with no code and no span, so a typo'd
    /// namespace passed `rite check` clean.
    fn unknown_capability(&mut self, path: &[String], span: Span, file: rite_core::FileId) -> bool {
        let Some(ns) = path.first() else {
            return false;
        };
        if capability_namespaces().contains(ns.as_str()) {
            return false;
        }
        let help = if self.qualifier_in_scope(ns.as_str()) {
            format!(
                "`{}` is an imported module; access an export: @{}.<name>",
                ns, ns
            )
        } else {
            format!("if `{}` is a module, import it first: use {}", ns, ns)
        };
        self.diagnostics.push(
            simple_error(
                E042_UNKNOWN_CAPABILITY,
                format!("unknown capability `@{}`", ns),
                file,
                span,
                "not a capability namespace or an imported module",
            )
            .with_help(help),
        );
        true
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
/// that are not in the table answer "not effectful"; a path whose *namespace*
/// is unknown is rejected with `E042` in `resolve_expr` before it gets here.
pub fn is_effectful(path: &str) -> bool {
    HOST_EFFECTS
        .iter()
        .find(|(name, _, _)| *name == path)
        .map(|(_, effectful, _)| *effectful)
        .unwrap_or(false)
}

/// Whether `@path` answers a result value, so postfix `?` applies to it.
/// `None` for a path the table does not know (a module access, a typo the
/// E042 check reports separately).
pub fn host_returns_result(path: &str) -> Option<bool> {
    HOST_EFFECTS
        .iter()
        .find(|(name, _, _)| *name == path)
        .map(|(_, _, returns_result)| *returns_result)
}

/// Every capability namespace (`fs`, `http`, …), derived from [`HOST_EFFECTS`].
///
/// Used for two checks: an import may not bind one of these as its qualifier,
/// and an `@name` whose first segment is neither a namespace here nor an
/// imported qualifier is `E042` at check time rather than a runtime failure.
pub fn capability_namespaces() -> &'static HashSet<&'static str> {
    static SET: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();
    SET.get_or_init(|| {
        HOST_EFFECTS
            .iter()
            .filter_map(|(path, _, _)| path.split('.').next())
            .collect()
    })
}

/// Can this arm set match every possible scrutinee?
///
/// Rite is dynamically typed, so exhaustiveness is only provable in two ways:
/// an arm that matches unconditionally, or an arm set that saturates one of
/// the finite value domains. Anything else can fall through to
/// `match failure: no arm matched` at runtime, which is what E029 warns about.
fn covers_all_inputs(arms: &[rite_syntax::MatchArm]) -> bool {
    // A guarded arm covers nothing: the guard can refuse any value.
    if arms
        .iter()
        .any(|a| a.guard.is_none() && is_irrefutable(&a.pattern))
    {
        return true;
    }
    // Domains a finite arm set can saturate: booleans, `ok`/`err`,
    // `some`/`none`. An or-pattern's alternatives count as if they were
    // separate arms, so `true | false → …` saturates the boolean domain.
    #[derive(Default)]
    struct Saw {
        t: bool,
        f: bool,
        ok: bool,
        err: bool,
        some: bool,
        none: bool,
        lit_none: bool,
    }
    fn note(pat: &Pattern, s: &mut Saw) {
        match pat {
            Pattern::Literal(l) => match l.kind {
                LitKind::Bool(true) => s.t = true,
                LitKind::Bool(false) => s.f = true,
                LitKind::None => s.lit_none = true,
                _ => {}
            },
            Pattern::Result(r) => match r.kind {
                ResultPatKind::Ok => s.ok = true,
                ResultPatKind::Err => s.err = true,
                ResultPatKind::Some => s.some = true,
                ResultPatKind::None => s.none = true,
            },
            Pattern::Or(o) => o.alternatives.iter().for_each(|p| note(p, s)),
            _ => {}
        }
    }
    let mut s = Saw::default();
    for arm in arms.iter().filter(|a| a.guard.is_none()) {
        note(&arm.pattern, &mut s);
    }
    (s.t && s.f) || (s.ok && s.err) || (s.some && (s.none || s.lit_none))
}

/// A pattern that matches any value, binding nothing (`_`) or everything (`x`).
///
/// Every other pattern is refutable: literals and atoms compare, lists check
/// length, records check fields, `ok`/`err` check the result tag, and a typed
/// pattern checks the type. An or-pattern is irrefutable when any alternative
/// is.
fn is_irrefutable(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard(_) | Pattern::Ident(_) => true,
        Pattern::Or(o) => o.alternatives.iter().any(is_irrefutable),
        _ => false,
    }
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
        for (path, _, _) in HOST_EFFECTS {
            assert!(seen.insert(*path), "duplicate HOST_EFFECTS entry {}", path);
        }
    }

    #[test]
    fn effect_table_paths_are_cap_dot_function() {
        for (path, _, _) in HOST_EFFECTS {
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
