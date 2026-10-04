//! Recursive-descent parser for plant programs.
//!
//! ```text
//! program    = header { statement }
//! header     = "lsystem" name integer ";"
//! statement  = "param" name "=" expr ";"
//!            | "module" name [ "(" names ")" ] [ "queries" names ] ";"
//!            | "organ" name kind [ "area" expr ] ";"
//!            | "tool" name "@" integer "{" [ setting { "," setting } [ "," ] ] "}" ";"
//!            | "axiom" { item } ";"
//!            | ( "rule" | "decompose" | "interpret" ) name [ "(" names ")" ]
//!                  [ ":" expr ] [ "weight" expr ] "->" { item } ";"
//! item       = ( name | turtle ) [ "(" [ expr { "," expr } ] ")" ]
//! turtle     = "[" | "]" | "+" | "-" | "&" | "^" | "/" | "\" | "|" | "$" | "!" | "%"
//! expr       = or [ "?" expr ":" expr ]
//! or         = and { "||" and }
//! and        = compare { "&&" compare }
//! compare    = sum [ ( "<" | "<=" | ">" | ">=" | "==" | "!=" ) sum ]
//! sum        = product { ( "+" | "-" ) product }
//! product    = unary { ( "*" | "/" ) unary }
//! unary      = ( "-" | "!" ) unary | power
//! power      = primary [ "^" unary ]
//! primary    = number | name | name "(" [ expr { "," expr } ] ")" | "(" expr ")"
//! ```
//!
//! Inside an expression `^` is a power and `/` a division; between modules
//! they are the pitch-up and roll commands.

use super::ProgramError;
use super::ast::{
    BinaryOp, Expr, ModuleCall, ModuleDecl, OrganDecl, ParamDecl, ProgramAst, Rule, RuleKind,
    ToolDecl, UnaryOp,
};
use super::lexer::{Span, Spanned, Token, tokenize};

/// Parse program text into a syntax tree.
///
/// # Errors
///
/// Returns the first lexical or syntax error with its position.
pub fn parse(source: &str) -> Result<ProgramAst, ProgramError> {
    let tokens = tokenize(source).map_err(|(span, message)| ProgramError { span, message })?;
    Parser { tokens, index: 0 }.program()
}

struct Parser {
    tokens: Vec<Spanned>,
    index: usize,
}

type Parsed<T> = Result<T, ProgramError>;

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.index].token
    }

    fn span(&self) -> Span {
        self.tokens[self.index].span
    }

    fn advance(&mut self) -> Spanned {
        let token = self.tokens[self.index].clone();
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
        token
    }

    fn error<T>(&self, message: impl Into<String>) -> Parsed<T> {
        Err(ProgramError {
            span: self.span(),
            message: message.into(),
        })
    }

    fn expect(&mut self, expected: &Token, context: &str) -> Parsed<Span> {
        if self.peek() == expected {
            Ok(self.advance().span)
        } else {
            self.error(format!(
                "expected {expected} {context}, found {}",
                self.peek()
            ))
        }
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == token {
            self.advance();
            true
        } else {
            false
        }
    }

    fn name(&mut self, what: &str) -> Parsed<(String, Span)> {
        let span = self.span();
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok((name, span))
            }
            other => self.error(format!("expected {what}, found {other}")),
        }
    }

    fn keyword(&mut self, keyword: &str) -> bool {
        if matches!(self.peek(), Token::Ident(name) if name == keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn integer(&mut self, what: &str) -> Parsed<u32> {
        match *self.peek() {
            Token::Number(value)
                if value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX) =>
            {
                self.advance();
                // Checked above: a whole number inside the u32 range.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Ok(value as u32)
            }
            _ => self.error(format!(
                "expected {what} (a whole number), found {}",
                self.peek()
            )),
        }
    }

    fn program(mut self) -> Parsed<ProgramAst> {
        if !self.keyword("lsystem") {
            return self.error("a program starts with `lsystem <name> <revision>;`");
        }
        let (name, _) = self.name("the program name")?;
        let revision = self.integer("the program revision")?;
        self.expect(&Token::Semicolon, "after the lsystem header")?;
        let mut ast = ProgramAst {
            name,
            revision,
            params: Vec::new(),
            modules: Vec::new(),
            organs: Vec::new(),
            tools: Vec::new(),
            axiom: Vec::new(),
            axiom_span: Span::default(),
            rules: Vec::new(),
        };
        let mut have_axiom = false;
        while *self.peek() != Token::End {
            let span = self.span();
            let (keyword, _) = self.name("a statement")?;
            match keyword.as_str() {
                "param" => ast.params.push(self.param(span)?),
                "module" => ast.modules.push(self.module(span)?),
                "organ" => ast.organs.push(self.organ(span)?),
                "tool" => ast.tools.push(self.tool(span)?),
                "axiom" => {
                    if have_axiom {
                        return Err(ProgramError {
                            span,
                            message: "a program has exactly one axiom".into(),
                        });
                    }
                    have_axiom = true;
                    ast.axiom = self.successor()?;
                    ast.axiom_span = span;
                }
                "rule" => ast.rules.push(self.rule(RuleKind::Production, span)?),
                "decompose" => ast.rules.push(self.rule(RuleKind::Decomposition, span)?),
                "interpret" => ast.rules.push(self.rule(RuleKind::Interpretation, span)?),
                other => {
                    return Err(ProgramError {
                        span,
                        message: format!(
                            "unknown statement `{other}`; expected param, module, organ, \
                             tool, axiom, rule, decompose or interpret"
                        ),
                    });
                }
            }
        }
        if !have_axiom {
            return self.error("the program has no axiom");
        }
        Ok(ast)
    }

    fn param(&mut self, span: Span) -> Parsed<ParamDecl> {
        let (name, _) = self.name("a parameter name")?;
        self.expect(&Token::Assign, "after the parameter name")?;
        let value = self.expr()?;
        self.expect(&Token::Semicolon, "after the parameter value")?;
        Ok(ParamDecl { name, value, span })
    }

    fn name_list(&mut self, what: &str) -> Parsed<Vec<(String, Span)>> {
        let mut names = vec![self.name(what)?];
        while self.eat(&Token::Comma) {
            names.push(self.name(what)?);
        }
        Ok(names)
    }

    fn param_names(&mut self) -> Parsed<Vec<(String, Span)>> {
        if !self.eat(&Token::LParen) {
            return Ok(Vec::new());
        }
        if self.eat(&Token::RParen) {
            return Ok(Vec::new());
        }
        let names = self.name_list("a parameter name")?;
        self.expect(&Token::RParen, "after the parameter names")?;
        Ok(names)
    }

    fn module(&mut self, span: Span) -> Parsed<ModuleDecl> {
        let (name, _) = self.name("a module name")?;
        let params = self.param_names()?;
        let queries = if self.keyword("queries") {
            self.name_list("an environment query (light, vigour, space or position)")?
        } else {
            Vec::new()
        };
        self.expect(&Token::Semicolon, "after the module declaration")?;
        Ok(ModuleDecl {
            name,
            params,
            queries,
            span,
        })
    }

    fn organ(&mut self, span: Span) -> Parsed<OrganDecl> {
        let (name, _) = self.name("an organ name")?;
        let kind = self.name("an organ kind (foliage, leaf, flower, fruit or cone)")?;
        let area = if self.keyword("area") {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&Token::Semicolon, "after the organ declaration")?;
        Ok(OrganDecl {
            name,
            kind,
            area,
            span,
        })
    }

    fn tool(&mut self, span: Span) -> Parsed<ToolDecl> {
        let (name, _) = self.name("a tool name")?;
        self.expect(&Token::At, "between the tool name and its version")?;
        let version = self.integer("the tool version")?;
        self.expect(&Token::LBrace, "to open the tool settings")?;
        let mut settings = Vec::new();
        while *self.peek() != Token::RBrace {
            let (key, key_span) = self.name("a tool setting")?;
            self.expect(&Token::Assign, "after the setting name")?;
            let value = self.expr()?;
            settings.push((key, value, key_span));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace, "to close the tool settings")?;
        self.expect(&Token::Semicolon, "after the tool settings")?;
        Ok(ToolDecl {
            name,
            version,
            settings,
            span,
        })
    }

    fn rule(&mut self, kind: RuleKind, span: Span) -> Parsed<Rule> {
        let (symbol, _) = self.predecessor_symbol()?;
        let params = self.param_names()?;
        let condition = if self.eat(&Token::Colon) {
            Some(self.expr()?)
        } else {
            None
        };
        let weight = if self.keyword("weight") {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&Token::Arrow, "between the predecessor and its successor")?;
        let successor = self.successor()?;
        Ok(Rule {
            kind,
            symbol,
            params,
            condition,
            weight,
            successor,
            span,
        })
    }

    /// A rule may rewrite a named module or a parametric turtle command.
    fn predecessor_symbol(&mut self) -> Parsed<(String, Span)> {
        let span = self.span();
        if let Some(text) = turtle_text(self.peek()) {
            self.advance();
            return Ok((text.to_string(), span));
        }
        self.name("the module a rule rewrites")
    }

    fn successor(&mut self) -> Parsed<Vec<ModuleCall>> {
        let mut items = Vec::new();
        loop {
            let span = self.span();
            let symbol = match self.peek().clone() {
                Token::Semicolon => {
                    self.advance();
                    return Ok(items);
                }
                Token::Ident(name) => {
                    self.advance();
                    name
                }
                Token::AndAnd | Token::OrOr => {
                    let text = if *self.peek() == Token::AndAnd {
                        "&"
                    } else {
                        "|"
                    };
                    self.advance();
                    for _ in 0..2 {
                        items.push(ModuleCall {
                            symbol: text.to_string(),
                            args: Vec::new(),
                            span,
                        });
                    }
                    continue;
                }
                token => {
                    let Some(text) = turtle_text(&token) else {
                        return self.error(format!(
                            "expected a module, a turtle command or `;`, found {token}"
                        ));
                    };
                    self.advance();
                    text.to_string()
                }
            };
            let args = if self.eat(&Token::LParen) {
                self.arguments()?
            } else {
                Vec::new()
            };
            items.push(ModuleCall { symbol, args, span });
        }
    }

    /// Arguments after an opening parenthesis, through the closing one.
    fn arguments(&mut self) -> Parsed<Vec<Expr>> {
        let mut args = Vec::new();
        if self.eat(&Token::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if self.eat(&Token::Comma) {
                continue;
            }
            self.expect(&Token::RParen, "after the arguments")?;
            return Ok(args);
        }
    }

    fn expr(&mut self) -> Parsed<Expr> {
        let condition = self.or()?;
        if self.eat(&Token::Question) {
            let then = self.expr()?;
            self.expect(&Token::Colon, "in a `? :` expression")?;
            let otherwise = self.expr()?;
            return Ok(Expr::Select(
                Box::new(condition),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
        Ok(condition)
    }

    fn or(&mut self) -> Parsed<Expr> {
        let mut left = self.and()?;
        while self.eat(&Token::OrOr) {
            let right = self.and()?;
            left = Expr::Binary(BinaryOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Parsed<Expr> {
        let mut left = self.compare()?;
        while self.eat(&Token::AndAnd) {
            let right = self.compare()?;
            left = Expr::Binary(BinaryOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn compare(&mut self) -> Parsed<Expr> {
        let left = self.sum()?;
        let op = match self.peek() {
            Token::Less => BinaryOp::Less,
            Token::LessEq => BinaryOp::LessEq,
            Token::Greater => BinaryOp::Greater,
            Token::GreaterEq => BinaryOp::GreaterEq,
            Token::EqEq => BinaryOp::Equal,
            Token::NotEq => BinaryOp::NotEqual,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.sum()?;
        Ok(Expr::Binary(op, Box::new(left), Box::new(right)))
    }

    fn sum(&mut self) -> Parsed<Expr> {
        let mut left = self.product()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.product()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn product(&mut self) -> Parsed<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => return Ok(left),
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Parsed<Expr> {
        if self.eat(&Token::Minus) {
            return Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.unary()?)));
        }
        if self.eat(&Token::Bang) {
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(self.unary()?)));
        }
        self.power()
    }

    fn power(&mut self) -> Parsed<Expr> {
        let base = self.primary()?;
        if self.eat(&Token::Caret) {
            let exponent = self.unary()?;
            return Ok(Expr::Binary(
                BinaryOp::Pow,
                Box::new(base),
                Box::new(exponent),
            ));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Parsed<Expr> {
        let span = self.span();
        match self.peek().clone() {
            Token::Number(value) => {
                self.advance();
                Ok(Expr::Number(value))
            }
            Token::Ident(name) => {
                self.advance();
                if *self.peek() == Token::LParen {
                    self.advance();
                    let args = self.arguments()?;
                    return Ok(Expr::Call(name, args, span));
                }
                Ok(Expr::Name(name, span))
            }
            Token::LParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(&Token::RParen, "to close the parenthesis")?;
                Ok(inner)
            }
            other => self.error(format!("expected a value, found {other}")),
        }
    }
}

/// The text of a turtle-command token, if it is one.
fn turtle_text(token: &Token) -> Option<&'static str> {
    Some(match token {
        Token::LBracket => "[",
        Token::RBracket => "]",
        Token::Plus => "+",
        Token::Minus => "-",
        Token::Amp => "&",
        Token::Caret => "^",
        Token::Slash => "/",
        Token::Backslash => "\\",
        Token::Pipe => "|",
        Token::Dollar => "$",
        Token::Bang => "!",
        Token::Percent => "%",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_complete_program() {
        let ast = parse(
            "lsystem test 3;
             param len = 0.5 * 2;
             module A(o) queries light, vigour;
             organ leaf leaf area 0.01;
             tool light@1 { cell = 0.5, depth = 4, };
             axiom !(0.01) A(0);
             rule A(o) : light > 0.2 && o < 3 weight 2 -> F(len) [ +(30) A(o + 1) ] leaf(1) A(o);
             decompose W(n) : n > 0 -> [ & A(1) ] W(n - 1);
             interpret leaf(s) -> ;",
        )
        .unwrap();
        assert_eq!(ast.name, "test");
        assert_eq!(ast.revision, 3);
        assert_eq!(ast.params.len(), 1);
        assert_eq!(ast.modules[0].queries.len(), 2);
        assert_eq!(ast.tools[0].settings.len(), 2);
        assert_eq!(ast.axiom.len(), 2);
        assert_eq!(ast.rules.len(), 3);
        let rule = &ast.rules[0];
        assert!(rule.weight.is_some());
        let symbols: Vec<&str> = rule.successor.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, ["F", "[", "+", "A", "]", "leaf", "A"]);
        assert!(ast.rules[2].successor.is_empty());
    }

    #[test]
    fn power_binds_tighter_than_negation_and_division_is_left_associative() {
        let ast = parse("lsystem p 1; param a = -2^2; param b = 8 / 4 / 2; axiom F;").unwrap();
        assert_eq!(
            ast.params[0].value,
            Expr::Unary(
                UnaryOp::Neg,
                Box::new(Expr::Binary(
                    BinaryOp::Pow,
                    Box::new(Expr::Number(2.0)),
                    Box::new(Expr::Number(2.0))
                ))
            )
        );
        let Expr::Binary(BinaryOp::Div, left, _) = &ast.params[1].value else {
            panic!("expected a division");
        };
        assert!(matches!(**left, Expr::Binary(BinaryOp::Div, _, _)));
    }

    #[test]
    fn turtle_commands_between_modules_are_not_arithmetic() {
        let ast = parse("lsystem p 1; axiom F(1) /(137.5) ^(10) - F && F;").unwrap();
        let symbols: Vec<&str> = ast.axiom.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(symbols, ["F", "/", "^", "-", "F", "&", "&", "F"]);
    }

    #[test]
    fn errors_name_the_position() {
        let error = parse("lsystem p 1;\naxiom F(1;\n").unwrap_err();
        assert_eq!(error.span.line, 2);
        assert!(error.message.contains("expected"), "{}", error.message);
        let error = parse("lsystem p 1; param x = 1;").unwrap_err();
        assert!(error.message.contains("no axiom"));
    }
}
