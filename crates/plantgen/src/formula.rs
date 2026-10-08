//! Formulas: the expressions of trait rules (growth plan G2.2).
//!
//! A formula is written in the L-system language's expression syntax over
//! named numbers (traits): numbers, names, `+ - * / ^`, `-` and `!`,
//! comparisons, `&&`, `||`, `a ? b : c`, and the language's arithmetic
//! functions `min`, `max`, `clamp`, `abs`, `sqrt`, `exp`, `ln`, `log10`,
//! `pow`, `floor`, `ceil`, `round`, `frac`, `sign`, `lerp`, `smoothstep`
//! and `step`, which behave as they do in a program. It draws nothing at
//! random: a rule states a fact. Transcendental functions go through
//! `libm`, as everywhere in `PlantGen`. The module reads nothing but its
//! text, so the build script compiles it too.

use std::collections::BTreeSet;

/// A parsed formula.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula(Node);

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Number(f64),
    Name(String),
    Neg(Box<Node>),
    Not(Box<Node>),
    Binary(Op, Box<Node>, Box<Node>),
    Select(Box<Node>, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Func {
    Min,
    Max,
    Clamp,
    Abs,
    Sqrt,
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
}

const FUNCS: [(Func, &str, usize); 17] = [
    (Func::Min, "min", 2),
    (Func::Max, "max", 2),
    (Func::Clamp, "clamp", 3),
    (Func::Abs, "abs", 1),
    (Func::Sqrt, "sqrt", 1),
    (Func::Exp, "exp", 1),
    (Func::Ln, "ln", 1),
    (Func::Log10, "log10", 1),
    (Func::Pow, "pow", 2),
    (Func::Floor, "floor", 1),
    (Func::Ceil, "ceil", 1),
    (Func::Round, "round", 1),
    (Func::Frac, "frac", 1),
    (Func::Sign, "sign", 1),
    (Func::Lerp, "lerp", 3),
    (Func::Smoothstep, "smoothstep", 3),
    (Func::Step, "step", 2),
];

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Symbol(&'static str),
}

const SYMBOLS: [&str; 19] = [
    "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "^", "(", ")", ",", "?", ":", "!", "<",
    ">",
];

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            at += 1;
        } else if byte.is_ascii_digit() || byte == b'.' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'.') {
                at += 1;
            }
            if at < bytes.len() && (bytes[at] == b'e' || bytes[at] == b'E') {
                at += 1;
                if at < bytes.len() && (bytes[at] == b'+' || bytes[at] == b'-') {
                    at += 1;
                }
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
            }
            let number = &text[start..at];
            tokens.push(Token::Number(
                number
                    .parse()
                    .map_err(|_| format!("`{number}` is not a number"))?,
            ));
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            tokens.push(Token::Name(text[start..at].to_string()));
        } else {
            let symbol = SYMBOLS
                .iter()
                .find(|symbol| text[at..].starts_with(**symbol))
                .ok_or_else(|| format!("`{}` has no place in a formula", &text[at..=at]))?;
            tokens.push(Token::Symbol(symbol));
            at += symbol.len();
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn eat(&mut self, symbol: &str) -> bool {
        if matches!(self.tokens.get(self.at), Some(Token::Symbol(found)) if *found == symbol) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, symbol: &str) -> Result<(), String> {
        if self.eat(symbol) {
            Ok(())
        } else {
            Err(format!("`{symbol}` expected"))
        }
    }

    fn expr(&mut self) -> Result<Node, String> {
        let condition = self.or()?;
        if self.eat("?") {
            let then = self.expr()?;
            self.expect(":")?;
            let otherwise = self.expr()?;
            return Ok(Node::Select(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        Ok(condition)
    }

    fn or(&mut self) -> Result<Node, String> {
        let mut left = self.and()?;
        while self.eat("||") {
            left = Node::Binary(Op::Or, Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Node, String> {
        let mut left = self.compare()?;
        while self.eat("&&") {
            left = Node::Binary(Op::And, Box::new(left), Box::new(self.compare()?));
        }
        Ok(left)
    }

    fn compare(&mut self) -> Result<Node, String> {
        let left = self.sum()?;
        for (symbol, op) in [
            ("<=", Op::LessEq),
            (">=", Op::GreaterEq),
            ("==", Op::Equal),
            ("!=", Op::NotEqual),
            ("<", Op::Less),
            (">", Op::Greater),
        ] {
            if self.eat(symbol) {
                return Ok(Node::Binary(op, Box::new(left), Box::new(self.sum()?)));
            }
        }
        Ok(left)
    }

    fn sum(&mut self) -> Result<Node, String> {
        let mut left = self.product()?;
        loop {
            let op = if self.eat("+") {
                Op::Add
            } else if self.eat("-") {
                Op::Sub
            } else {
                return Ok(left);
            };
            left = Node::Binary(op, Box::new(left), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat("*") {
                Op::Mul
            } else if self.eat("/") {
                Op::Div
            } else {
                return Ok(left);
            };
            left = Node::Binary(op, Box::new(left), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.eat("-") {
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        let base = self.primary()?;
        if self.eat("^") {
            return Ok(Node::Binary(
                Op::Pow,
                Box::new(base),
                Box::new(self.unary()?),
            ));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, String> {
        match self.tokens.get(self.at).cloned() {
            Some(Token::Number(value)) => {
                self.at += 1;
                Ok(Node::Number(value))
            }
            Some(Token::Name(name)) => {
                self.at += 1;
                if !self.eat("(") {
                    return Ok(Node::Name(name));
                }
                let (func, arity) = FUNCS
                    .iter()
                    .find(|(_, text, _)| *text == name)
                    .map(|(func, _, arity)| (*func, *arity))
                    .ok_or_else(|| format!("`{name}` is not a function a formula may call"))?;
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                if args.len() != arity {
                    return Err(format!(
                        "`{name}` takes {arity} arguments, not {}",
                        args.len()
                    ));
                }
                Ok(Node::Call(func, args))
            }
            Some(Token::Symbol("(")) => {
                self.at += 1;
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(Token::Symbol(symbol)) => Err(format!("`{symbol}` where a value was expected")),
            None => Err("the formula ends where a value was expected".into()),
        }
    }
}

impl Formula {
    /// The formula written as `text`.
    ///
    /// # Errors
    ///
    /// Says what is wrong with it.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parser = Parser {
            tokens: tokenize(text)?,
            at: 0,
        };
        let node = parser.expr()?;
        match parser.tokens.get(parser.at) {
            None => Ok(Self(node)),
            Some(_) => Err("the formula goes on after its end".into()),
        }
    }

    /// The names it reads, sorted.
    #[must_use]
    pub fn names(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        collect(&self.0, &mut names);
        names
    }

    /// Its value, each name read through `value`; `None` if a name has no
    /// value or the result is not a finite number.
    #[must_use]
    pub fn eval(&self, value: &dyn Fn(&str) -> Option<f64>) -> Option<f64> {
        eval(&self.0, value).filter(|result| result.is_finite())
    }
}

fn collect(node: &Node, names: &mut BTreeSet<String>) {
    match node {
        Node::Number(_) => {}
        Node::Name(name) => {
            names.insert(name.clone());
        }
        Node::Neg(inner) | Node::Not(inner) => collect(inner, names),
        Node::Binary(_, left, right) => {
            collect(left, names);
            collect(right, names);
        }
        Node::Select(condition, then, otherwise) => {
            collect(condition, names);
            collect(then, names);
            collect(otherwise, names);
        }
        Node::Call(_, args) => {
            for arg in args {
                collect(arg, names);
            }
        }
    }
}

fn flag(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn truthy(value: f64) -> bool {
    value != 0.0
}

/// The language's `==`: exact, as counts and flags compare. NaN equals
/// nothing.
#[allow(clippy::float_cmp)]
fn equal(left: f64, right: f64) -> bool {
    left == right
}

fn eval(node: &Node, value: &dyn Fn(&str) -> Option<f64>) -> Option<f64> {
    Some(match node {
        Node::Number(number) => *number,
        Node::Name(name) => value(name)?,
        Node::Neg(inner) => -eval(inner, value)?,
        Node::Not(inner) => flag(!truthy(eval(inner, value)?)),
        Node::Select(condition, then, otherwise) => {
            if truthy(eval(condition, value)?) {
                eval(then, value)?
            } else {
                eval(otherwise, value)?
            }
        }
        Node::Binary(op, left, right) => {
            let (left, right) = (eval(left, value)?, eval(right, value)?);
            match op {
                Op::Add => left + right,
                Op::Sub => left - right,
                Op::Mul => left * right,
                Op::Div => left / right,
                Op::Pow => libm::pow(left, right),
                Op::Less => flag(left < right),
                Op::LessEq => flag(left <= right),
                Op::Greater => flag(left > right),
                Op::GreaterEq => flag(left >= right),
                Op::Equal => flag(equal(left, right)),
                Op::NotEqual => flag(!equal(left, right)),
                Op::And => flag(truthy(left) && truthy(right)),
                Op::Or => flag(truthy(left) || truthy(right)),
            }
        }
        Node::Call(func, args) => {
            let mut a = [0.0; 3];
            for (slot, arg) in a.iter_mut().zip(args) {
                *slot = eval(arg, value)?;
            }
            apply(*func, a)
        }
    })
}

fn apply(func: Func, a: [f64; 3]) -> f64 {
    match func {
        Func::Min => a[0].min(a[1]),
        Func::Max => a[0].max(a[1]),
        Func::Clamp => a[0].max(a[1]).min(a[2]),
        Func::Abs => a[0].abs(),
        Func::Sqrt => a[0].sqrt(),
        Func::Exp => libm::exp(a[0]),
        Func::Ln => libm::log(a[0]),
        Func::Log10 => libm::log10(a[0]),
        Func::Pow => libm::pow(a[0], a[1]),
        Func::Floor => a[0].floor(),
        Func::Ceil => a[0].ceil(),
        Func::Round => a[0].round(),
        Func::Frac => a[0] - a[0].floor(),
        Func::Sign => {
            if a[0] > 0.0 {
                1.0
            } else if a[0] < 0.0 {
                -1.0
            } else {
                a[0]
            }
        }
        Func::Lerp => a[0] + (a[1] - a[0]) * a[2],
        Func::Smoothstep => {
            let t = ((a[2] - a[0]) / (a[1] - a[0])).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        Func::Step => flag(a[1] >= a[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::Formula;

    fn value(text: &str) -> Option<f64> {
        Formula::parse(text).unwrap().eval(&|name| match name {
            "leaf_length_m" => Some(0.15),
            "leaflets" => Some(7.0),
            "short_shoots" => Some(1.0),
            _ => None,
        })
    }

    #[test]
    fn formulas_read_as_the_language_does() {
        assert_eq!(value("1 + 2 * 3"), Some(7.0));
        assert_eq!(value("(1 + 2) * 3"), Some(9.0));
        assert_eq!(value("-2 ^ 2"), Some(-4.0));
        assert_eq!(value("2 ^ 3 ^ 2"), Some(512.0));
        assert_eq!(value("leaflets > 5 ? 1 : 0"), Some(1.0));
        assert_eq!(value("short_shoots && leaflets < 3"), Some(0.0));
        assert_eq!(value("!short_shoots || leaflets == 7"), Some(1.0));
        assert_eq!(
            value("clamp(8 * pow(0.15 / leaf_length_m, 0.75), 5, 60)"),
            Some(8.0)
        );
        assert_eq!(
            value("min(1, 2) + max(1, 2) + abs(-3) + sqrt(16)"),
            Some(10.0)
        );
        assert_eq!(
            value("floor(2.5) + ceil(2.5) + round(2.5) + frac(2.25)"),
            Some(8.25)
        );
        assert_eq!(
            value("lerp(2, 4, 0.5) + smoothstep(0, 1, 0.5) + step(1, 2)"),
            Some(4.5)
        );
        assert_eq!(value("1e-3 * 1000"), Some(1.0));
        assert!((value("ln(exp(2)) + log10(100)").unwrap() - 4.0).abs() < 1e-12);
        // A name with no value, or a result that is no number, gives none.
        assert_eq!(value("leaf_width_m * 2"), None);
        assert_eq!(value("1 / 0"), None);
        assert_eq!(
            Formula::parse("leaf_length_m * leaflets + leaf_length_m")
                .unwrap()
                .names()
                .into_iter()
                .collect::<Vec<_>>(),
            ["leaf_length_m", "leaflets"]
        );
    }

    #[test]
    fn formulas_against_the_rules_are_refused() {
        for (text, wanted) in [
            ("uniform(0, 1)", "`uniform` is not a function"),
            ("min(1)", "`min` takes 2 arguments, not 1"),
            ("1 +", "ends where a value was expected"),
            ("(1 + 2", "`)` expected"),
            ("1 2", "goes on after its end"),
            ("1 # 2", "`#` has no place"),
            ("a ? 1", "`:` expected"),
            ("1.2.3", "`1.2.3` is not a number"),
        ] {
            let error = Formula::parse(text).unwrap_err();
            assert!(error.contains(wanted), "{text}: {error}");
        }
    }
}
