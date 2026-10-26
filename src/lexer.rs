//! Source text and tokens (spec §2.1, §2.4).

use crate::decimal::Decimal;
use crate::diag::{Diagnostic, Span};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Null,
    True,
    False,
    If,
    Then,
    Else,
    Do,
    Try,
    Catch,
    Fun,
    End,
    Type,
}

impl Kw {
    fn from_word(w: &str) -> Option<Kw> {
        Some(match w {
            "null" => Kw::Null,
            "true" => Kw::True,
            "false" => Kw::False,
            "if" => Kw::If,
            "then" => Kw::Then,
            "else" => Kw::Else,
            "do" => Kw::Do,
            "try" => Kw::Try,
            "catch" => Kw::Catch,
            "fun" => Kw::Fun,
            "end" => Kw::End,
            "type" => Kw::Type,
            _ => return None,
        })
    }

    pub fn text(self) -> &'static str {
        match self {
            Kw::Null => "null",
            Kw::True => "true",
            Kw::False => "false",
            Kw::If => "if",
            Kw::Then => "then",
            Kw::Else => "else",
            Kw::Do => "do",
            Kw::Try => "try",
            Kw::Catch => "catch",
            Kw::Fun => "fun",
            Kw::End => "end",
            Kw::Type => "type",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P {
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Lt,
    Gt,
    Assign,
    Pipe,
    Arrow,
    EqEq,
    NotEq,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Coalesce,
    QDot,
    QBracket,
}

#[derive(Clone, Debug)]
pub struct QueryTok {
    pub segments: Vec<String>,
    pub holes: Vec<Hole>,
}

#[derive(Clone, Debug)]
pub struct Hole {
    pub tokens: Vec<Token>,
    /// Span of the `${ ... }` hole including delimiters.
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Tok {
    Ident(Rc<str>),
    Kw(Kw),
    /// Number literal; `None` when the exponent is beyond host range.
    Num(Option<Rc<Decimal>>),
    Str(Rc<str>),
    Query(Rc<QueryTok>),
    P(P),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
}

fn lexical(msg: impl Into<String>, start: usize, end: usize) -> Diagnostic {
    Diagnostic::new("lex", "lexical", msg, Span::new(start, end))
}

/// Validate source bytes: strict UTF-8, no BOM. Returns the text.
pub fn decode_source(bytes: &[u8]) -> Result<&str, Diagnostic> {
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            if s.starts_with('\u{FEFF}') {
                Err(lexical("byte-order mark is not permitted", 0, 3))
            } else {
                Ok(s)
            }
        }
        Err(e) => {
            let at = e.valid_up_to();
            Err(lexical("invalid UTF-8 in source", at, at + 1))
        }
    }
}

/// Tokenize a complete program. The final token is `Eof`.
pub fn tokenize(src: &str) -> Result<Vec<Token>, Diagnostic> {
    if src.starts_with('\u{FEFF}') {
        return Err(lexical("byte-order mark is not permitted", 0, 3));
    }
    let mut lx = Lexer { src, b: src.as_bytes(), i: 0 };
    let mut toks = lx.tokens(false)?;
    toks.push(Token { tok: Tok::Eof, span: Span::new(src.len(), src.len()) });
    Ok(toks)
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<u8> {
        self.b.get(self.i + k).copied()
    }

    /// Lex tokens until EOF, or (in a hole) until the matching `}`.
    fn tokens(&mut self, in_hole: bool) -> Result<Vec<Token>, Diagnostic> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        loop {
            self.skip_ws()?;
            let start = self.i;
            let c = match self.peek() {
                None => {
                    if in_hole {
                        return Err(lexical("unterminated query hole", start, start));
                    }
                    return Ok(out);
                }
                Some(c) => c,
            };
            if in_hole && c == b'}' && depth == 0 {
                self.i += 1;
                return Ok(out);
            }
            let tok = if is_ident_start(c) {
                self.word()?
            } else if c.is_ascii_digit() {
                self.number()?
            } else if c == b'"' {
                Tok::Str(Rc::from(self.string()?.as_str()))
            } else {
                let p = self.punct()?;
                if in_hole {
                    match p {
                        P::LBrace => depth += 1,
                        P::RBrace => depth -= 1,
                        _ => {}
                    }
                }
                Tok::P(p)
            };
            out.push(Token { tok, span: Span::new(start, self.i) });
        }
    }

    fn skip_ws(&mut self) -> Result<(), Diagnostic> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.i += 1,
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        self.i += 1;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn word(&mut self) -> Result<Tok, Diagnostic> {
        let start = self.i;
        while matches!(self.peek(), Some(c) if is_ident_char(c)) {
            self.i += 1;
        }
        let w = &self.src[start..self.i];
        if w == "q" && self.peek() == Some(b'`') {
            self.i += 1;
            return self.query(start);
        }
        Ok(match Kw::from_word(w) {
            Some(k) => Tok::Kw(k),
            None => Tok::Ident(Rc::from(w)),
        })
    }

    fn number(&mut self) -> Result<Tok, Diagnostic> {
        let start = self.i;
        let bad = |lx: &Lexer, msg: &str| lexical(msg.to_string(), start, lx.i.max(start + 1));
        if self.peek() == Some(b'0') {
            self.i += 1;
            if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
                return Err(bad(self, "leading zero in number"));
            }
        } else {
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            let fs = self.i;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
            if fs == self.i {
                return Err(bad(self, "decimal point must be followed by a digit"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let es = self.i;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
            if es == self.i {
                return Err(bad(self, "exponent requires digits"));
            }
        }
        match self.peek() {
            Some(c) if is_ident_char(c) => {
                self.i += 1;
                return Err(bad(self, "number immediately followed by identifier character"));
            }
            Some(b'.') => {
                self.i += 1;
                return Err(bad(self, "extra decimal point in number"));
            }
            _ => {}
        }
        let text = &self.src[start..self.i];
        Ok(match Decimal::parse_json(text) {
            Ok(Some(d)) => Tok::Num(Some(Rc::new(d))),
            Ok(None) => return Err(bad(self, "malformed number")),
            Err(_) => Tok::Num(None),
        })
    }

    fn hex4(&mut self) -> Result<u32, Diagnostic> {
        let start = self.i;
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.peek().ok_or_else(|| lexical("incomplete \\u escape", start, self.i))?;
            let d = (c as char)
                .to_digit(16)
                .ok_or_else(|| lexical("invalid \\u escape", start, self.i + 1))?;
            v = v * 16 + d;
            self.i += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, Diagnostic> {
        let start = self.i;
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = match self.peek() {
                None => return Err(lexical("unterminated string", start, self.i)),
                Some(c) => c,
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    let esc = self.i;
                    self.i += 1;
                    let e = self.peek().ok_or_else(|| lexical("unterminated string", start, self.i))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let u = self.hex4()?;
                            if (0xD800..0xDC00).contains(&u) {
                                if self.peek() == Some(b'\\') && self.peek_at(1) == Some(b'u') {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let cp = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cp).unwrap());
                                    } else {
                                        return Err(lexical("unpaired surrogate escape", esc, self.i));
                                    }
                                } else {
                                    return Err(lexical("unpaired surrogate escape", esc, self.i));
                                }
                            } else if (0xDC00..0xE000).contains(&u) {
                                return Err(lexical("unpaired surrogate escape", esc, self.i));
                            } else {
                                out.push(char::from_u32(u).unwrap());
                            }
                        }
                        _ => return Err(lexical("unknown string escape", esc, self.i)),
                    }
                }
                c if c < 0x20 => {
                    return Err(lexical("raw control character or line ending in string", self.i, self.i + 1))
                }
                _ => {
                    let ch = self.src[self.i..].chars().next().unwrap();
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    fn query(&mut self, start: usize) -> Result<Tok, Diagnostic> {
        let mut segments = Vec::new();
        let mut holes = Vec::new();
        let mut cur = String::new();
        loop {
            let c = match self.peek() {
                None => return Err(lexical("unterminated query literal", start, self.i)),
                Some(c) => c,
            };
            match c {
                b'`' => {
                    self.i += 1;
                    segments.push(cur);
                    return Ok(Tok::Query(Rc::new(QueryTok { segments, holes })));
                }
                b'\\' => {
                    let esc = self.i;
                    match self.peek_at(1) {
                        Some(b'\\') => {
                            cur.push('\\');
                            self.i += 2;
                        }
                        Some(b'`') => {
                            cur.push('`');
                            self.i += 2;
                        }
                        Some(b'$') if self.peek_at(2) == Some(b'{') => {
                            cur.push_str("${");
                            self.i += 3;
                        }
                        _ => return Err(lexical("invalid escape in query text", esc, esc + 2)),
                    }
                }
                b'$' if self.peek_at(1) == Some(b'{') => {
                    let hs = self.i;
                    self.i += 2;
                    let tokens = self.tokens(true)?;
                    holes.push(Hole { tokens, span: Span::new(hs, self.i) });
                    segments.push(std::mem::take(&mut cur));
                }
                _ => {
                    let ch = self.src[self.i..].chars().next().unwrap();
                    cur.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    fn punct(&mut self) -> Result<P, Diagnostic> {
        let start = self.i;
        let c = self.peek().unwrap();
        let n = self.peek_at(1);
        let (p, len) = match (c, n) {
            (b'=', Some(b'>')) => (P::Arrow, 2),
            (b'=', Some(b'=')) => (P::EqEq, 2),
            (b'!', Some(b'=')) => (P::NotEq, 2),
            (b'<', Some(b'=')) => (P::Le, 2),
            (b'>', Some(b'=')) => (P::Ge, 2),
            (b'&', Some(b'&')) => (P::AndAnd, 2),
            (b'|', Some(b'|')) => (P::OrOr, 2),
            (b'?', Some(b'?')) => (P::Coalesce, 2),
            (b'?', Some(b'.')) => (P::QDot, 2),
            (b'?', Some(b'[')) => (P::QBracket, 2),
            (b'(', _) => (P::LParen, 1),
            (b')', _) => (P::RParen, 1),
            (b'[', _) => (P::LBracket, 1),
            (b']', _) => (P::RBracket, 1),
            (b'{', _) => (P::LBrace, 1),
            (b'}', _) => (P::RBrace, 1),
            (b',', _) => (P::Comma, 1),
            (b';', _) => (P::Semi, 1),
            (b':', _) => (P::Colon, 1),
            (b'.', Some(d)) if d.is_ascii_digit() => {
                return Err(lexical("number cannot start with a decimal point", start, start + 2))
            }
            (b'.', _) => (P::Dot, 1),
            (b'+', _) => (P::Plus, 1),
            (b'-', _) => (P::Minus, 1),
            (b'*', _) => (P::Star, 1),
            (b'/', _) => (P::Slash, 1),
            (b'%', _) => (P::Percent, 1),
            (b'!', _) => (P::Bang, 1),
            (b'<', _) => (P::Lt, 1),
            (b'>', _) => (P::Gt, 1),
            (b'=', _) => (P::Assign, 1),
            (b'|', _) => (P::Pipe, 1),
            _ => {
                let ch = self.src[self.i..].chars().next().unwrap();
                return Err(lexical(format!("unexpected character {ch:?}"), start, start + ch.len_utf8()));
            }
        };
        self.i += len;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_fails(s: &str) {
        assert!(tokenize(s).is_err(), "expected lexical error for {s:?}");
    }

    #[test]
    fn numbers() {
        lex_fails("01");
        lex_fails(".5");
        lex_fails("1.");
        lex_fails("1abc");
        lex_fails("1.2.3");
        assert_eq!(tokenize("1-2").unwrap().len(), 4);
        assert_eq!(tokenize("1e-2").unwrap().len(), 2);
    }

    #[test]
    fn strings_and_queries() {
        lex_fails("\"\\x41\"");
        lex_fails("\"\\uD800\"");
        lex_fails("\"a\nb\"");
        lex_fails("\u{FEFF}1");
        let t = tokenize("q`select ${do{x=2;{n:x}}}, ${q`nested`} \\${literal}`").unwrap();
        match &t[0].tok {
            Tok::Query(q) => {
                assert_eq!(q.segments, vec!["select ", ", ", " ${literal}"]);
                assert_eq!(q.holes.len(), 2);
            }
            _ => panic!(),
        }
        lex_fails("q `x`");
    }
}
