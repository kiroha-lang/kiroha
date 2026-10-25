//! Source spans and frontend diagnostics.

use crate::value::{err, Value};

/// Byte span with exclusive end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    pub fn join(self, o: Span) -> Span {
        Span { start: self.start.min(o.start), end: self.end.max(o.end) }
    }
}

/// A lex/parse/check diagnostic.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub phase: &'static str,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub data: Vec<(&'static str, Value)>,
}

impl Diagnostic {
    pub fn new(phase: &'static str, code: &'static str, message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic { phase, code, message: message.into(), span, data: Vec::new() }
    }

    pub fn with(mut self, key: &'static str, v: Value) -> Diagnostic {
        self.data.push((key, v));
        self
    }

    pub fn error_value(&self) -> Value {
        err::make(self.code, self.message.clone(), self.data.clone())
    }
}

/// Line/column lookup. Lines are one-based; columns count scalar values.
pub struct LineIndex<'a> {
    src: &'a str,
}

impl<'a> LineIndex<'a> {
    pub fn new(src: &'a str) -> LineIndex<'a> {
        LineIndex { src }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let b = self.src.as_bytes();
        let off = offset.min(b.len());
        let mut line = 1;
        let mut line_start = 0;
        let mut i = 0;
        while i < off {
            match b[i] {
                b'\r' => {
                    if i + 1 < b.len() && b[i + 1] == b'\n' {
                        if i + 1 >= off {
                            break;
                        }
                        i += 1;
                    }
                    line += 1;
                    line_start = i + 1;
                }
                b'\n' => {
                    line += 1;
                    line_start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        let mut s = line_start.min(off);
        while !self.src.is_char_boundary(s) {
            s += 1;
        }
        let mut e = off;
        while !self.src.is_char_boundary(e) {
            e -= 1;
        }
        let col = if e >= s { self.src[s..e].chars().count() + 1 } else { 1 };
        (line, col)
    }
}
