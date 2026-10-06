//! Name resolution, checking and compilation of a parsed program.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::ProgramError;
use super::ast::{BinaryOp, Expr, ModuleCall, ProgramAst, Rule, RuleKind, UnaryOp};
use super::expr::{Code, EnvField, Func, NO_ENV, Op, Query, Scope, Var, eval};
use super::lexer::Span;
use super::parser::parse;

/// Largest program text the compiler accepts.
pub const MAX_PROGRAM_BYTES: usize = 256 * 1024;

/// Most organ types one program may declare. Each organ type gets one
/// template in a package's organ atlas, and cards name it in one byte.
pub const MAX_ORGAN_TYPES: usize = 64;

/// Most body types one program may declare. A segment names its body in
/// one byte of a graph (see [`crate::graph::GraphSegment::body`]), and each
/// body's spines add templates to the organ atlas after the organs'.
pub const MAX_BODY_TYPES: usize = 8;

/// Turtle commands and other built-in symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turtle {
    /// `[`: start a branch.
    Push,
    /// `]`: end a branch.
    Pop,
    /// `%`: cut the rest of the branch during derivation.
    Cut,
    /// `F(l)`: draw a segment of length `l`.
    Forward,
    /// `f(l)`: move without drawing.
    Move,
    /// `+(a)`: turn left by `a` degrees.
    Left,
    /// `-(a)`: turn right.
    Right,
    /// `&(a)`: pitch down.
    Down,
    /// `^(a)`: pitch up.
    Up,
    /// `\(a)`: roll left.
    RollLeft,
    /// `/(a)`: roll right.
    RollRight,
    /// `|`: turn around.
    Around,
    /// `$`: roll until the left vector is horizontal.
    Level,
    /// `!(r)`: set the radius of the following segments.
    Width,
    /// `T(x, y, z, e)`: bend every following segment toward `(x, y, z)` with
    /// elasticity `e`.
    Tropism,
    /// `steer(x, y, z, w)`: turn the heading a fraction `w` of the way toward
    /// the plant-frame direction `(x, y, z)`.
    Steer,
}

impl Turtle {
    /// Every built-in symbol in symbol-id order, with its text and argument
    /// count after defaults are filled in.
    pub const ALL: [(Self, &'static str, u8); 16] = [
        (Self::Push, "[", 0),
        (Self::Pop, "]", 0),
        (Self::Cut, "%", 0),
        (Self::Forward, "F", 1),
        (Self::Move, "f", 1),
        (Self::Left, "+", 1),
        (Self::Right, "-", 1),
        (Self::Down, "&", 1),
        (Self::Up, "^", 1),
        (Self::RollLeft, "\\", 1),
        (Self::RollRight, "/", 1),
        (Self::Around, "|", 0),
        (Self::Level, "$", 0),
        (Self::Width, "!", 1),
        (Self::Tropism, "T", 4),
        (Self::Steer, "steer", 4),
    ];

    fn is_turn(self) -> bool {
        matches!(
            self,
            Self::Left | Self::Right | Self::Down | Self::Up | Self::RollLeft | Self::RollRight
        )
    }
}

pub const PUSH: u16 = 0;
pub const POP: u16 = 1;
pub const CUT: u16 = 2;
pub const FORWARD: u16 = 3;
pub const MOVE: u16 = 4;
pub const FIRST_USER_SYMBOL: u16 = 16;

/// What an organ is, which decides how it is drawn and shaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganKind {
    /// A cluster of needles or small leaves drawn as one card.
    Foliage,
    Leaf,
    Flower,
    Fruit,
    Cone,
}

impl OrganKind {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "foliage" => Self::Foliage,
            "leaf" => Self::Leaf,
            "flower" => Self::Flower,
            "fruit" => Self::Fruit,
            "cone" => Self::Cone,
            _ => return None,
        })
    }

    /// Shading area of an organ 1 m long when the program does not give
    /// one, m²: about what the kind's default look draws.
    pub(crate) fn default_area(self) -> f64 {
        match self {
            Self::Foliage => 0.42,
            Self::Leaf => 0.23,
            Self::Flower => 0.64,
            Self::Fruit => 0.89,
            Self::Cone => 0.65,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Turtle(Turtle),
    /// A module declared with `module`; the bits are its [`Query`] set.
    Module {
        queries: u8,
    },
    /// An organ declared with `organ`, with its shading area per unit size².
    Organ {
        kind: OrganKind,
        area: Code,
    },
    /// A body declared with `body`: drawn like `F`, its segments meshed as
    /// a fleshy body. `index` is its position among the program's bodies.
    Body {
        index: u8,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// Number of parameters every instance of the symbol carries.
    pub arity: u8,
}

/// A module to create: an axiom entry or one module of a successor.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub symbol: u16,
    pub args: Box<[Code]>,
    /// Ordinal used to derive the new module's lineage. Modules that carry
    /// identity (`F`, `f`, declared modules and organs) count separately from
    /// turtle commands, so adding a turn to a rule does not change any
    /// branch's identity or random draws.
    pub ordinal: u32,
}

/// Turtle-command ordinals live in their own half of the ordinal space.
pub const TURTLE_ORDINAL: u32 = 1 << 31;

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub condition: Option<Code>,
    pub weight: Option<Code>,
    pub successor: Box<[Item]>,
    /// Index of the successor module that continues the predecessor: the
    /// first module outside any branch with the predecessor's symbol. It keeps
    /// the predecessor's lineage and birth time.
    pub continuation: Option<usize>,
    pub span: Span,
}

/// Environment tools a program can configure, by registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Light = 0,
    Space = 1,
    Vigour = 2,
    Pipe = 3,
}

pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: &'static str,
    pub version: u32,
    pub keys: &'static [(&'static str, f64)],
}

/// The tool registry. A new capability is a new entry or a new version of an
/// entry; a version, once released, never changes behaviour.
pub const TOOLS: [ToolSpec; 4] = [
    ToolSpec {
        kind: ToolKind::Light,
        name: "light",
        version: 1,
        keys: &[("cell", 0.5), ("extinction", 0.5), ("bud", 0.01)],
    },
    ToolSpec {
        kind: ToolKind::Space,
        name: "space",
        version: 1,
        keys: &[
            ("shape", 1.0),
            ("base", 0.0),
            ("height", 4.0),
            ("radius", 2.0),
            ("density", 20.0),
            ("influence", 1.5),
            ("kill", 0.4),
            ("angle", 90.0),
        ],
    },
    ToolSpec {
        kind: ToolKind::Vigour,
        name: "vigour",
        version: 1,
        keys: &[("lambda", 0.5), ("alpha", 2.0), ("max", 1.0e9)],
    },
    ToolSpec {
        kind: ToolKind::Pipe,
        name: "pipe",
        version: 1,
        keys: &[
            ("exponent", 2.5),
            ("tip", 0.003),
            ("organ", 0.002),
            ("rings", 0.0),
        ],
    },
];

/// A configured tool: one expression per registry key, re-evaluated each step.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolConfig {
    pub version: u32,
    pub settings: Box<[Code]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub default: Code,
}

/// A checked, compiled plant program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub name: String,
    pub revision: u32,
    /// SHA-256 of the program text, part of every package key.
    pub source_sha256: String,
    pub params: Vec<ParamInfo>,
    pub symbols: Vec<Symbol>,
    pub axiom: Box<[Item]>,
    pub productions: Vec<Vec<CompiledRule>>,
    pub decompositions: Vec<Vec<CompiledRule>>,
    pub interpretations: Vec<Vec<CompiledRule>>,
    pub tools: [Option<ToolConfig>; 4],
}

impl Program {
    /// Parse, check and compile program text.
    ///
    /// # Errors
    ///
    /// Returns the first error with its line and column.
    pub fn compile(source: &str) -> Result<Self, ProgramError> {
        if source.len() > MAX_PROGRAM_BYTES {
            return Err(ProgramError {
                span: Span::default(),
                message: format!(
                    "the program is {} bytes; the limit is {MAX_PROGRAM_BYTES}",
                    source.len()
                ),
            });
        }
        let ast = parse(source)?;
        let mut program = Compiler::default().compile(&ast)?;
        program.source_sha256 = hex(&Sha256::digest(source.as_bytes()));
        Ok(program)
    }

    /// The organ types the program declares, in declaration order: their
    /// names and kinds. The position of an organ type here is its index in
    /// plant graphs and in a package's organ atlas.
    pub fn organs(&self) -> impl Iterator<Item = (&str, OrganKind)> {
        self.symbols.iter().filter_map(|symbol| match symbol.kind {
            SymbolKind::Organ { kind, .. } => Some((symbol.name.as_str(), kind)),
            _ => None,
        })
    }

    /// The body types the program declares, in declaration order. The
    /// position of a body here, plus one, is what a graph segment drawn
    /// with it records (0 is wood).
    pub fn bodies(&self) -> impl Iterator<Item = &str> {
        self.symbols.iter().filter_map(|symbol| match symbol.kind {
            SymbolKind::Body { .. } => Some(symbol.name.as_str()),
            _ => None,
        })
    }

    #[must_use]
    pub fn symbol_id(&self, name: &str) -> Option<u16> {
        self.symbols
            .iter()
            .position(|symbol| symbol.name == name)
            .and_then(|index| u16::try_from(index).ok())
    }

    #[must_use]
    pub fn tool(&self, kind: ToolKind) -> Option<&ToolConfig> {
        self.tools[kind as usize].as_ref()
    }

    /// Evaluate parameter defaults in order, replacing any named in
    /// `overrides`. Later defaults see earlier overridden values.
    ///
    /// # Errors
    ///
    /// Fails for an override that names no parameter or a non-finite value.
    pub fn resolve_params(
        &self,
        overrides: &BTreeMap<String, f64>,
    ) -> Result<Vec<f64>, ProgramError> {
        for name in overrides.keys() {
            if !self.params.iter().any(|param| &param.name == name) {
                return Err(ProgramError {
                    span: Span::default(),
                    message: format!(
                        "the species sets `{name}`, but program `{}` has no such parameter",
                        self.name
                    ),
                });
            }
        }
        let mut values = Vec::with_capacity(self.params.len());
        let mut stack = Vec::new();
        for param in &self.params {
            let value = if let Some(value) = overrides.get(&param.name) {
                *value
            } else {
                let scope = Scope {
                    globals: &values,
                    locals: &[],
                    env: &NO_ENV,
                    t: 0.0,
                    dt: 0.0,
                    age: 0.0,
                    step: 0.0,
                    key: 0,
                };
                eval(&param.default, &scope, &mut stack)
            };
            if !value.is_finite() {
                return Err(ProgramError {
                    span: Span::default(),
                    message: format!("parameter `{}` is {value}; it must be finite", param.name),
                });
            }
            values.push(value);
        }
        Ok(values)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

const KEYWORDS: [&str; 13] = [
    "lsystem",
    "param",
    "module",
    "organ",
    "body",
    "tool",
    "axiom",
    "rule",
    "decompose",
    "interpret",
    "queries",
    "area",
    "weight",
];

const BUILTIN_NAMES: [&str; 13] = [
    "t", "dt", "age", "step", "pi", "true", "false", "rand", "gauss", "uniform", "F", "f", "T",
];

fn reserved(name: &str) -> Option<&'static str> {
    if KEYWORDS.contains(&name) {
        Some("a keyword")
    } else if BUILTIN_NAMES.contains(&name) || name == "steer" {
        Some("a built-in name")
    } else if EnvField::from_name(name).is_some() {
        Some("an environment field")
    } else if Func::from_name(name).is_some() {
        Some("a function")
    } else {
        None
    }
}

/// Where an expression appears, which decides the names it may use.
struct Context<'a> {
    what: &'static str,
    globals: usize,
    locals: &'a [(String, Span)],
    /// `Some((module name, queries))` in production rules.
    env: Option<(&'a str, u8)>,
    time: bool,
    age: bool,
    random: bool,
}

#[derive(Default)]
struct Compiler {
    param_index: BTreeMap<String, u16>,
    params: Vec<ParamInfo>,
    symbol_index: BTreeMap<String, u16>,
    symbols: Vec<Symbol>,
    random_sites: u32,
    angle: Option<u16>,
}

/// Every module that queries light, vigour or space needs that tool.
fn check_queries(ast: &ProgramAst, tools: &[Option<ToolConfig>; 4]) -> Result<(), ProgramError> {
    for decl in &ast.modules {
        for (name, span) in &decl.queries {
            let tool = match Query::from_name(name) {
                Some(Query::Light) => ToolKind::Light,
                Some(Query::Vigour) => ToolKind::Vigour,
                Some(Query::Space) => ToolKind::Space,
                _ => continue,
            };
            if tools[tool as usize].is_none() {
                return err(
                    *span,
                    format!(
                        "module `{}` queries {name}, but the program configures no `tool {}@1`",
                        decl.name, TOOLS[tool as usize].name
                    ),
                );
            }
        }
    }
    Ok(())
}

fn err<T>(span: Span, message: impl Into<String>) -> Result<T, ProgramError> {
    Err(ProgramError {
        span,
        message: message.into(),
    })
}

/// Expressions that may read every parameter and nothing else.
const PARAMS_ONLY: Context<'static> = Context {
    what: "a setting",
    globals: 0,
    locals: &[],
    env: None,
    time: false,
    age: false,
    random: false,
};

impl Compiler {
    fn compile(mut self, ast: &ProgramAst) -> Result<Program, ProgramError> {
        for (index, (turtle, text, arity)) in Turtle::ALL.iter().enumerate() {
            let id = u16::try_from(index).unwrap_or(u16::MAX);
            self.symbol_index.insert((*text).to_string(), id);
            self.symbols.push(Symbol {
                name: (*text).to_string(),
                kind: SymbolKind::Turtle(*turtle),
                arity: *arity,
            });
        }
        debug_assert_eq!(self.symbols.len(), usize::from(FIRST_USER_SYMBOL));

        self.declare_params(ast)?;
        self.declare_modules(ast)?;
        self.declare_organs(ast)?;
        self.declare_bodies(ast)?;
        let tools = self.configure_tools(ast)?;
        check_queries(ast, &tools)?;

        let axiom_context = Context {
            what: "the axiom",
            globals: self.params.len(),
            time: true,
            random: true,
            ..PARAMS_ONLY
        };
        let (axiom, _) = self.successor(&ast.axiom, &axiom_context, ast.axiom_span, None)?;

        let symbol_count = self.symbols.len();
        let mut productions = vec![Vec::new(); symbol_count];
        let mut decompositions = vec![Vec::new(); symbol_count];
        let mut interpretations = vec![Vec::new(); symbol_count];
        for rule in &ast.rules {
            let (symbol, compiled) = self.rule(rule)?;
            let list = match rule.kind {
                RuleKind::Production => &mut productions,
                RuleKind::Decomposition => &mut decompositions,
                RuleKind::Interpretation => &mut interpretations,
            };
            list[usize::from(symbol)].push(compiled);
        }

        Ok(Program {
            name: ast.name.clone(),
            revision: ast.revision,
            source_sha256: String::new(),
            params: self.params,
            symbols: self.symbols,
            axiom,
            productions,
            decompositions,
            interpretations,
            tools,
        })
    }

    /// Parameters, in order: each default may use the ones before it.
    fn declare_params(&mut self, ast: &ProgramAst) -> Result<(), ProgramError> {
        for decl in &ast.params {
            self.check_new_name(&decl.name, decl.span)?;
            let context = Context {
                what: "a parameter value",
                globals: self.params.len(),
                ..PARAMS_ONLY
            };
            let default = self.expr(&decl.value, &context)?;
            let index = u16::try_from(self.params.len()).map_err(|_| ProgramError {
                span: decl.span,
                message: "too many parameters".into(),
            })?;
            self.param_index.insert(decl.name.clone(), index);
            self.params.push(ParamInfo {
                name: decl.name.clone(),
                default,
            });
        }
        self.angle = self.param_index.get("angle").copied();
        Ok(())
    }

    fn declare_modules(&mut self, ast: &ProgramAst) -> Result<(), ProgramError> {
        for decl in &ast.modules {
            self.check_new_name(&decl.name, decl.span)?;
            let mut seen = Vec::new();
            for (name, span) in &decl.params {
                if seen.contains(name) {
                    return err(*span, format!("parameter `{name}` is listed twice"));
                }
                seen.push(name.clone());
            }
            let mut queries = 0;
            for (name, span) in &decl.queries {
                let Some(query) = Query::from_name(name) else {
                    return err(
                        *span,
                        format!(
                            "unknown query `{name}`; expected light, vigour, space or position"
                        ),
                    );
                };
                queries |= query.bit();
            }
            let arity = u8::try_from(decl.params.len()).map_err(|_| ProgramError {
                span: decl.span,
                message: "too many module parameters".into(),
            })?;
            self.add_symbol(&decl.name, SymbolKind::Module { queries }, arity, decl.span)?;
        }
        Ok(())
    }

    fn declare_organs(&mut self, ast: &ProgramAst) -> Result<(), ProgramError> {
        let context = Context {
            what: "an organ's area",
            globals: self.params.len(),
            ..PARAMS_ONLY
        };
        if let Some(decl) = ast.organs.get(MAX_ORGAN_TYPES) {
            return err(
                decl.span,
                format!("a program may declare at most {MAX_ORGAN_TYPES} organ types"),
            );
        }
        for decl in &ast.organs {
            self.check_new_name(&decl.name, decl.span)?;
            let Some(kind) = OrganKind::from_name(&decl.kind.0) else {
                return err(
                    decl.kind.1,
                    format!(
                        "unknown organ kind `{}`; expected foliage, leaf, flower, fruit or cone",
                        decl.kind.0
                    ),
                );
            };
            let area = match &decl.area {
                Some(expr) => self.expr(expr, &context)?,
                None => Code::constant(kind.default_area()),
            };
            self.add_symbol(&decl.name, SymbolKind::Organ { kind, area }, 1, decl.span)?;
        }
        Ok(())
    }

    /// Bodies after the organs, so a program without bodies keeps its
    /// symbol numbers.
    fn declare_bodies(&mut self, ast: &ProgramAst) -> Result<(), ProgramError> {
        if let Some(decl) = ast.bodies.get(MAX_BODY_TYPES) {
            return err(
                decl.span,
                format!("a program may declare at most {MAX_BODY_TYPES} body types"),
            );
        }
        for (index, decl) in ast.bodies.iter().enumerate() {
            self.check_new_name(&decl.name, decl.span)?;
            let index = u8::try_from(index).unwrap_or(u8::MAX);
            self.add_symbol(&decl.name, SymbolKind::Body { index }, 1, decl.span)?;
        }
        Ok(())
    }

    /// Tool settings, defaults first; a setting may read parameters and
    /// the time.
    fn configure_tools(
        &mut self,
        ast: &ProgramAst,
    ) -> Result<[Option<ToolConfig>; 4], ProgramError> {
        let mut tools: [Option<ToolConfig>; 4] = [None, None, None, None];
        let context = Context {
            what: "a tool setting",
            globals: self.params.len(),
            time: true,
            ..PARAMS_ONLY
        };
        for decl in &ast.tools {
            let Some(spec) = TOOLS.iter().find(|spec| spec.name == decl.name) else {
                let known: Vec<&str> = TOOLS.iter().map(|spec| spec.name).collect();
                return err(
                    decl.span,
                    format!(
                        "unknown tool `{}`; known tools: {}",
                        decl.name,
                        known.join(", ")
                    ),
                );
            };
            if decl.version != spec.version {
                return err(
                    decl.span,
                    format!(
                        "`{}@{}` does not exist; this build has `{}@{}`",
                        decl.name, decl.version, spec.name, spec.version
                    ),
                );
            }
            if tools[spec.kind as usize].is_some() {
                return err(
                    decl.span,
                    format!("tool `{}` is configured twice", decl.name),
                );
            }
            let mut settings: Vec<Code> = spec
                .keys
                .iter()
                .map(|(_, default)| Code::constant(*default))
                .collect();
            let mut seen = Vec::new();
            for (key, value, span) in &decl.settings {
                let Some(index) = spec.keys.iter().position(|(name, _)| name == key) else {
                    let known: Vec<&str> = spec.keys.iter().map(|(name, _)| *name).collect();
                    return err(
                        *span,
                        format!(
                            "`{}` has no setting `{key}`; its settings are {}",
                            decl.name,
                            known.join(", ")
                        ),
                    );
                };
                if seen.contains(&index) {
                    return err(*span, format!("setting `{key}` is given twice"));
                }
                seen.push(index);
                settings[index] = self.expr(value, &context)?;
            }
            tools[spec.kind as usize] = Some(ToolConfig {
                version: spec.version,
                settings: settings.into_boxed_slice(),
            });
        }
        Ok(tools)
    }

    fn check_new_name(&self, name: &str, span: Span) -> Result<(), ProgramError> {
        if let Some(what) = reserved(name) {
            return err(span, format!("`{name}` is {what} and cannot be redefined"));
        }
        if self.param_index.contains_key(name) || self.symbol_index.contains_key(name) {
            return err(span, format!("`{name}` is already defined"));
        }
        Ok(())
    }

    fn add_symbol(
        &mut self,
        name: &str,
        kind: SymbolKind,
        arity: u8,
        span: Span,
    ) -> Result<(), ProgramError> {
        let id = u16::try_from(self.symbols.len()).map_err(|_| ProgramError {
            span,
            message: "too many symbols".into(),
        })?;
        self.symbol_index.insert(name.to_string(), id);
        self.symbols.push(Symbol {
            name: name.to_string(),
            kind,
            arity,
        });
        Ok(())
    }

    fn rule(&mut self, rule: &Rule) -> Result<(u16, CompiledRule), ProgramError> {
        let Some(&symbol) = self.symbol_index.get(&rule.symbol) else {
            return err(
                rule.span,
                format!(
                    "`{}` is not declared; declare it with `module {}` or `organ {} <kind>`",
                    rule.symbol, rule.symbol, rule.symbol
                ),
            );
        };
        let info = self.symbols[usize::from(symbol)].clone();
        if let SymbolKind::Turtle(turtle) = info.kind
            && matches!(
                turtle,
                Turtle::Push | Turtle::Pop | Turtle::Cut | Turtle::Around | Turtle::Level
            )
        {
            return err(rule.span, format!("`{}` cannot be rewritten", info.name));
        }
        if rule.params.len() != usize::from(info.arity) {
            return err(
                rule.span,
                format!(
                    "`{}` has {} parameter(s), but the rule names {}",
                    info.name,
                    info.arity,
                    rule.params.len()
                ),
            );
        }
        let mut seen: Vec<&str> = Vec::new();
        for (name, span) in &rule.params {
            if let Some(what) = reserved(name) {
                return err(
                    *span,
                    format!("`{name}` is {what} and cannot name a rule parameter"),
                );
            }
            if self.param_index.contains_key(name) {
                return err(
                    *span,
                    format!("rule parameter `{name}` would hide the program parameter `{name}`"),
                );
            }
            if seen.contains(&name.as_str()) {
                return err(*span, format!("parameter `{name}` is listed twice"));
            }
            seen.push(name);
        }
        let env = match (rule.kind, &info.kind) {
            (RuleKind::Production, SymbolKind::Module { queries }) => {
                Some((info.name.as_str(), *queries))
            }
            (RuleKind::Production, _) => Some((info.name.as_str(), 0)),
            _ => None,
        };
        let context = Context {
            what: match rule.kind {
                RuleKind::Production => "a rule",
                RuleKind::Decomposition => "a decomposition rule",
                RuleKind::Interpretation => "an interpretation rule",
            },
            globals: self.params.len(),
            locals: &rule.params,
            env,
            time: true,
            age: true,
            random: true,
        };
        let condition = rule
            .condition
            .as_ref()
            .map(|expr| self.expr(expr, &context))
            .transpose()?;
        let weight = rule
            .weight
            .as_ref()
            .map(|expr| self.expr(expr, &context))
            .transpose()?;
        let predecessor = (rule.kind == RuleKind::Production).then_some(symbol);
        let (successor, continuation) =
            self.successor(&rule.successor, &context, rule.span, predecessor)?;
        Ok((
            symbol,
            CompiledRule {
                condition,
                weight,
                successor,
                continuation,
                span: rule.span,
            },
        ))
    }

    fn successor(
        &mut self,
        calls: &[ModuleCall],
        context: &Context<'_>,
        span: Span,
        predecessor: Option<u16>,
    ) -> Result<(Box<[Item]>, Option<usize>), ProgramError> {
        let mut items = Vec::with_capacity(calls.len());
        let mut depth = 0_i32;
        let mut continuation = None;
        let (mut identity, mut turtle) = (0_u32, 0_u32);
        for call in calls {
            let Some(&symbol) = self.symbol_index.get(&call.symbol) else {
                return err(
                    call.span,
                    format!(
                        "`{}` is not declared; declare it with `module`, `organ` or `body`",
                        call.symbol
                    ),
                );
            };
            let info = self.symbols[usize::from(symbol)].clone();
            let mut args = Vec::with_capacity(usize::from(info.arity));
            for arg in &call.args {
                args.push(self.expr(arg, context)?);
            }
            if args.len() != usize::from(info.arity) {
                let default = match info.kind {
                    SymbolKind::Turtle(Turtle::Forward | Turtle::Move)
                    | SymbolKind::Organ { .. }
                    | SymbolKind::Body { .. } => Some(Code::constant(1.0)),
                    SymbolKind::Turtle(kind) if kind.is_turn() => match self.angle {
                        Some(index) => Some(Code(Box::new([Op::Global(index)]))),
                        None => {
                            return err(
                                call.span,
                                format!(
                                    "`{}` without an angle needs `param angle = <degrees>;`",
                                    info.name
                                ),
                            );
                        }
                    },
                    _ => None,
                };
                match default {
                    Some(code) if args.is_empty() => args.push(code),
                    _ => {
                        return err(
                            call.span,
                            format!(
                                "`{}` takes {} argument(s), found {}",
                                info.name,
                                info.arity,
                                call.args.len()
                            ),
                        );
                    }
                }
            }
            match info.kind {
                SymbolKind::Turtle(Turtle::Push) => depth += 1,
                SymbolKind::Turtle(Turtle::Pop) => {
                    depth -= 1;
                    if depth < 0 {
                        return err(call.span, "`]` without a matching `[`");
                    }
                }
                _ => {}
            }
            let carries_identity = matches!(
                info.kind,
                SymbolKind::Turtle(Turtle::Forward | Turtle::Move)
                    | SymbolKind::Module { .. }
                    | SymbolKind::Organ { .. }
                    | SymbolKind::Body { .. }
            );
            let ordinal = if carries_identity {
                identity += 1;
                identity - 1
            } else {
                turtle += 1;
                TURTLE_ORDINAL | (turtle - 1)
            };
            if continuation.is_none() && depth == 0 && Some(symbol) == predecessor {
                continuation = Some(items.len());
            }
            items.push(Item {
                symbol,
                args: args.into_boxed_slice(),
                ordinal,
            });
        }
        if depth != 0 {
            return err(span, "a `[` is never closed");
        }
        Ok((items.into_boxed_slice(), continuation))
    }

    fn expr(&mut self, expr: &Expr, context: &Context<'_>) -> Result<Code, ProgramError> {
        let mut ops = Vec::new();
        self.emit(expr, context, &mut ops)?;
        Ok(Code(ops.into_boxed_slice()))
    }

    fn emit(
        &mut self,
        expr: &Expr,
        context: &Context<'_>,
        ops: &mut Vec<Op>,
    ) -> Result<(), ProgramError> {
        match expr {
            Expr::Number(value) => ops.push(Op::Const(*value)),
            Expr::Name(name, span) => ops.push(self.name(name, *span, context)?),
            Expr::Unary(op, inner) => {
                self.emit(inner, context, ops)?;
                ops.push(match op {
                    UnaryOp::Neg => Op::Neg,
                    UnaryOp::Not => Op::Not,
                });
            }
            Expr::Binary(op, left, right) => {
                self.emit(left, context, ops)?;
                self.emit(right, context, ops)?;
                ops.push(match op {
                    BinaryOp::Add => Op::Add,
                    BinaryOp::Sub => Op::Sub,
                    BinaryOp::Mul => Op::Mul,
                    BinaryOp::Div => Op::Div,
                    BinaryOp::Pow => Op::Pow,
                    BinaryOp::Less => Op::Less,
                    BinaryOp::LessEq => Op::LessEq,
                    BinaryOp::Greater => Op::Greater,
                    BinaryOp::GreaterEq => Op::GreaterEq,
                    BinaryOp::Equal => Op::Equal,
                    BinaryOp::NotEqual => Op::NotEqual,
                    BinaryOp::And => Op::And,
                    BinaryOp::Or => Op::Or,
                });
            }
            Expr::Select(condition, then, otherwise) => {
                self.emit(condition, context, ops)?;
                self.emit(then, context, ops)?;
                self.emit(otherwise, context, ops)?;
                ops.push(Op::Select);
            }
            Expr::Call(name, args, span) => self.call(name, args, *span, context, ops)?,
        }
        Ok(())
    }

    fn name(&self, name: &str, span: Span, context: &Context<'_>) -> Result<Op, ProgramError> {
        if let Some(index) = context.locals.iter().position(|(local, _)| local == name) {
            return Ok(Op::Local(u8::try_from(index).unwrap_or(u8::MAX)));
        }
        match name {
            "pi" => return Ok(Op::Const(std::f64::consts::PI)),
            "true" => return Ok(Op::Const(1.0)),
            "false" => return Ok(Op::Const(0.0)),
            "t" | "dt" | "step" => {
                if !context.time {
                    return err(span, format!("`{name}` cannot be used in {}", context.what));
                }
                return Ok(Op::Var(match name {
                    "t" => Var::T,
                    "dt" => Var::Dt,
                    _ => Var::Step,
                }));
            }
            "age" => {
                if !context.age {
                    return err(span, format!("`age` cannot be used in {}", context.what));
                }
                return Ok(Op::Var(Var::Age));
            }
            _ => {}
        }
        if let Some(field) = EnvField::from_name(name) {
            return match context.env {
                Some((module, queries)) => {
                    if queries & field.query().bit() == 0 {
                        let query = match field.query() {
                            Query::Light => "light",
                            Query::Vigour => "vigour",
                            Query::Space => "space",
                            Query::Position => "position",
                        };
                        err(
                            span,
                            format!(
                                "`{name}` needs `{module}` to be declared with `queries {query}`"
                            ),
                        )
                    } else {
                        Ok(Op::Env(field))
                    }
                }
                None => err(
                    span,
                    format!(
                        "`{name}` is an environment value, readable only in production rules, \
                         not in {}",
                        context.what
                    ),
                ),
            };
        }
        if let Some(&index) = self.param_index.get(name)
            && usize::from(index) < context.globals
        {
            return Ok(Op::Global(index));
        }
        err(span, format!("unknown name `{name}`"))
    }

    fn call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        context: &Context<'_>,
        ops: &mut Vec<Op>,
    ) -> Result<(), ProgramError> {
        if matches!(name, "rand" | "gauss" | "uniform") {
            if !context.random {
                return err(span, format!("`{name}` cannot be used in {}", context.what));
            }
            let site = self.random_sites;
            self.random_sites += 1;
            match (name, args.len()) {
                ("rand", 0) => ops.push(Op::Rand(site)),
                ("gauss", 0) => ops.push(Op::Gauss(site)),
                ("rand" | "gauss", 1) => {
                    self.emit(&args[0], context, ops)?;
                    ops.push(if name == "rand" {
                        Op::RandKey
                    } else {
                        Op::GaussKey
                    });
                }
                ("uniform", 2) => {
                    self.emit(&args[0], context, ops)?;
                    self.emit(&args[1], context, ops)?;
                    ops.push(Op::Uniform(site));
                }
                _ => {
                    let expected = if name == "uniform" { "2" } else { "0 or 1" };
                    return err(span, format!("`{name}` takes {expected} argument(s)"));
                }
            }
            return Ok(());
        }
        let Some((func, arity)) = Func::from_name(name) else {
            return err(span, format!("unknown function `{name}`"));
        };
        if args.len() != arity {
            return err(
                span,
                format!("`{name}` takes {arity} argument(s), found {}", args.len()),
            );
        }
        for arg in args {
            self.emit(arg, context, ops)?;
        }
        ops.push(Op::Call(func, u8::try_from(arity).unwrap_or(u8::MAX)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(source: &str) -> Result<Program, ProgramError> {
        Program::compile(source)
    }

    #[test]
    fn compiles_rules_and_fills_defaults() {
        let program = compile(
            "lsystem p 1;
             param angle = 30;
             module A(n);
             organ leaf leaf;
             axiom A(0);
             rule A(n) : n < 3 -> F [ + A(n + 1) ] leaf A(n + 1);",
        )
        .unwrap();
        let a = program.symbol_id("A").unwrap();
        let rule = &program.productions[usize::from(a)][0];
        // F gets length 1, + gets the angle parameter, leaf gets size 1.
        assert_eq!(rule.successor[0].args[0].as_constant(), Some(1.0));
        assert_eq!(&*rule.successor[2].args[0].0, &[Op::Global(0)]);
        assert_eq!(rule.successor[5].args[0].as_constant(), Some(1.0));
        // The continuation is the A outside the branch.
        assert_eq!(rule.continuation, Some(6));
        // Identity ordinals skip turtle commands.
        let ordinals: Vec<u32> = rule.successor.iter().map(|item| item.ordinal).collect();
        assert_eq!(
            ordinals,
            [
                0,
                TURTLE_ORDINAL,
                TURTLE_ORDINAL | 1,
                1,
                TURTLE_ORDINAL | 2,
                2,
                3
            ]
        );
    }

    #[test]
    fn resolves_parameters_with_overrides() {
        let program = compile("lsystem p 1; param a = 2; param b = a * 3; axiom F(b);").unwrap();
        assert_eq!(
            program.resolve_params(&BTreeMap::new()).unwrap(),
            [2.0, 6.0]
        );
        let overrides = BTreeMap::from([("a".to_string(), 5.0)]);
        assert_eq!(program.resolve_params(&overrides).unwrap(), [5.0, 15.0]);
        let unknown = BTreeMap::from([("c".to_string(), 1.0)]);
        assert!(program.resolve_params(&unknown).is_err());
    }

    #[test]
    fn rejects_programs_that_break_the_rules() {
        let cases = [
            (
                "lsystem p 1; module A; axiom A; rule A : light > 0 -> A;",
                "queries light",
            ),
            (
                "lsystem p 1; module A queries light; axiom A;",
                "no `tool light@1`",
            ),
            ("lsystem p 1; axiom + F;", "param angle"),
            ("lsystem p 1; axiom [ F;", "never closed"),
            ("lsystem p 1; axiom F ];", "without a matching"),
            ("lsystem p 1; param t = 1; axiom F;", "built-in"),
            ("lsystem p 1; param a = rand(); axiom F;", "cannot be used"),
            (
                "lsystem p 1; module A(x); axiom A(1); rule A -> A;",
                "1 parameter",
            ),
            ("lsystem p 1; axiom F; rule [ -> F;", "cannot be rewritten"),
            ("lsystem p 1; tool light@2 { }; axiom F;", "does not exist"),
            (
                "lsystem p 1; tool light@1 { colour = 1 }; axiom F;",
                "no setting",
            ),
            (
                "lsystem p 1; module A; axiom A; decompose A : t > 1 && age > 1 -> B;",
                "not declared",
            ),
            (
                "lsystem p 1; param x = 1; module A(x); axiom A(1); rule A(x) -> A(x);",
                "hide",
            ),
            (
                "lsystem p 1; module A queries position; axiom A; interpret A : py > 1 -> F;",
                "only in production",
            ),
        ];
        for (source, expected) in cases {
            let error = compile(source).expect_err(source);
            assert!(
                error.message.contains(expected),
                "{source}: expected `{expected}` in `{}`",
                error.message
            );
        }
    }
}
