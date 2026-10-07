//! Tokenizer for the plant L-system language.
//!
//! Turtle commands reuse arithmetic characters (`+ - / & ^ ! | % $ \`), so the
//! lexer only splits text into tokens; the parser decides from context whether
//! `/` is a division or the roll-right command.

use std::fmt;

/// A 1-based line and column in the program text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Span {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semicolon,
    Colon,
    Assign,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Backslash,
    Amp,
    AndAnd,
    Caret,
    Pipe,
    OrOr,
    Dollar,
    Bang,
    Percent,
    Question,
    At,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    EqEq,
    NotEq,
    End,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Ident(name) => return write!(f, "`{name}`"),
            Self::Number(value) => return write!(f, "number {value}"),
            Self::LParen => "`(`",
            Self::RParen => "`)`",
            Self::LBracket => "`[`",
            Self::RBracket => "`]`",
            Self::LBrace => "`{`",
            Self::RBrace => "`}`",
            Self::Comma => "`,`",
            Self::Semicolon => "`;`",
            Self::Colon => "`:`",
            Self::Assign => "`=`",
            Self::Arrow => "`->`",
            Self::Plus => "`+`",
            Self::Minus => "`-`",
            Self::Star => "`*`",
            Self::Slash => "`/`",
            Self::Backslash => "`\\`",
            Self::Amp => "`&`",
            Self::AndAnd => "`&&`",
            Self::Caret => "`^`",
            Self::Pipe => "`|`",
            Self::OrOr => "`||`",
            Self::Dollar => "`$`",
            Self::Bang => "`!`",
            Self::Percent => "`%`",
            Self::Question => "`?`",
            Self::At => "`@`",
            Self::Less => "`<`",
            Self::LessEq => "`<=`",
            Self::Greater => "`>`",
            Self::GreaterEq => "`>=`",
            Self::EqEq => "`==`",
            Self::NotEq => "`!=`",
            Self::End => "end of program",
        };
        f.write_str(text)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub token: Token,
    pub span: Span,
}

/// Columns a run of `count` characters spans.
fn width(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The end of the number starting at `start`: digits and points, then an
/// optional exponent.
fn number_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
        index += 1;
    }
    if index < chars.len() && (chars[index] == 'e' || chars[index] == 'E') {
        let mut lookahead = index + 1;
        if lookahead < chars.len() && (chars[lookahead] == '+' || chars[lookahead] == '-') {
            lookahead += 1;
        }
        if lookahead < chars.len() && chars[lookahead].is_ascii_digit() {
            index = lookahead;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
        }
    }
    index
}

/// An operator or punctuation token starting with `c`, and its width.
fn punctuation(c: char, next: Option<char>) -> Option<(Token, usize)> {
    Some(match (c, next) {
        ('-', Some('>')) => (Token::Arrow, 2),
        ('&', Some('&')) => (Token::AndAnd, 2),
        ('|', Some('|')) => (Token::OrOr, 2),
        ('<', Some('=')) => (Token::LessEq, 2),
        ('>', Some('=')) => (Token::GreaterEq, 2),
        ('=', Some('=')) => (Token::EqEq, 2),
        ('!', Some('=')) => (Token::NotEq, 2),
        ('(', _) => (Token::LParen, 1),
        (')', _) => (Token::RParen, 1),
        ('[', _) => (Token::LBracket, 1),
        (']', _) => (Token::RBracket, 1),
        ('{', _) => (Token::LBrace, 1),
        ('}', _) => (Token::RBrace, 1),
        (',', _) => (Token::Comma, 1),
        (';', _) => (Token::Semicolon, 1),
        (':', _) => (Token::Colon, 1),
        ('=', _) => (Token::Assign, 1),
        ('+', _) => (Token::Plus, 1),
        ('-', _) => (Token::Minus, 1),
        ('*', _) => (Token::Star, 1),
        ('/', _) => (Token::Slash, 1),
        ('\\', _) => (Token::Backslash, 1),
        ('&', _) => (Token::Amp, 1),
        ('^', _) => (Token::Caret, 1),
        ('|', _) => (Token::Pipe, 1),
        ('$', _) => (Token::Dollar, 1),
        ('!', _) => (Token::Bang, 1),
        ('%', _) => (Token::Percent, 1),
        ('?', _) => (Token::Question, 1),
        ('@', _) => (Token::At, 1),
        ('<', _) => (Token::Less, 1),
        ('>', _) => (Token::Greater, 1),
        _ => return None,
    })
}

/// Split program text into tokens. `#` starts a comment that runs to the end
/// of the line.
///
/// # Errors
///
/// Fails at the first character that starts no token, or a malformed
/// number, with its position.
pub fn tokenize(source: &str) -> Result<Vec<Spanned>, (Span, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1_u32;
    let mut column = 1_u32;
    while index < chars.len() {
        let c = chars[index];
        let span = Span { line, column };
        if c == '\n' {
            index += 1;
            line += 1;
            column = 1;
            continue;
        }
        if c == '#' {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        let (token, end) = if c.is_whitespace() {
            (None, index + 1)
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut end = index;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            (Some(Token::Ident(chars[index..end].iter().collect())), end)
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(index + 1).is_some_and(char::is_ascii_digit))
        {
            let end = number_end(&chars, index);
            let text: String = chars[index..end].iter().collect();
            let value: f64 = text
                .parse()
                .map_err(|_| (span, format!("invalid number `{text}`")))?;
            (Some(Token::Number(value)), end)
        } else {
            let (token, count) = punctuation(c, chars.get(index + 1).copied())
                .ok_or_else(|| (span, format!("unexpected character `{c}`")))?;
            (Some(token), index + count)
        };
        column = column.saturating_add(width(end - index));
        index = end;
        if let Some(token) = token {
            tokens.push(Spanned { token, span });
        }
    }
    tokens.push(Spanned {
        token: Token::End,
        span: Span { line, column },
    });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_numbers_operators_and_comments() {
        let tokens = tokenize("rule A(x) : x >= 1.5e-1 -> F(x/2) &(30) # comment\n;").unwrap();
        let kinds: Vec<Token> = tokens.into_iter().map(|t| t.token).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Ident("rule".into()),
                Token::Ident("A".into()),
                Token::LParen,
                Token::Ident("x".into()),
                Token::RParen,
                Token::Colon,
                Token::Ident("x".into()),
                Token::GreaterEq,
                Token::Number(0.15),
                Token::Arrow,
                Token::Ident("F".into()),
                Token::LParen,
                Token::Ident("x".into()),
                Token::Slash,
                Token::Number(2.0),
                Token::RParen,
                Token::Amp,
                Token::LParen,
                Token::Number(30.0),
                Token::RParen,
                Token::Semicolon,
                Token::End,
            ]
        );
    }

    #[test]
    fn reports_line_and_column_of_bad_characters() {
        let error = tokenize("param a = 1;\n  param b = 2 ` ;").unwrap_err();
        assert_eq!(
            error.0,
            Span {
                line: 2,
                column: 15
            }
        );
    }
}
