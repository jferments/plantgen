//! Syntax tree of a plant program, as written. [`super::program`] resolves
//! names and turns it into bytecode.

use super::lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Name(String, Span),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `condition ? then : otherwise`
    Select(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>, Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Equal,
    NotEqual,
    And,
    Or,
}

/// A module written in an axiom or a successor: a symbol and its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleCall {
    pub symbol: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Rewrites a module once per derivation step.
    Production,
    /// Expands a module into a structure in the same step it appears.
    Decomposition,
    /// Expands a module into turtle commands while drawing; never stored.
    Interpretation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub kind: RuleKind,
    pub symbol: String,
    pub params: Vec<(String, Span)>,
    pub condition: Option<Expr>,
    pub weight: Option<Expr>,
    pub successor: Vec<ModuleCall>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub name: String,
    pub params: Vec<(String, Span)>,
    pub queries: Vec<(String, Span)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrganDecl {
    pub name: String,
    pub kind: (String, Span),
    pub area: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDecl {
    pub name: String,
    pub version: u32,
    pub settings: Vec<(String, Expr, Span)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramAst {
    pub name: String,
    pub revision: u32,
    pub params: Vec<ParamDecl>,
    pub modules: Vec<ModuleDecl>,
    pub organs: Vec<OrganDecl>,
    pub tools: Vec<ToolDecl>,
    pub axiom: Vec<ModuleCall>,
    pub axiom_span: Span,
    pub rules: Vec<Rule>,
}
