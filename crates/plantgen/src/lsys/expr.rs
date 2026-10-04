//! Expression bytecode and its evaluator.
//!
//! Expressions compile to a postfix sequence of [`Op`]s with no jumps, so
//! evaluation time is bounded by program size: a downloaded plant program
//! cannot loop. `a ? b : c` evaluates both branches and then selects, which is
//! safe because evaluation has no side effects (random draws are pure
//! functions of their key, see [`crate::rng`]).

use crate::math;
use crate::rng::{hash_words, normal, unit};

/// Values the environment tools wrote for one module in the last step.
pub type EnvValues = [f64; ENV_FIELDS];

pub const ENV_FIELDS: usize = 17;

/// The environment fields a rule can read, grouped by the query that
/// provides them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvField {
    /// `light`: light exposure, 0 (full shade) to 1 (open sky).
    Light,
    /// `vigour`: resource the Borchert-Honda model allocated to the module.
    Vigour,
    /// `qsum`: summed light of the module's branch (its own light for a tip).
    Qsum,
    /// `nseg`: number of segments in the module's branch.
    Nseg,
    /// `ntip`: number of tips (query modules with nothing after them) in the
    /// module's branch, not counting the module itself.
    Ntip,
    /// `space`: number of attraction points the module perceives.
    Space,
    Sx,
    Sy,
    Sz,
    Px,
    Py,
    Pz,
    Hx,
    Hy,
    Hz,
    /// `order`: bracket depth of the module, which is its branch order.
    Order,
    /// `height`: current height of the whole plant.
    Height,
}

impl EnvField {
    pub const ALL: [(Self, &'static str); ENV_FIELDS] = [
        (Self::Light, "light"),
        (Self::Vigour, "vigour"),
        (Self::Qsum, "qsum"),
        (Self::Nseg, "nseg"),
        (Self::Ntip, "ntip"),
        (Self::Space, "space"),
        (Self::Sx, "sx"),
        (Self::Sy, "sy"),
        (Self::Sz, "sz"),
        (Self::Px, "px"),
        (Self::Py, "py"),
        (Self::Pz, "pz"),
        (Self::Hx, "hx"),
        (Self::Hy, "hy"),
        (Self::Hz, "hz"),
        (Self::Order, "order"),
        (Self::Height, "height"),
    ];

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, text)| *text == name)
            .map(|(field, _)| *field)
    }

    /// The query a module must declare to read this field.
    #[must_use]
    pub fn query(self) -> Query {
        match self {
            Self::Light => Query::Light,
            Self::Vigour | Self::Qsum | Self::Nseg | Self::Ntip => Query::Vigour,
            Self::Space | Self::Sx | Self::Sy | Self::Sz => Query::Space,
            _ => Query::Position,
        }
    }
}

/// Environment queries a module can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    Light,
    Vigour,
    Space,
    Position,
}

impl Query {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "light" => Self::Light,
            "vigour" => Self::Vigour,
            "space" => Self::Space,
            "position" => Self::Position,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn bit(self) -> u8 {
        match self {
            Self::Light => 1,
            Self::Vigour => 2,
            Self::Space => 4,
            Self::Position => 8,
        }
    }
}

/// Built-in variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    /// Time since the plant germinated, in years.
    T,
    /// Length of one derivation step, in years.
    Dt,
    /// Age of the module being rewritten: `t` minus its birth time.
    Age,
    /// Index of the derivation step.
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Min,
    Max,
    Clamp,
    Abs,
    Sqrt,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Exp,
    Ln,
    Log10,
    Pow,
    Floor,
    Ceil,
    Round,
    Frac,
    Sign,
    Lerp,
    Smoothstep,
    Step,
    Mod,
    Rad,
    Deg,
    Richards,
}

impl Func {
    pub const ALL: [(Self, &'static str, usize); 28] = [
        (Self::Min, "min", 2),
        (Self::Max, "max", 2),
        (Self::Clamp, "clamp", 3),
        (Self::Abs, "abs", 1),
        (Self::Sqrt, "sqrt", 1),
        (Self::Sin, "sin", 1),
        (Self::Cos, "cos", 1),
        (Self::Tan, "tan", 1),
        (Self::Asin, "asin", 1),
        (Self::Acos, "acos", 1),
        (Self::Atan, "atan", 1),
        (Self::Atan2, "atan2", 2),
        (Self::Exp, "exp", 1),
        (Self::Ln, "ln", 1),
        (Self::Log10, "log10", 1),
        (Self::Pow, "pow", 2),
        (Self::Floor, "floor", 1),
        (Self::Ceil, "ceil", 1),
        (Self::Round, "round", 1),
        (Self::Frac, "frac", 1),
        (Self::Sign, "sign", 1),
        (Self::Lerp, "lerp", 3),
        (Self::Smoothstep, "smoothstep", 3),
        (Self::Step, "step", 2),
        (Self::Mod, "mod", 2),
        (Self::Rad, "rad", 1),
        (Self::Deg, "deg", 1),
        (Self::Richards, "richards", 4),
    ];

    #[must_use]
    pub fn from_name(name: &str) -> Option<(Self, usize)> {
        Self::ALL
            .iter()
            .find(|(_, text, _)| *text == name)
            .map(|(func, _, arity)| (*func, *arity))
    }

    fn apply(self, a: &[f64]) -> f64 {
        match self {
            Self::Min => a[0].min(a[1]),
            Self::Max => a[0].max(a[1]),
            Self::Clamp => a[0].max(a[1]).min(a[2]),
            Self::Abs => a[0].abs(),
            Self::Sqrt => math::sqrt(a[0]),
            Self::Sin => math::sin(a[0]),
            Self::Cos => math::cos(a[0]),
            Self::Tan => math::tan(a[0]),
            Self::Asin => math::asin(a[0]),
            Self::Acos => math::acos(a[0]),
            Self::Atan => math::atan(a[0]),
            Self::Atan2 => math::atan2(a[0], a[1]),
            Self::Exp => math::exp(a[0]),
            Self::Ln => math::ln(a[0]),
            Self::Log10 => math::log10(a[0]),
            Self::Pow => math::pow(a[0], a[1]),
            Self::Floor => a[0].floor(),
            Self::Ceil => a[0].ceil(),
            Self::Round => a[0].round(),
            Self::Frac => a[0] - a[0].floor(),
            Self::Sign => {
                if a[0] > 0.0 {
                    1.0
                } else if a[0] < 0.0 {
                    -1.0
                } else {
                    a[0]
                }
            }
            Self::Lerp => math::lerp(a[0], a[1], a[2]),
            Self::Smoothstep => math::smoothstep(a[0], a[1], a[2]),
            Self::Step => {
                if a[1] < a[0] {
                    0.0
                } else {
                    1.0
                }
            }
            Self::Mod => a[0] - a[1] * (a[0] / a[1]).floor(),
            Self::Rad => math::radians(a[0]),
            Self::Deg => math::degrees(a[0]),
            // Chapman-Richards growth curve: hmax * (1 - e^(-k t))^c.
            Self::Richards => {
                if a[0] <= 0.0 {
                    0.0
                } else {
                    a[1] * math::pow(1.0 - math::exp(-a[2] * a[0]), a[3])
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Const(f64),
    /// A program parameter.
    Global(u16),
    /// A parameter of the module being rewritten.
    Local(u8),
    Var(Var),
    Env(EnvField),
    Neg,
    Not,
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
    /// Pops `otherwise`, `then`, `condition`.
    Select,
    /// A function call with its argument count.
    Call(Func, u8),
    /// `rand()` at a numbered call site.
    Rand(u32),
    /// `rand(k)`: pops `k`.
    RandKey,
    /// `gauss()` at a numbered call site.
    Gauss(u32),
    /// `gauss(k)`: pops `k`.
    GaussKey,
    /// `uniform(lo, hi)` at a numbered call site: pops `hi`, `lo`.
    Uniform(u32),
}

/// One compiled expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Code(pub Box<[Op]>);

impl Code {
    #[must_use]
    pub fn constant(value: f64) -> Self {
        Self(Box::new([Op::Const(value)]))
    }

    /// The value if the expression is a single constant.
    #[must_use]
    pub fn as_constant(&self) -> Option<f64> {
        match *self.0 {
            [Op::Const(value)] => Some(value),
            _ => None,
        }
    }
}

/// Everything an expression can read.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub globals: &'a [f64],
    pub locals: &'a [f64],
    pub env: &'a EnvValues,
    pub t: f64,
    pub dt: f64,
    pub age: f64,
    pub step: f64,
    /// Key of every random draw: the module's lineage and, for productions,
    /// the derivation step.
    pub key: u64,
}

pub const NO_ENV: EnvValues = [0.0; ENV_FIELDS];

const SALT_RAND: u64 = 0x7261_6e64;
const SALT_RAND_KEY: u64 = 0x7261_6e6b;
const SALT_GAUSS: u64 = 0x6761_7573;
const SALT_GAUSS_KEY: u64 = 0x6761_756b;
const SALT_UNIFORM: u64 = 0x756e_6966;

/// `0` and NaN are false; every other value is true.
#[must_use]
pub fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

fn flag(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

/// The language's `==`: exact, as plant programs compare whole-number
/// counters and flags. NaN equals nothing.
#[allow(clippy::float_cmp)]
fn equal(left: f64, right: f64) -> bool {
    left == right
}

fn key_bits(value: f64) -> u64 {
    // -0.0 and 0.0 name the same draw.
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// Evaluate `code`; `stack` is scratch space reused between calls.
#[must_use]
pub fn eval(code: &Code, scope: &Scope<'_>, stack: &mut Vec<f64>) -> f64 {
    stack.clear();
    for op in &*code.0 {
        let value = match *op {
            Op::Const(value) => value,
            Op::Global(index) => scope.globals[usize::from(index)],
            Op::Local(index) => scope.locals[usize::from(index)],
            Op::Var(var) => match var {
                Var::T => scope.t,
                Var::Dt => scope.dt,
                Var::Age => scope.age,
                Var::Step => scope.step,
            },
            Op::Env(field) => scope.env[field as usize],
            Op::Neg => -pop(stack),
            Op::Not => flag(!truthy(pop(stack))),
            Op::Select => {
                let otherwise = pop(stack);
                let then = pop(stack);
                if truthy(pop(stack)) { then } else { otherwise }
            }
            Op::Call(func, arity) => {
                let start = stack.len().saturating_sub(usize::from(arity));
                let value = func.apply(&stack[start..]);
                stack.truncate(start);
                value
            }
            Op::Rand(site) => unit(hash_words(&[scope.key, SALT_RAND, u64::from(site)])),
            Op::RandKey => {
                let key = key_bits(pop(stack));
                unit(hash_words(&[scope.key, SALT_RAND_KEY, key]))
            }
            Op::Gauss(site) => normal(hash_words(&[scope.key, SALT_GAUSS, u64::from(site)])),
            Op::GaussKey => {
                let key = key_bits(pop(stack));
                normal(hash_words(&[scope.key, SALT_GAUSS_KEY, key]))
            }
            Op::Uniform(site) => {
                let hi = pop(stack);
                let lo = pop(stack);
                lo + (hi - lo) * unit(hash_words(&[scope.key, SALT_UNIFORM, u64::from(site)]))
            }
            binary => {
                let right = pop(stack);
                let left = pop(stack);
                match binary {
                    Op::Add => left + right,
                    Op::Sub => left - right,
                    Op::Mul => left * right,
                    Op::Div => left / right,
                    Op::Pow => math::pow(left, right),
                    Op::Less => flag(left < right),
                    Op::LessEq => flag(left <= right),
                    Op::Greater => flag(left > right),
                    Op::GreaterEq => flag(left >= right),
                    Op::Equal => flag(equal(left, right)),
                    Op::NotEqual => flag(!equal(left, right)),
                    Op::And => flag(truthy(left) && truthy(right)),
                    Op::Or => flag(truthy(left) || truthy(right)),
                    _ => unreachable!("every non-binary op is handled above"),
                }
            }
        };
        stack.push(value);
    }
    stack.pop().unwrap_or(f64::NAN)
}

fn pop(stack: &mut Vec<f64>) -> f64 {
    stack.pop().unwrap_or(f64::NAN)
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn scope() -> Scope<'static> {
        Scope {
            globals: &[2.0, 10.0],
            locals: &[3.0],
            env: &NO_ENV,
            t: 5.0,
            dt: 1.0,
            age: 2.0,
            step: 5.0,
            key: 42,
        }
    }

    #[test]
    fn evaluates_postfix_arithmetic_and_selection() {
        let mut stack = Vec::new();
        // (g0 + l0) * 2 > 9 ? t : -1
        let code = Code(Box::new([
            Op::Global(0),
            Op::Local(0),
            Op::Add,
            Op::Const(2.0),
            Op::Mul,
            Op::Const(9.0),
            Op::Greater,
            Op::Var(Var::T),
            Op::Const(-1.0),
            Op::Select,
        ]));
        assert_eq!(eval(&code, &scope(), &mut stack), 5.0);
        let code = Code(Box::new([
            Op::Const(5.0),
            Op::Const(0.0),
            Op::Const(1.0),
            Op::Call(Func::Clamp, 3),
        ]));
        assert_eq!(eval(&code, &scope(), &mut stack), 1.0);
    }

    #[test]
    fn random_draws_depend_only_on_key_and_site() {
        let mut stack = Vec::new();
        let a = eval(&Code(Box::new([Op::Rand(0)])), &scope(), &mut stack);
        let b = eval(&Code(Box::new([Op::Rand(0)])), &scope(), &mut stack);
        let c = eval(&Code(Box::new([Op::Rand(1)])), &scope(), &mut stack);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let keyed = Code(Box::new([Op::Const(-0.0), Op::RandKey]));
        let zero = Code(Box::new([Op::Const(0.0), Op::RandKey]));
        assert_eq!(
            eval(&keyed, &scope(), &mut stack),
            eval(&zero, &scope(), &mut stack)
        );
    }

    #[test]
    fn nan_is_false_and_modulo_is_floored() {
        assert!(!truthy(f64::NAN));
        assert!(truthy(-0.5));
        assert_eq!(Func::Mod.apply(&[-1.0, 3.0]), 2.0);
        assert_eq!(Func::Richards.apply(&[-1.0, 30.0, 0.1, 2.0]), 0.0);
        assert!((Func::Richards.apply(&[1e6, 30.0, 0.1, 2.0]) - 30.0).abs() < 1e-9);
    }
}
