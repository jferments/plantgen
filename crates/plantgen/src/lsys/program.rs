//! Name resolution, checking and compilation of a parsed program.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::ProgramError;
use super::ast::{BinaryOp, Expr, ModuleCall, ProgramAst, Rule, RuleKind, UnaryOp};
use super::expr::{Code, EnvField, Func, NO_ENV, Op, Query, Scope, Var, eval};
use super::lexer::Span;
use super::parser::parse_chain;

/// Largest program text the compiler accepts.
pub const MAX_PROGRAM_BYTES: usize = 256 * 1024;

/// Most organ types one program may declare. Each organ type gets one
/// template in a package's organ atlas, and cards name it in one byte.
pub const MAX_ORGAN_TYPES: usize = 64;

/// Most structure types one program may declare (spec rework M7). A
/// graph names a structure's type in two bytes.
pub const MAX_STRUCTURE_TYPES: usize = 64;

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
/// `/`: a ring turns each copy with it.
pub const ROLL: u16 = 10;
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
    /// An organ declared with `organ`, with its shading area per unit size²
    /// and what it is when its declaration says (`as term`, spec rework M7).
    Organ {
        kind: OrganKind,
        area: Code,
        term: Option<&'static str>,
    },
    /// A body declared with `body`: drawn like `F`, its segments meshed as
    /// a fleshy body. `index` is its position among the program's bodies.
    Body {
        index: u8,
    },
    /// A structure declared with `structure` (spec rework M7): drawing it
    /// opens the structure, which holds what is drawn after it to the end
    /// of the branch. `index` is its position among the program's
    /// structures, `term` what it is.
    Structure {
        index: u8,
        term: &'static str,
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

/// Structures count in a block of the turtle commands' half of their own
/// (spec rework M7), so opening a structure in a successor moves no other
/// module's lineage or random draws.
pub const STRUCTURE_ORDINAL: u32 = TURTLE_ORDINAL | (1 << 23);

/// The most copies a ring writes; past it growth stops with a limit error.
pub const MAX_RING: u32 = 64;

/// A ring's copy k takes its items' ordinals with k << `RING_SHIFT` added,
/// below the bit that marks decomposed modules, so no two copies share a
/// lineage and copy 0 keeps the items' own.
pub const RING_SHIFT: u32 = 24;

/// `ring(n) [ items ]`, or `ring(n) organ(size, roll, pitch[, level])`
/// with an organ that carries its own turn, in a successor or the axiom
/// (phyllotaxis P1): the items, as they would compile without the ring,
/// written n times round the turtle's heading. Copy k of a bracket is
/// `[ /(k * 360 / n) items ]`, and copy k of an organ has k * 360 / n added
/// to its roll; copy 0 is the items themselves, with their draws and
/// identity, so a ring of one is what it holds. Copy k draws with the
/// scope's key mixed with k and adds k << [`RING_SHIFT`] to its items'
/// ordinals. A ring holds no value of its own: its items say how each
/// copy leans and varies.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    /// How many copies: rounded half away from zero where the successor is
    /// written; below 1 the ring writes nothing.
    pub count: Code,
    /// Its first item in the successor.
    pub at: usize,
    /// How many items it holds: its bracket's, `[` to `]`, or one organ.
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRule {
    pub condition: Option<Code>,
    pub weight: Option<Code>,
    pub successor: Box<[Item]>,
    /// Index of the successor module that continues the predecessor: the
    /// first module outside any branch with the predecessor's symbol, or,
    /// for an organ without one, the first organ outside any branch (a
    /// flower that becomes a fruit; organ phases, spec rework M7). It keeps
    /// the predecessor's lineage and birth time.
    pub continuation: Option<usize>,
    /// The successor's rings, in order. Interpretation rules have none.
    pub rings: Box<[Ring]>,
    pub span: Span,
}

/// Environment tools a program can configure, by registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Light = 0,
    Space = 1,
    Vigour = 2,
    Pipe = 3,
    Host = 4,
    Substrate = 5,
}

pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: &'static str,
    pub version: u32,
    pub keys: &'static [(&'static str, f64)],
}

/// The tool registry. A new capability is a new entry or a new version of an
/// entry; a version, once released, never changes behaviour.
/// The first random call site of tool settings, far above any rule's.
pub const TOOL_SITES: u32 = 1 << 24;

pub const TOOLS: [ToolSpec; 9] = [
    ToolSpec {
        kind: ToolKind::Light,
        name: "light",
        version: 1,
        keys: &[("cell", 0.5), ("extinction", 0.5), ("bud", 0.01)],
    },
    // G3: the substrate shades the plant and reflects light up into it
    // (each material's albedo); `reach` is how far past the plant the
    // substrate is looked at (`super::tools`).
    ToolSpec {
        kind: ToolKind::Light,
        name: "light",
        version: 2,
        keys: &[
            ("cell", 0.5),
            ("extinction", 0.5),
            ("bud", 0.01),
            ("reach", 0.5),
        ],
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
    // Growth plan G1: space that returns where the plant is shed, an
    // outline widest anywhere up the crown and as full as a box or as
    // pointed as a cone, lobes round the edge, and billows: the edge moved
    // in and out by smooth noise `bump_size` metres across
    // (`super::tools`).
    ToolSpec {
        kind: ToolKind::Space,
        name: "space",
        version: 2,
        keys: &[
            ("shape", 1.0),
            ("base", 0.0),
            ("height", 4.0),
            ("radius", 2.0),
            ("density", 20.0),
            ("influence", 1.5),
            ("kill", 0.4),
            ("angle", 90.0),
            ("renew", 1.0),
            ("widest", 0.5),
            ("fullness", 2.0),
            ("lobes", 0.0),
            ("lobe_depth", 0.0),
            ("bumps", 0.0),
            ("bump_size", 3.0),
        ],
    },
    // G3: contact. Living tips are solid, `radius` metres round: each
    // reads how many others touch it and the way out from among them.
    ToolSpec {
        kind: ToolKind::Space,
        name: "space",
        version: 3,
        keys: &[("radius", 0.01)],
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
    ToolSpec {
        kind: ToolKind::Host,
        name: "host",
        version: 1,
        keys: &[("reach", 2.0)],
    },
    // G3: what the plant grows on, from the conditions' substrate (level
    // soil without one): distance, normal and material at each module.
    ToolSpec {
        kind: ToolKind::Substrate,
        name: "substrate",
        version: 1,
        keys: &[],
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
    /// The axiom's rings, in order.
    pub axiom_rings: Box<[Ring]>,
    pub productions: Vec<Vec<CompiledRule>>,
    pub decompositions: Vec<Vec<CompiledRule>>,
    pub interpretations: Vec<Vec<CompiledRule>>,
    pub tools: [Option<ToolConfig>; 6],
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
        let ast = super::chain::resolve(parse_chain(source)?)?;
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

    /// What each organ type is where its declaration says (`as term`),
    /// in declaration order; `None` where it does not (see
    /// [`crate::organs`]).
    pub fn organ_terms(&self) -> impl Iterator<Item = Option<&'static str>> + '_ {
        self.symbols.iter().filter_map(|symbol| match symbol.kind {
            SymbolKind::Organ { term, .. } => Some(term),
            _ => None,
        })
    }

    /// The structure types the program declares, in declaration order:
    /// their names and terms. The position of a structure type here is
    /// its index in plant graphs.
    pub fn structures(&self) -> impl Iterator<Item = (&str, &'static str)> {
        self.symbols.iter().filter_map(|symbol| match symbol.kind {
            SymbolKind::Structure { term, .. } => Some((symbol.name.as_str(), term)),
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

const KEYWORDS: [&str; 16] = [
    "lsystem",
    "param",
    "module",
    "organ",
    "body",
    "structure",
    "as",
    "tool",
    "axiom",
    "rule",
    "decompose",
    "interpret",
    "queries",
    "area",
    "weight",
    "ring",
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

/// A compiled successor or axiom.
struct Successor {
    items: Box<[Item]>,
    rings: Box<[Ring]>,
    continuation: Option<usize>,
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
fn check_queries(ast: &ProgramAst, tools: &[Option<ToolConfig>; 6]) -> Result<(), ProgramError> {
    for decl in &ast.modules {
        for (name, span) in &decl.queries {
            let tool = match Query::from_name(name) {
                Some(Query::Light) => ToolKind::Light,
                Some(Query::Vigour) => ToolKind::Vigour,
                Some(Query::Space) => ToolKind::Space,
                Some(Query::Host) => ToolKind::Host,
                Some(Query::Substrate) => ToolKind::Substrate,
                _ => continue,
            };
            if tools[tool as usize].is_none() {
                return err(
                    *span,
                    format!(
                        "module `{}` queries {name}, but the program configures no `tool {}`",
                        decl.name,
                        TOOLS.iter().find(|spec| spec.kind == tool).map_or_else(
                            String::new,
                            |spec| format!("{}@{}", spec.name, spec.version)
                        )
                    ),
                );
            }
        }
    }
    Ok(())
}

/// Whether `rule` may rewrite `symbol`: not a bracket, a cut, `|` or `$`,
/// a structure by production only, and with the symbol's parameters.
fn rewritable(symbol: &Symbol, rule: &Rule) -> Result<(), ProgramError> {
    if let SymbolKind::Turtle(turtle) = symbol.kind
        && matches!(
            turtle,
            Turtle::Push | Turtle::Pop | Turtle::Cut | Turtle::Around | Turtle::Level
        )
    {
        return err(rule.span, format!("`{}` cannot be rewritten", symbol.name));
    }
    if matches!(symbol.kind, SymbolKind::Structure { .. }) && rule.kind != RuleKind::Production {
        return err(
            rule.span,
            format!(
                "structure `{}` cannot be decomposed or interpreted; a rule may only \
                 rewrite it, to end it or keep it",
                symbol.name
            ),
        );
    }
    if rule.params.len() != usize::from(symbol.arity) {
        return err(
            rule.span,
            format!(
                "`{}` has {} parameter(s), but the rule names {}",
                symbol.name,
                symbol.arity,
                rule.params.len()
            ),
        );
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
        debug_assert_eq!(
            self.symbols[usize::from(ROLL)].kind,
            SymbolKind::Turtle(Turtle::RollRight)
        );

        self.declare_params(ast)?;
        self.declare_modules(ast)?;
        self.declare_organs(ast)?;
        self.declare_bodies(ast)?;
        self.declare_structures(ast)?;
        // Draws in tool settings number their sites in a block of their
        // own, so a program that adds one never moves the sites of the
        // axiom or of the rules it inherits.
        let sites = std::mem::replace(&mut self.random_sites, TOOL_SITES);
        let tools = self.configure_tools(ast)?;
        self.random_sites = sites;
        check_queries(ast, &tools)?;

        let axiom_context = Context {
            what: "the axiom",
            globals: self.params.len(),
            time: true,
            random: true,
            ..PARAMS_ONLY
        };
        let axiom = self.successor(&ast.axiom, &axiom_context, ast.axiom_span, None, true)?;

        let symbol_count = self.symbols.len();
        let mut productions = vec![Vec::new(); symbol_count];
        let mut decompositions = vec![Vec::new(); symbol_count];
        let mut interpretations = vec![Vec::new(); symbol_count];
        // Rules compile in the chain's order, the furthest ancestor's first
        // (so their random call sites keep their numbers); a program's own
        // rules for a module are then tried before those it inherits.
        let mut depths = vec![vec![Vec::new(); symbol_count]; 3];
        for rule in &ast.rules {
            let (symbol, compiled) = self.rule(rule)?;
            let kind = match rule.kind {
                RuleKind::Production => 0,
                RuleKind::Decomposition => 1,
                RuleKind::Interpretation => 2,
            };
            let list = match kind {
                0 => &mut productions,
                1 => &mut decompositions,
                _ => &mut interpretations,
            };
            list[usize::from(symbol)].push(compiled);
            depths[kind][usize::from(symbol)].push(rule.depth);
        }
        for (kind, list) in [&mut productions, &mut decompositions, &mut interpretations]
            .into_iter()
            .enumerate()
        {
            for (symbol, rules) in list.iter_mut().enumerate() {
                let order = &depths[kind][symbol];
                if order.windows(2).all(|pair| pair[0] <= pair[1]) {
                    continue;
                }
                let mut keyed: Vec<_> = order.iter().copied().zip(rules.drain(..)).collect();
                keyed.sort_by_key(|(depth, _)| *depth);
                rules.extend(keyed.into_iter().map(|(_, rule)| rule));
            }
        }

        Ok(Program {
            name: ast.name.clone(),
            revision: ast.revision,
            source_sha256: String::new(),
            params: self.params,
            symbols: self.symbols,
            axiom: axiom.items,
            axiom_rings: axiom.rings,
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
                            "unknown query `{name}`; expected light, vigour, space, position or host"
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
            let term = match &decl.term {
                Some((name, span)) => {
                    Some(crate::organs::organ_term(name).ok_or_else(|| ProgramError {
                        span: *span,
                        message: format!(
                            "unknown term `{name}`; an organ is piece or one of the \
                             vocabulary's organs: {}",
                            crate::organs::known_terms().join(", ")
                        ),
                    })?)
                }
                None => None,
            };
            self.add_symbol(
                &decl.name,
                SymbolKind::Organ { kind, area, term },
                1,
                decl.span,
            )?;
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

    /// Structures after the bodies, so a program without structures keeps
    /// its symbol numbers.
    fn declare_structures(&mut self, ast: &ProgramAst) -> Result<(), ProgramError> {
        if let Some(decl) = ast.structures.get(MAX_STRUCTURE_TYPES) {
            return err(
                decl.span,
                format!("a program may declare at most {MAX_STRUCTURE_TYPES} structure types"),
            );
        }
        for (index, decl) in ast.structures.iter().enumerate() {
            self.check_new_name(&decl.name, decl.span)?;
            let (name, span) = &decl.term;
            let Some(term) = crate::organs::structure_term(name) else {
                return err(
                    *span,
                    format!(
                        "unknown term `{name}`; a structure is one of the vocabulary's organs: {}",
                        crate::organs::known_terms().join(", ")
                    ),
                );
            };
            let index = u8::try_from(index).unwrap_or(u8::MAX);
            self.add_symbol(
                &decl.name,
                SymbolKind::Structure { index, term },
                0,
                decl.span,
            )?;
        }
        Ok(())
    }

    /// Tool settings, defaults first; a setting may read parameters, the
    /// time and per-plant draws (`rand`, `gauss`, `uniform`: keyed on the
    /// plant's seed alone, so the same every step; growth plan G1).
    fn configure_tools(
        &mut self,
        ast: &ProgramAst,
    ) -> Result<[Option<ToolConfig>; 6], ProgramError> {
        let mut tools: [Option<ToolConfig>; 6] = [None, None, None, None, None, None];
        let context = Context {
            what: "a tool setting",
            globals: self.params.len(),
            time: true,
            random: true,
            ..PARAMS_ONLY
        };
        for decl in &ast.tools {
            if !TOOLS.iter().any(|spec| spec.name == decl.name) {
                let mut known: Vec<&str> = TOOLS.iter().map(|spec| spec.name).collect();
                known.dedup();
                return err(
                    decl.span,
                    format!(
                        "unknown tool `{}`; known tools: {}",
                        decl.name,
                        known.join(", ")
                    ),
                );
            }
            let Some(spec) = TOOLS
                .iter()
                .find(|spec| spec.name == decl.name && spec.version == decl.version)
            else {
                let versions: Vec<String> = TOOLS
                    .iter()
                    .filter(|spec| spec.name == decl.name)
                    .map(|spec| format!("`{}@{}`", spec.name, spec.version))
                    .collect();
                return err(
                    decl.span,
                    format!(
                        "`{}@{}` does not exist; this build has {}",
                        decl.name,
                        decl.version,
                        versions.join(" and ")
                    ),
                );
            };
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
                    "`{}` is not declared; declare it with `module {}`, `organ {} <kind>`, \
                     `body {}` or `structure {} <term>`",
                    rule.symbol, rule.symbol, rule.symbol, rule.symbol, rule.symbol
                ),
            );
        };
        let info = self.symbols[usize::from(symbol)].clone();
        rewritable(&info, rule)?;
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
        let successor = self.successor(
            &rule.successor,
            &context,
            rule.span,
            predecessor,
            rule.kind != RuleKind::Interpretation,
        )?;
        Ok((
            symbol,
            CompiledRule {
                condition,
                weight,
                successor: successor.items,
                continuation: successor.continuation,
                rings: successor.rings,
                span: rule.span,
            },
        ))
    }

    /// A successor or the axiom: its items, its rings and, in a
    /// production, the item that continues the predecessor. Rings may
    /// stand in it unless it is an interpretation's.
    fn successor(
        &mut self,
        calls: &[ModuleCall],
        context: &Context<'_>,
        span: Span,
        predecessor: Option<u16>,
        rings_allowed: bool,
    ) -> Result<Successor, ProgramError> {
        let mut items = Vec::with_capacity(calls.len());
        let mut rings = Vec::new();
        // The ring being compiled: its count, its first item, and the depth
        // its bracket's `]` returns to (none for an organ, done at once).
        let mut open: Option<(Code, usize, Option<i32>)> = None;
        let mut depth = 0_i32;
        let (mut continuation, mut first_organ) = (None, None);
        let (mut identity, mut turtle, mut structures) = (0_u32, 0_u32, 0_u32);
        for (index, call) in calls.iter().enumerate() {
            if call.symbol == "ring" {
                if !rings_allowed {
                    return err(
                        call.span,
                        format!("a ring cannot stand in {}", context.what),
                    );
                }
                if open.is_some() {
                    return err(call.span, "a ring may not hold a ring");
                }
                let (count, closes) = self.ring(calls, index, context, depth)?;
                open = Some((count, items.len(), closes));
                continue;
            }
            let Some(&symbol) = self.symbol_index.get(&call.symbol) else {
                return err(
                    call.span,
                    format!(
                        "`{}` is not declared; declare it with `module`, `organ`, `body` or \
                         `structure`",
                        call.symbol
                    ),
                );
            };
            let info = self.symbols[usize::from(symbol)].clone();
            let args = self.arguments(call, &info, context)?;
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
            } else if matches!(info.kind, SymbolKind::Structure { .. }) {
                structures += 1;
                STRUCTURE_ORDINAL | (structures - 1)
            } else {
                turtle += 1;
                TURTLE_ORDINAL | (turtle - 1)
            };
            if depth == 0 {
                if continuation.is_none() && Some(symbol) == predecessor {
                    continuation = Some(items.len());
                }
                if first_organ.is_none() && matches!(info.kind, SymbolKind::Organ { .. }) {
                    first_organ = Some(items.len());
                }
            }
            items.push(Item {
                symbol,
                args: args.into_boxed_slice(),
                ordinal,
            });
            if let Some((_, _, closes)) = &open
                && closes.is_none_or(|level| symbol == POP && depth == level)
                && let Some((count, at, _)) = open.take()
            {
                rings.push(Ring {
                    count,
                    at,
                    len: items.len() - at,
                });
            }
        }
        if depth != 0 {
            return err(span, "a `[` is never closed");
        }
        debug_assert!(open.is_none(), "a ring's items always close it");
        // An organ rewritten into another lives on in it.
        let organ = predecessor.is_some_and(|symbol| {
            matches!(
                self.symbols[usize::from(symbol)].kind,
                SymbolKind::Organ { .. }
            )
        });
        let continuation = continuation.or(first_organ.filter(|_| organ));
        Ok(Successor {
            items: items.into_boxed_slice(),
            rings: rings.into_boxed_slice(),
            continuation,
        })
    }

    /// A call's arguments, with the one an omitted argument defaults to: a
    /// step or a size of 1, a turn of `angle`.
    fn arguments(
        &mut self,
        call: &ModuleCall,
        info: &Symbol,
        context: &Context<'_>,
    ) -> Result<Vec<Code>, ProgramError> {
        let mut args = Vec::with_capacity(usize::from(info.arity));
        for arg in &call.args {
            args.push(self.expr(arg, context)?);
        }
        // An organ may carry its own turn (growth plan G1): `leaf(s, roll,
        // pitch)` or `leaf(s, roll, pitch, level)` places it as
        // `[ /(roll) &(pitch) $ leaf(s) ]` would (`$` only with a level
        // above 0), in one module instead of five
        // (`super::turtle::organ_frame`).
        let turned_organ =
            matches!(info.kind, SymbolKind::Organ { .. }) && matches!(args.len(), 3 | 4);
        if args.len() != usize::from(info.arity) && !turned_organ {
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
        Ok(args)
    }

    /// `ring(n)` at `calls[index]`, `depth` branches deep: its count, and
    /// the depth the bracket after it closes at (none when an organ with
    /// its own turn follows it).
    fn ring(
        &mut self,
        calls: &[ModuleCall],
        index: usize,
        context: &Context<'_>,
        depth: i32,
    ) -> Result<(Code, Option<i32>), ProgramError> {
        let call = &calls[index];
        let [count] = call.args.as_slice() else {
            return err(call.span, "`ring` takes one argument: how many copies");
        };
        let turned_organ = |next: &ModuleCall| {
            matches!(next.args.len(), 3 | 4)
                && self.symbol_index.get(&next.symbol).is_some_and(|&symbol| {
                    matches!(
                        self.symbols[usize::from(symbol)].kind,
                        SymbolKind::Organ { .. }
                    )
                })
        };
        let closes = match calls.get(index + 1) {
            Some(next) if next.symbol == "[" => Some(depth),
            Some(next) if turned_organ(next) => None,
            _ => {
                return err(
                    call.span,
                    "`ring(n)` is followed by a bracket, `[ … ]`, or by an organ that \
                     carries its own turn, `leaf(size, roll, pitch)`",
                );
            }
        };
        Ok((self.expr(count, context)?, closes))
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
                            Query::Host => "host",
                            Query::Substrate => "substrate",
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
    fn structures_and_terms_compile_and_count_apart() {
        // Spec rework M7.
        let program = compile(
            "lsystem p 1;
             module A;
             organ pinna leaf as leaflet area 0.2;
             organ scrap leaf as piece;
             organ leaf leaf;
             structure frond leaf;
             axiom A;
             rule A -> F [ frond F pinna ] leaf A;
             rule frond : age > 2 -> ;",
        )
        .unwrap();
        assert_eq!(
            program.organ_terms().collect::<Vec<_>>(),
            [Some("leaflet"), Some("piece"), None]
        );
        assert_eq!(
            program.structures().collect::<Vec<_>>(),
            [("frond", "leaf")]
        );
        // The structure is a symbol after the organs, of no arguments, and
        // counts in a block of its own: the modules after it keep theirs.
        let frond = program.symbol_id("frond").unwrap();
        assert_eq!(program.symbols[usize::from(frond)].arity, 0);
        let a = program.symbol_id("A").unwrap();
        let ordinals: Vec<u32> = program.productions[usize::from(a)][0]
            .successor
            .iter()
            .map(|item| item.ordinal)
            .collect();
        assert_eq!(
            ordinals,
            [
                0,
                TURTLE_ORDINAL,
                STRUCTURE_ORDINAL,
                1,
                2,
                TURTLE_ORDINAL | 1,
                3,
                4
            ]
        );
        assert_eq!(program.productions[usize::from(frond)].len(), 1);
    }

    #[test]
    fn terms_and_structures_are_checked() {
        let error = |source: &str| compile(source).unwrap_err().message;
        let message = error("lsystem p 1; organ leaf leaf as frond; axiom F;");
        assert!(
            message.starts_with(
                "unknown term `frond`; an organ is piece or one of the vocabulary's organs: "
            ) && message.contains("leaflet"),
            "{message}"
        );
        let message = error("lsystem p 1; structure frond piece; axiom F;");
        assert!(
            message.starts_with("unknown term `piece`; a structure is"),
            "{message}"
        );
        let message = error("lsystem p 1; structure whole plant; axiom F;");
        assert!(message.starts_with("unknown term `plant`"), "{message}");
        let message = error("lsystem p 1; structure frond leaf; axiom frond(1);");
        assert!(message.contains("takes 0 argument(s)"), "{message}");
        let message =
            error("lsystem p 1; structure frond leaf; axiom frond; decompose frond -> F;");
        assert!(
            message.contains("cannot be decomposed or interpreted"),
            "{message}"
        );
        let message = error("lsystem p 1; structure frond leaf; module frond; axiom F;");
        assert!(message.contains("already defined"), "{message}");
        let message = error("lsystem p 1; param as = 1; axiom F;");
        assert!(message.contains("keyword"), "{message}");
    }

    /// A ring holds the items they would compile to without it: the same
    /// draws, ordinals and continuation, and where they stand.
    #[test]
    fn a_ring_holds_the_items_it_would_without_it() {
        let ringed = compile(
            "lsystem p 1; module A(n); organ leaf leaf; axiom A(2);
             rule A(n) -> F ring(n) [ &(30) leaf(uniform(0.5, 1)) ] ring(n + 1) leaf(1, 0, 40) A(n);",
        )
        .unwrap();
        let plain = compile(
            "lsystem p 1; module A(n); organ leaf leaf; axiom A(2);
             rule A(n) -> F [ &(30) leaf(uniform(0.5, 1)) ] leaf(1, 0, 40) A(n);",
        )
        .unwrap();
        let a = usize::from(ringed.symbol_id("A").unwrap());
        let (ringed, plain) = (&ringed.productions[a][0], &plain.productions[a][0]);
        assert_eq!(ringed.successor, plain.successor);
        assert_eq!(ringed.continuation, plain.continuation);
        let held: Vec<(usize, usize)> = ringed
            .rings
            .iter()
            .map(|ring| (ring.at, ring.len))
            .collect();
        assert_eq!(held, [(1, 4), (5, 1)]);
        assert!(plain.rings.is_empty());
    }

    #[test]
    fn rings_stand_only_where_they_can_hold_their_items() {
        let cases = [
            ("lsystem p 1; axiom ring(2) F;", "followed by a bracket"),
            (
                "lsystem p 1; organ leaf leaf; axiom ring(2) leaf(1);",
                "followed by a bracket",
            ),
            ("lsystem p 1; axiom ring(2);", "followed by a bracket"),
            ("lsystem p 1; axiom ring(2, 3) [ F ];", "takes one argument"),
            ("lsystem p 1; axiom ring [ F ];", "takes one argument"),
            (
                "lsystem p 1; axiom ring(2) [ F ring(2) [ F ] ];",
                "may not hold a ring",
            ),
            (
                "lsystem p 1; module A; axiom A; interpret A -> ring(2) [ F ];",
                "cannot stand in an interpretation rule",
            ),
            ("lsystem p 1; axiom ring(2) [ F;", "never closed"),
            ("lsystem p 1; param ring = 2; axiom F;", "keyword"),
            ("lsystem p 1; module ring; axiom F;", "keyword"),
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
            ("lsystem p 1; tool light@9 { }; axiom F;", "does not exist"),
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
