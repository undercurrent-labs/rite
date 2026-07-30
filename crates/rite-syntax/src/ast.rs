use rite_core::{FileId, Span};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub file: FileId,
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(FunctionDecl),
    Data(DataDecl),
    Event(EventDecl),
    Import(ImportDecl),
    Test(TestDecl),
    Statement(Stmt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDecl {
    pub is_pub: bool,
    /// Declared with `◆!` / `def!`: this function performs host effects, so
    /// calling it takes a marker. Checked against what the body actually does.
    #[serde(default)]
    pub is_effectful: bool,
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDecl {
    pub name: Ident,
    pub fields: Vec<RecordEntry>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDecl {
    pub kind: EventKind,
    pub atom: AtomLit,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Item,
    Room,
    World,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDecl {
    pub path: ModulePath,
    pub alias: Option<Ident>,
    /// `pub use` re-exports the module's public names from this module.
    #[serde(default)]
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulePath {
    pub segments: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub params: Vec<Param>,
    pub body: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    Binding(Binding),
    Assign(Assign),
    Expr(Expr),
    Return(ReturnStmt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub pattern: Pattern,
    pub mutable: bool,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assign {
    pub name: Ident,
    /// None = `:=`; Some(op) = op-assign (`+=`, …) desugared as `name := name op value`.
    pub op: Option<BinOp>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    /// True when the value came from juxtaposition — `^ 200 ⟨…⟩` — rather than an
    /// explicit list. Both produce a `List`, so without this the formatter cannot tell
    /// them apart and reprints the HTTP handler idiom as `^ [200, ⟨…⟩]`.
    #[serde(default)]
    pub juxtaposed: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Literal(Literal),
    Ident(Ident),
    Atom(AtomLit),
    List(ListExpr),
    Record(RecordExpr),
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Call(CallExpr),
    Member(MemberExpr),
    Index(IndexExpr),
    Pipeline(PipelineExpr),
    If(IfExpr),
    Match(MatchExpr),
    Block(Block),
    Capability(CapabilityRef),
    Placeholder(Placeholder),
    Try(TryExpr),
    HttpListen(HttpListenExpr),
    Route(RouteExpr),
    Group(GroupExpr),
    Coalesce(CoalesceExpr),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(l) => l.span,
            Expr::Ident(i) => i.span,
            Expr::Atom(a) => a.span,
            Expr::List(l) => l.span,
            Expr::Record(r) => r.span,
            Expr::Binary(b) => b.span,
            Expr::Unary(u) => u.span,
            Expr::Call(c) => c.span,
            Expr::Member(m) => m.span,
            Expr::Index(i) => i.span,
            Expr::Pipeline(p) => p.span,
            Expr::If(i) => i.span,
            Expr::Match(m) => m.span,
            Expr::Block(b) => b.span,
            Expr::Capability(c) => c.span,
            Expr::Placeholder(p) => p.span,
            Expr::Try(t) => t.span,
            Expr::HttpListen(h) => h.span,
            Expr::Route(r) => r.span,
            Expr::Group(g) => g.span,
            Expr::Coalesce(c) => c.span,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Literal {
    pub kind: LitKind,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LitKind {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomLit {
    pub parts: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListExpr {
    pub elements: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordExpr {
    pub entries: Vec<RecordEntry>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordEntry {
    pub key: RecordKey,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordKey {
    Ident(Ident),
    String(String),
    Atom(AtomLit),
    /// `..rec` spread — value is the record expression to merge.
    Spread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryExpr {
    pub op: BinOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Xor,
    In,
    NotIn,
    Power,
    Idiv,
    Range,     // a..b exclusive
    RangeIncl, // a..=b or a‥b inclusive
    Compose,   // f ∘ g
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub expr: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
    Effect,
    Spread, // ..expr inside list/record
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberExpr {
    pub object: Box<Expr>,
    pub field: Ident,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexExpr {
    pub object: Box<Expr>,
    pub index: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineExpr {
    pub input: Box<Expr>,
    pub stages: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_branch: Block,
    pub else_branch: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRef {
    pub path: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placeholder {
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TryExpr {
    pub expr: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpListenExpr {
    pub addr: Box<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteExpr {
    pub method: HttpMethod,
    pub path: String,
    pub params: Vec<Param>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupExpr {
    pub expr: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoalesceExpr {
    pub left: Box<Expr>,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    Ident(Ident),
    Atom(AtomLit),
    Literal(Literal),
    Wildcard(Span),
    List(ListPattern),
    Record(RecordPattern),
    Result(ResultPattern),
    Typed(TypedPattern),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPattern {
    pub elements: Vec<Pattern>,
    pub rest: Option<Box<Pattern>>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordPattern {
    pub fields: Vec<FieldPattern>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldPattern {
    pub name: Ident,
    pub pattern: Option<Pattern>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultPattern {
    pub kind: ResultPatKind,
    pub binding: Option<Box<Pattern>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultPatKind {
    Ok,
    Err,
    Some,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedPattern {
    pub pattern: Box<Pattern>,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeExpr {
    Named(Ident),
    List(Box<TypeExpr>),
    Result(Box<TypeExpr>),
    Record(Vec<(Ident, TypeExpr)>),
    Any(Span),
}
