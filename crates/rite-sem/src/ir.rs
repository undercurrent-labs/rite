//! Shared semantic intermediate representation for interpreter and compiler.

use rite_core::Span;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeFunctionId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramIr {
    pub modules: Vec<ModuleIr>,
    pub entry: EntryPoint,
    pub functions: Vec<FunctionIr>,
    pub native_names: HashMap<String, NativeFunctionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleIr {
    pub name: String,
    pub statements: Vec<ExprIr>,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntryPoint {
    Script,
    Main { func: FuncId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionIr {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<LocalId>,
    pub param_names: Vec<String>,
    pub body: BlockIr,
    pub is_pub: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockIr {
    pub params: Vec<LocalId>,
    pub body: Vec<ExprIr>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprIr {
    Constant(ValueLiteral),
    Local(LocalId),
    Global(String),
    Bind {
        local: LocalId,
        name: String,
        mutable: bool,
        value: Box<ExprIr>,
        span: Span,
    },
    Assign {
        local: LocalId,
        value: Box<ExprIr>,
        span: Span,
    },
    Call {
        callee: Box<ExprIr>,
        args: Vec<ExprIr>,
        span: Span,
    },
    NativeCall {
        name: String,
        args: Vec<ExprIr>,
        effect: EffectKind,
        span: Span,
    },
    CapabilityCall {
        path: Vec<String>,
        args: Vec<ExprIr>,
        effect: EffectKind,
        span: Span,
    },
    Closure(ClosureIr),
    Pipeline {
        input: Box<ExprIr>,
        stages: Vec<PipelineStageIr>,
        span: Span,
    },
    If {
        condition: Box<ExprIr>,
        then_branch: BlockIr,
        else_branch: Option<BlockIr>,
        span: Span,
    },
    Match {
        value: Box<ExprIr>,
        arms: Vec<MatchArmIr>,
        span: Span,
    },
    Return(Option<Box<ExprIr>>, Span),
    BuildList(Vec<ExprIr>, Span),
    BuildRecord(Vec<(KeyIr, ExprIr)>, Span),
    Member {
        object: Box<ExprIr>,
        field: String,
        span: Span,
    },
    Index {
        object: Box<ExprIr>,
        index: Box<ExprIr>,
        span: Span,
    },
    Unary {
        op: UnaryOpIr,
        expr: Box<ExprIr>,
        span: Span,
    },
    Binary {
        op: BinaryOpIr,
        left: Box<ExprIr>,
        right: Box<ExprIr>,
        span: Span,
    },
    Try {
        expr: Box<ExprIr>,
        span: Span,
    },
    Coalesce {
        left: Box<ExprIr>,
        right: Box<ExprIr>,
        span: Span,
    },
    Block(BlockIr),
    Atom(String, Span),
    Placeholder(Span),
    HttpListen {
        addr: Box<ExprIr>,
        routes: Vec<RouteIr>,
        middleware: Vec<ExprIr>,
        span: Span,
    },
    /// Sequence of expressions; value is last.
    Seq(Vec<ExprIr>, Span),
}

impl ExprIr {
    pub fn span(&self) -> Span {
        match self {
            ExprIr::Constant(v) => v.span(),
            ExprIr::Local(_) | ExprIr::Global(_) => Span::DUMMY,
            ExprIr::Bind { span, .. }
            | ExprIr::Assign { span, .. }
            | ExprIr::Call { span, .. }
            | ExprIr::NativeCall { span, .. }
            | ExprIr::CapabilityCall { span, .. }
            | ExprIr::Pipeline { span, .. }
            | ExprIr::If { span, .. }
            | ExprIr::Match { span, .. }
            | ExprIr::Return(_, span)
            | ExprIr::BuildList(_, span)
            | ExprIr::BuildRecord(_, span)
            | ExprIr::Member { span, .. }
            | ExprIr::Index { span, .. }
            | ExprIr::Unary { span, .. }
            | ExprIr::Binary { span, .. }
            | ExprIr::Try { span, .. }
            | ExprIr::Coalesce { span, .. }
            | ExprIr::Atom(_, span)
            | ExprIr::Placeholder(span)
            | ExprIr::HttpListen { span, .. }
            | ExprIr::Seq(_, span) => *span,
            ExprIr::Closure(c) => c.span,
            ExprIr::Block(b) => b.span,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureIr {
    pub params: Vec<LocalId>,
    pub param_names: Vec<String>,
    pub body: BlockIr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStageIr {
    pub kind: StageKind,
    pub expr: ExprIr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageKind {
    Call,
    MemberProjection(String),
    Block,
    PlaceholderCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArmIr {
    pub pattern: PatternIr,
    pub body: ExprIr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternIr {
    Ident(LocalId, String),
    Atom(String),
    Literal(ValueLiteral),
    Wildcard,
    List {
        elements: Vec<PatternIr>,
        rest: Option<Box<PatternIr>>,
    },
    Record {
        fields: Vec<(String, Option<PatternIr>)>,
    },
    Result {
        kind: ResultPatKindIr,
        binding: Option<Box<PatternIr>>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResultPatKindIr {
    Ok,
    Err,
    Some,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteIr {
    pub method: String,
    pub path: String,
    pub param: Option<LocalId>,
    pub body: BlockIr,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyIr {
    Ident(String),
    String(String),
    Atom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectKind {
    Pure,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOpIr {
    Neg,
    Not,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOpIr {
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
    In,
    NotIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueLiteral {
    None(Span),
    Bool(bool, Span),
    Int(i64, Span),
    Float(f64, Span),
    String(String, Span),
}

impl ValueLiteral {
    pub fn span(&self) -> Span {
        match self {
            ValueLiteral::None(s)
            | ValueLiteral::Bool(_, s)
            | ValueLiteral::Int(_, s)
            | ValueLiteral::Float(_, s)
            | ValueLiteral::String(_, s) => *s,
        }
    }
}

pub fn ir_to_json(program: &ProgramIr) -> serde_json::Value {
    serde_json::to_value(program).unwrap_or(serde_json::Value::Null)
}
