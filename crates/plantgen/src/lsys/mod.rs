//! The plant L-system language and its growth engine.
//!
//! A plant program is a parametric, stochastic, open L-system in the sense of
//! *The Algorithmic Beauty of Plants* (Prusinkiewicz and Lindenmayer 1990)
//! and Měch and Prusinkiewicz (1996): modules with parameters, production
//! rules with conditions and weighted random choice, decomposition and
//! interpretation rules, and environment queries answered by registered
//! tools. The language is documented for authors in
//! `docs/developer/PLANTS.md`.
//!
//! The engine is sandboxed. Programs cannot read files or loop: expressions
//! have no jumps, rules are applied once per step, and [`Limits`] bound the
//! string, the geometry, the tools' grids and every recursion.

pub mod ast;
pub mod derive;
pub mod expr;
pub mod lexer;
pub mod parser;
pub mod program;
pub mod tools;
pub mod turtle;

use std::fmt;

pub use lexer::Span;
pub use program::{OrganKind, Program};
pub use tools::Neighbourhood;

/// A problem in program text, with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramError {
    pub span: Span,
    pub message: String,
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.span.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.span, self.message)
        }
    }
}

impl std::error::Error for ProgramError {}

/// A problem while growing a plant.
#[derive(Debug, Clone, PartialEq)]
pub enum GrowthError {
    /// A sandbox limit was reached.
    Limit { what: &'static str, limit: u64 },
    /// A rule produced an invalid value; the span is the rule's statement.
    Rule { span: Span, message: String },
    /// A tool was configured with invalid settings.
    Tool { tool: &'static str, message: String },
    /// The growth settings themselves are invalid.
    Settings(String),
}

impl fmt::Display for GrowthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { what, limit } => write!(f, "the plant exceeded {limit} {what}"),
            Self::Rule { span, message } if span.line == 0 => f.write_str(message),
            Self::Rule { span, message } => write!(f, "rule at {span}: {message}"),
            Self::Tool { tool, message } => write!(f, "tool `{tool}`: {message}"),
            Self::Settings(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GrowthError {}

/// Sandbox limits. The defaults allow a large old tree and stop a runaway
/// program within seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_modules: usize,
    pub max_steps: u32,
    pub max_decomposition_depth: usize,
    pub max_interpretation_depth: usize,
    /// Most segments, and separately most organs, in one drawn step.
    pub max_segments: usize,
    pub max_voxels: usize,
    pub max_points: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_modules: 1_000_000,
            max_steps: 10_000,
            max_decomposition_depth: 32,
            max_interpretation_depth: 8,
            max_segments: 2_000_000,
            max_voxels: 16_000_000,
            max_points: 2_000_000,
        }
    }
}
