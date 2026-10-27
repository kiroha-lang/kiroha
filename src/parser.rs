//! Recursive-descent parser for the normative grammar (spec §2.2).

use crate::ast::{BinOp, Body, FnInfo, Node, NodeId, Program, UnOp, Var};
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Kw, Tok, Token, P};
use crate::value::Value;
use crate::types::{TypeExpr, TypeExprKind, TypeRef, TypeParam};
use std::collections::HashSet;
use std::borrow::Cow;
use std::rc::Rc;

struct Parser<'a> {
    toks: Cow<'a, [Token]>,
    pos: usize,
    prog: &'a mut Program,
}

type PResult<T> = Result<T, Diagnostic>;
type Parameters = (Vec<Rc<str>>, Vec<Option<TypeExpr>>);

fn syntax(msg: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("parse", "syntax", msg, span)
}

/// Parse a token stream (ending in `Eof`) into a program.
pub fn parse(toks: &[Token]) -> PResult<Program> {
    let mut prog = Program::default();
    let items = {
        let mut p = Parser { toks: Cow::Borrowed(toks), pos: 0, prog: &mut prog };
        let items = p.sequence(|t| matches!(t, Tok::Eof))?;
        p.expect_eof()?;
        items
    };
    prog.items = items;
    Ok(prog)
}

fn tok_desc(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("identifier `{s}`"),
        Tok::Kw(k) => format!("`{}`", k.text()),
        Tok::Num(_) => "number".into(),
        Tok::Str(_) => "string".into(),
        Tok::Query(_) => "query literal".into(),
        Tok::SqlBlock(_) => "SQL block".into(),
        Tok::P(p) => format!("`{p:?}`"),
        Tok::Eof => "end of input".into(),
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, k: usize) -> &Tok {
        let i = (self.pos + k).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, p: P) -> bool {
        matches!(self.peek(), Tok::P(q) if *q == p)
    }

    fn at_kw(&self, k: Kw) -> bool {
        matches!(self.peek(), Tok::Kw(q) if *q == k)
    }

    fn unexpected(&self, wanted: &str) -> Diagnostic {
        syntax(format!("expected {wanted}, found {}", tok_desc(self.peek())), self.span())
    }

    fn expect(&mut self, p: P, wanted: &str) -> PResult<Span> {
        if self.at(p) {
            Ok(self.advance().span)
        } else {
            Err(self.unexpected(wanted))
        }
    }

    fn expect_kw(&mut self, k: Kw) -> PResult<()> {
        if self.at_kw(k) {
            self.advance();
            Ok(())
        } else {
            Err(self.unexpected(&format!("`{}`", k.text())))
        }
    }

    fn expect_eof(&mut self) -> PResult<()> {
        if matches!(self.peek(), Tok::Eof) {
            Ok(())
        } else {
            Err(self.unexpected("`;` or end of input"))
        }
    }

    fn ident(&mut self, wanted: &str) -> PResult<(Rc<str>, Span)> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                let sp = self.advance().span;
                Ok((s, sp))
            }
            _ => Err(self.unexpected(wanted)),
        }
    }

    /// WORD: identifier-shaped token including reserved words.
    fn word(&mut self) -> PResult<(Rc<str>, Span)> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                let sp = self.advance().span;
                Ok((s, sp))
            }
            Tok::Kw(k) => {
                let sp = self.advance().span;
                Ok((Rc::from(k.text()), sp))
            }
            _ => Err(self.unexpected("field name")),
        }
    }

    fn add(&mut self, n: Node, span: Span) -> NodeId {
        self.prog.add(n, span)
    }

    fn sequence(&mut self, is_end: fn(&Tok) -> bool) -> PResult<Vec<NodeId>> {
        let mut items = Vec::new();
        if is_end(self.peek()) {
            return Ok(items);
        }
        loop {
            let item = self.item()?;
            let definition = matches!(self.prog.node(item), Node::Define(_));
            items.push(item);
            if self.at(P::Semi) {
                self.advance();
                if is_end(self.peek()) {
                    break;
                }
                if self.at(P::Semi) {
                    return Err(syntax("repeated `;` in sequence", self.span()));
                }
            } else if is_end(self.peek()) {
                break;
            } else if definition {
                // The matching named end already delimits a definition.
                continue;
            } else {
                return Err(self.unexpected("`;`"));
            }
        }
        Ok(items)
    }

    fn item(&mut self) -> PResult<NodeId> {
        if self.at_kw(Kw::Fun) {
            return self.definition();
        }
        if self.at_kw(Kw::Type) {
            let start = self.advance().span;
            let (name, _) = self.ident("type name")?;
            let params = self.type_params()?;
            self.expect(P::Assign, "`=`")?;
            let body = self.type_expr()?;
            let span = start.join(body.span);
            return Ok(self.add(Node::TypeDecl { name, params, body, alias: 0 }, span));
        }
        if let Tok::Ident(_) = self.peek() {
            if matches!(self.peek_at(1), Tok::P(P::Assign | P::Colon)) {
                let (name, sp) = self.ident("identifier")?;
                let annotation = self.annotation()?;
                self.expect(P::Assign, "`=`")?;
                let e = self.expr()?;
                let span = sp.join(self.prog.span(e));
                return Ok(self.add(Node::Bind { name, slot: 0, e, annotation }, span));
            }
        }
        self.expr()
    }

    fn definition(&mut self) -> PResult<NodeId> {
        let sp = self.advance().span; // fun
        let (name, _) = self.ident("function name")?;
        let type_params = self.type_params()?;
        self.expect(P::LParen, "`(`")?;
        let (params, param_types) = self.parameters()?;
        self.expect(P::RParen, "`)`")?;
        let mut slot_name = None;
        let mut slot_type = None;
        if self.at(P::LBrace) {
            let saved = self.pos;
            let slot = (|| -> PResult<_> {
                self.advance();
                let name = self.ident("callback slot name")?.0;
                let annotation = self.annotation()?;
                self.expect(P::RBrace, "`}`")?;
                let unambiguous = annotation.as_ref().is_none_or(|a| matches!(a.kind, TypeExprKind::Function { .. })) || self.at(P::Colon);
                if !unambiguous { return Err(self.unexpected("callback function type")); }
                Ok((name, annotation))
            })();
            match slot { Ok((name, annotation)) => { slot_name = Some(name); slot_type = annotation; }, Err(_) => self.pos = saved }
        }
        let result_type = self.annotation()?;
        let items = self.sequence(|t| matches!(t, Tok::Kw(Kw::End) | Tok::Eof))?;
        if !self.at_kw(Kw::End) {
            return Err(self.unexpected(&format!("`end fun {name}`")));
        }
        self.advance();
        self.expect_kw(Kw::Fun)?;
        let (closing_name, closing_span) = self.ident("closing function name")?;
        if closing_name != name {
            return Err(syntax(format!("expected `end fun {name}`, found `end fun {closing_name}`"), closing_span));
        }
        let span = sp.join(closing_span);
        let info = FnInfo {
            name: Some(name),
            params,
            param_types, type_params, result_type, slot_type,
            slot_name,
            body: Body::Seq(items),
            nlocals: 0,
            captures: Vec::new(),
            def_slot: 0,
        };
        Ok(self.add(Node::Define(info), span))
    }

    fn annotation(&mut self) -> PResult<Option<TypeExpr>> {
        if !self.at(P::Colon) { return Ok(None); }
        self.advance();
        Ok(Some(self.type_expr()?))
    }
    fn parameters(&mut self) -> PResult<Parameters> {
        let mut names = Vec::new();
        let mut annotations = Vec::new();
        if !self.at(P::RParen) {
            loop {
                names.push(self.ident("parameter")?.0);
                annotations.push(self.annotation()?);
                if !self.at(P::Comma) { break; }
                self.advance();
            }
        }
        Ok((names, annotations))
    }
    fn type_gt(&mut self) -> PResult<()> {
        if self.at(P::Ge) {
            // In a type argument/parameter list, >= is the closing > then =.
            // Preserve normal comparison tokenization everywhere else.
            let span = self.span();
            self.toks.to_mut()[self.pos] = Token { tok: Tok::P(P::Gt), span: Span::new(span.start, span.start + 1) };
            self.toks.to_mut().insert(self.pos + 1, Token { tok: Tok::P(P::Assign), span: Span::new(span.start + 1, span.end) });
        }
        self.expect(P::Gt, "`>`")?;
        Ok(())
    }
    fn type_params(&mut self) -> PResult<Vec<TypeParam>> {
        let mut params = Vec::new();
        if self.at(P::Lt) {
            self.advance();
            loop {
                params.push(TypeParam { name: self.ident("type parameter")?.0, id: 0 });
                if !self.at(P::Comma) { break; }
                self.advance();
            }
            self.type_gt()?;
        }
        Ok(params)
    }
    fn type_args(&mut self) -> PResult<Vec<TypeExpr>> {
        self.expect(P::Lt, "`<`")?;
        let mut args = vec![self.type_expr()?];
        while self.at(P::Comma) { self.advance(); args.push(self.type_expr()?); }
        self.type_gt()?;
        Ok(args)
    }
    fn type_expr(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        let kind = match self.peek().clone() {
            Tok::Ident(name) => {
                self.advance();
                if matches!(&*name, "maybe" | "optional" | "open") && self.at(P::LParen) {
                    self.advance();
                    let inner = Box::new(self.type_expr()?);
                    self.expect(P::RParen, "`)`")?;
                    TypeExprKind::Wrap { name, inner }
                } else {
                    let args = if self.at(P::Lt) { self.type_args()? } else { Vec::new() };
                    TypeExprKind::Name { name, args, target: TypeRef::Unresolved }
                }
            }
            Tok::Kw(Kw::Null) => {
                self.advance();
                TypeExprKind::Name { name: Rc::from("null"), args: Vec::new(), target: TypeRef::Unresolved }
            }
            Tok::P(P::LBracket) => {
                self.advance(); let inner = Box::new(self.type_expr()?);
                self.expect(P::RBracket, "`]`")?; TypeExprKind::Array(inner)
            }
            Tok::P(P::LBrace) => {
                self.advance(); let mut fields = Vec::new(); let mut seen = HashSet::new();
                if !self.at(P::RBrace) {
                    loop {
                        let span = self.span();
                        let name = if let Tok::Str(s) = self.peek().clone() { self.advance(); s } else { self.word()?.0 };
                        if !seen.insert(name.clone()) { return Err(Diagnostic::new("parse", "duplicate_key", "duplicate type field", span)); }
                        self.expect(P::Colon, "`:`")?;
                        fields.push((name, self.type_expr()?));
                        if !self.at(P::Comma) { break; } self.advance();
                    }
                }
                self.expect(P::RBrace, "`}`")?; TypeExprKind::Record(fields)
            }
            Tok::P(P::LParen) => {
                self.advance(); let mut params = Vec::new();
                if !self.at(P::RParen) {
                    params.push(self.type_expr()?);
                    while self.at(P::Comma) { self.advance(); params.push(self.type_expr()?); }
                }
                self.expect(P::RParen, "`)`")?;
                let callback = if self.at(P::LBrace) {
                    self.advance(); let cb = Box::new(self.type_expr()?); self.expect(P::RBrace, "`}`")?; Some(cb)
                } else { None };
                if self.at(P::Arrow) {
                    self.advance();
                    TypeExprKind::Function { params, callback, result: Box::new(self.type_expr()?) }
                } else if params.len() == 1 && callback.is_none() {
                    return Ok(TypeExpr { kind: params.pop().unwrap().kind, span: start.join(self.prev_span()) });
                } else { return Err(self.unexpected("`=>` in function type")); }
            }
            _ => return Err(self.unexpected("type expression")),
        };
        Ok(TypeExpr { kind, span: start.join(self.prev_span()) })
    }

    fn expr(&mut self) -> PResult<NodeId> {
        if self.at_kw(Kw::If) {
            let sp = self.advance().span;
            let c = self.expr()?;
            self.expect_kw(Kw::Then)?;
            let t = self.expr()?;
            self.expect_kw(Kw::Else)?;
            let e = self.expr()?;
            let span = sp.join(self.prog.span(e));
            return Ok(self.add(Node::If { c, t, e }, span));
        }
        if self.at_kw(Kw::Try) {
            let sp = self.advance().span;
            let e = self.expr()?;
            self.expect_kw(Kw::Catch)?;
            self.expect(P::LBrace, "`{` starting catch handler")?;
            let (name, _) = self.ident("handler parameter")?;
            self.expect(P::Arrow, "`=>`")?;
            let items = self.sequence(|t| matches!(t, Tok::P(P::RBrace)))?;
            let end = self.expect(P::RBrace, "`}`")?;
            return Ok(self.add(Node::Try { e, name, slot: 0, items }, sp.join(end)));
        }
        self.pipe()
    }

    fn check_args(&self, args: &[NodeId]) -> PResult<()> {
        for &a in args {
            let root = self.prog.strip_groups(a);
            if matches!(self.prog.node(root), Node::Lambda(_)) {
                return Err(syntax(
                    "a lambda literal cannot be an ordinary argument; use a trailing callback",
                    self.prog.span(a),
                ));
            }
        }
        Ok(())
    }

    fn arguments(&mut self) -> PResult<Vec<NodeId>> {
        self.expect(P::LParen, "`(`")?;
        let mut args = Vec::new();
        if !self.at(P::RParen) {
            args.push(self.expr()?);
            while self.at(P::Comma) {
                self.advance();
                args.push(self.expr()?);
            }
        }
        self.expect(P::RParen, "`)`")?;
        self.check_args(&args)?;
        Ok(args)
    }

    fn trailing_callback(&mut self) -> PResult<Option<NodeId>> {
        if !self.at(P::LBrace) {
            return Ok(None);
        }
        if !self.is_lambda_head() {
            return Err(syntax("a trailing callback must be a lambda literal", self.span()));
        }
        Ok(Some(self.lambda()?))
    }

    fn pipe(&mut self) -> PResult<NodeId> {
        let mut left = self.coalesce()?;
        while self.at(P::Pipe) {
            self.advance();
            let (name, nsp) = self.ident("pipe stage function path")?;
            let mut head = self.add(Node::Name { name, var: Var::Unresolved }, nsp);
            while self.at(P::Dot) {
                self.advance();
                let (name, end) = self.word()?;
                let span = self.prog.span(head).join(end);
                head = self.add(Node::Member { recv: head, name, optional: false }, span);
            }
            if self.at(P::Lt) {
                let args = self.type_args()?; let span = self.prog.span(head).join(self.prev_span());
                head = self.add(Node::Instantiate { callee: head, args, schema: None }, span);
            }
            if !self.at(P::LParen) {
                return Err(self.unexpected("`(` after pipe stage path"));
            }
            let args = self.arguments()?;
            let cb = self.trailing_callback()?;
            let span = self.prog.span(left).join(self.prev_span());
            left = self.add(Node::Pipe { left, head, fields: Vec::new(), args, cb }, span);
        }
        Ok(left)
    }

    fn coalesce(&mut self) -> PResult<NodeId> {
        let l = self.or()?;
        if self.at(P::Coalesce) {
            self.advance();
            let r = self.coalesce()?;
            let span = self.prog.span(l).join(self.prog.span(r));
            return Ok(self.add(Node::Binary { op: BinOp::Coalesce, l, r }, span));
        }
        Ok(l)
    }

    fn or(&mut self) -> PResult<NodeId> {
        let mut l = self.and()?;
        while self.at(P::OrOr) {
            self.advance();
            let r = self.and()?;
            let span = self.prog.span(l).join(self.prog.span(r));
            l = self.add(Node::Binary { op: BinOp::Or, l, r }, span);
        }
        Ok(l)
    }

    fn and(&mut self) -> PResult<NodeId> {
        let mut l = self.equality()?;
        while self.at(P::AndAnd) {
            self.advance();
            let r = self.equality()?;
            let span = self.prog.span(l).join(self.prog.span(r));
            l = self.add(Node::Binary { op: BinOp::And, l, r }, span);
        }
        Ok(l)
    }

    fn equality(&mut self) -> PResult<NodeId> {
        let (l, lc) = self.ordering()?;
        let op = match self.peek() {
            Tok::P(P::EqEq) => BinOp::Eq,
            Tok::P(P::NotEq) => BinOp::Ne,
            _ => return Ok(l),
        };
        let opsp = self.advance().span;
        let (r, rc) = self.ordering()?;
        if lc || rc {
            return Err(syntax("at most one comparison operator per unparenthesized expression", opsp));
        }
        let span = self.prog.span(l).join(self.prog.span(r));
        Ok(self.add(Node::Binary { op, l, r }, span))
    }

    fn ordering(&mut self) -> PResult<(NodeId, bool)> {
        let l = self.additive()?;
        let op = match self.peek() {
            Tok::P(P::Lt) => BinOp::Lt,
            Tok::P(P::Le) => BinOp::Le,
            Tok::P(P::Gt) => BinOp::Gt,
            Tok::P(P::Ge) => BinOp::Ge,
            _ => return Ok((l, false)),
        };
        self.advance();
        let r = self.additive()?;
        if matches!(self.peek(), Tok::P(P::Lt | P::Le | P::Gt | P::Ge)) {
            return Err(syntax("comparison operators do not chain", self.span()));
        }
        let span = self.prog.span(l).join(self.prog.span(r));
        Ok((self.add(Node::Binary { op, l, r }, span), true))
    }

    fn additive(&mut self) -> PResult<NodeId> {
        let mut l = self.product()?;
        loop {
            let op = match self.peek() {
                Tok::P(P::Plus) => BinOp::Add,
                Tok::P(P::Minus) => BinOp::Sub,
                _ => return Ok(l),
            };
            self.advance();
            let r = self.product()?;
            let span = self.prog.span(l).join(self.prog.span(r));
            l = self.add(Node::Binary { op, l, r }, span);
        }
    }

    fn product(&mut self) -> PResult<NodeId> {
        let mut l = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::P(P::Star) => BinOp::Mul,
                Tok::P(P::Slash) => BinOp::Div,
                Tok::P(P::Percent) => BinOp::Rem,
                _ => return Ok(l),
            };
            self.advance();
            let r = self.unary()?;
            let span = self.prog.span(l).join(self.prog.span(r));
            l = self.add(Node::Binary { op, l, r }, span);
        }
    }

    fn unary(&mut self) -> PResult<NodeId> {
        let op = match self.peek() {
            Tok::P(P::Bang) => UnOp::Not,
            Tok::P(P::Minus) => UnOp::Neg,
            _ => return self.postfix(),
        };
        let sp = self.advance().span;
        let e = self.unary()?;
        let span = sp.join(self.prog.span(e));
        Ok(self.add(Node::Unary { op, e }, span))
    }

    fn postfix(&mut self) -> PResult<NodeId> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::P(P::LBracket) if matches!(self.prog.node(e), Node::Name { name, .. } if &**name == "sql") && self.typed_sql_ahead() => {
                    self.advance();
                    let annotation = self.type_expr()?;
                    self.expect(P::RBracket, "`]` after SQL row type")?;
                    let args = self.arguments()?;
                    e = self.sql_block_call(e, args, Some(annotation))?;
                }
                Tok::P(P::Lt) => {
                    let saved = self.pos;
                    match self.type_args() {
                        Ok(args) => { let span = self.prog.span(e).join(self.prev_span()); e = self.add(Node::Instantiate { callee: e, args, schema: None }, span); }
                        Err(_) => { self.pos = saved; return Ok(e); }
                    }
                }
                Tok::P(P::LParen) => {
                    let args = self.arguments()?;
                    if matches!(self.peek(), Tok::SqlBlock(_)) && matches!(self.prog.node(e), Node::Name { name, .. } if &**name == "sql") {
                        e = self.sql_block_call(e, args, None)?;
                        continue;
                    }
                    let cb = self.trailing_callback()?;
                    let span = self.prog.span(e).join(self.prev_span());
                    e = self.add(Node::Call { callee: e, args, cb }, span);
                }
                Tok::P(P::Dot) | Tok::P(P::QDot) => {
                    let optional = self.at(P::QDot);
                    self.advance();
                    let (w, wsp) = self.word()?;
                    if !optional && &*w == "as" && self.at(P::LParen) {
                        let args = self.arguments()?;
                        if args.len() != 1 {
                            return Err(syntax("as requires one schema descriptor", self.prev_span()));
                        }
                        let span = self.prog.span(e).join(self.prev_span());
                        e = self.validate_with_schema(e, args[0], span);
                        continue;
                    }
                    if !optional && &*w == "as" && self.at(P::LBracket) {
                        let saved = self.pos;
                        self.advance();
                        if let Ok(annotation) = self.type_expr() {
                            if self.at(P::RBracket) {
                                let end = self.advance().span;
                                let span = self.prog.span(e).join(end);
                                let schema = self.add(Node::TypeSchema { annotation, schema: None }, span);
                                e = self.validate_with_schema(e, schema, span);
                                continue;
                            }
                        }
                        self.pos = saved;
                    }
                    let span = self.prog.span(e).join(wsp);
                    e = self.add(Node::Member { recv: e, name: w, optional }, span);
                    if self.at(P::LBrace) {
                        let cb = self.trailing_callback()?;
                        let span = self.prog.span(e).join(self.prev_span());
                        e = self.add(Node::Call { callee: e, args: Vec::new(), cb }, span);
                    }
                }
                Tok::P(P::LBracket) | Tok::P(P::QBracket) => {
                    let optional = self.at(P::QBracket);
                    self.advance();
                    let key = self.expr()?;
                    let end = self.expect(P::RBracket, "`]`")?;
                    let span = self.prog.span(e).join(end);
                    e = self.add(Node::Get { recv: e, key, optional }, span);
                }
                _ => return Ok(e),
            }
        }
    }

    fn typed_sql_ahead(&self) -> bool {
        fn after_pair(toks: &[Token], start: usize, open: P, close: P) -> Option<usize> {
            let mut depth = 0usize;
            for (i, token) in toks.iter().enumerate().skip(start) {
                match token.tok {
                    Tok::P(p) if p == open => depth += 1,
                    Tok::P(p) if p == close => { depth -= 1; if depth == 0 { return Some(i + 1); } }
                    Tok::Eof => return None,
                    _ => {}
                }
            }
            None
        }
        let Some(after_type) = after_pair(&self.toks, self.pos, P::LBracket, P::RBracket) else { return false };
        if !matches!(self.toks.get(after_type).map(|t| &t.tok), Some(Tok::P(P::LParen))) { return false; }
        let Some(after_args) = after_pair(&self.toks, after_type, P::LParen, P::RParen) else { return false };
        matches!(self.toks.get(after_args).map(|t| &t.tok), Some(Tok::SqlBlock(_)))
    }

    fn sql_block_call(&mut self, callee: NodeId, mut args: Vec<NodeId>, annotation: Option<TypeExpr>) -> PResult<NodeId> {
        if !(1..=2).contains(&args.len()) {
            return Err(syntax("SQL block requires a database and optional options", self.span()));
        }
        let span = self.prog.span(callee).join(self.span());
        let q = match self.advance().tok { Tok::SqlBlock(q) => q, _ => unreachable!() };
        let query = self.query_node(q, span)?;
        args.insert(1, query);
        let call = self.add(Node::Call { callee, args, cb: None }, span);
        let Some(annotation) = annotation else { return Ok(call) };
        let rows = self.add(Node::Member { recv: call, name: Rc::from("rows"), optional: false }, span);
        let schema = self.add(Node::TypeSchema { annotation, schema: None }, span);
        let schema_array = self.add(Node::Array(vec![schema]), span);
        Ok(self.validate_with_schema(rows, schema_array, span))
    }

    fn validate_with_schema(&mut self, value: NodeId, schema: NodeId, span: Span) -> NodeId {
        let validate = self.add(Node::Intrinsic(crate::builtins::Builtin::Validate), span);
        self.add(Node::Call { callee: validate, args: vec![value, schema], cb: None }, span)
    }

    fn query_node(&mut self, q: Rc<crate::lexer::QueryTok>, sp: Span) -> PResult<NodeId> {
        let mut holes = Vec::new();
        for h in &q.holes {
            let mut toks = h.tokens.clone();
            toks.push(Token { tok: Tok::Eof, span: Span::new(h.span.end - 1, h.span.end - 1) });
            if toks.len() == 1 { return Err(syntax("empty query hole", h.span)); }
            let mut sub = Parser { toks: Cow::Borrowed(&toks), pos: 0, prog: self.prog };
            let e = sub.expr()?;
            if !matches!(sub.peek(), Tok::Eof) { return Err(sub.unexpected("`}` closing query hole")); }
            holes.push(e);
        }
        Ok(self.add(Node::Query { segments: Rc::new(q.segments.clone()), holes }, sp))
    }

    fn is_lambda_head(&mut self) -> bool {
        let saved = self.pos;
        self.advance(); // {
        let result = self.lambda_parameters().is_ok() && self.at(P::Arrow);
        self.pos = saved;
        result
    }

    fn lambda_parameters(&mut self) -> PResult<Parameters> {
        if self.at(P::LParen) {
            self.advance();
            let result = self.parameters()?;
            if result.0.len() == 1 { return Err(self.unexpected("at least two parenthesized lambda parameters")); }
            self.expect(P::RParen, "`)`")?;
            Ok(result)
        } else {
            let name = self.ident("parameter")?.0;
            let annotation = self.annotation()?;
            Ok((vec![name], vec![annotation]))
        }
    }

    fn lambda(&mut self) -> PResult<NodeId> {
        let sp = self.expect(P::LBrace, "`{`")?;
        let (params, param_types) = self.lambda_parameters()?;
        self.expect(P::Arrow, "`=>`")?;
        let items = self.sequence(|t| matches!(t, Tok::P(P::RBrace)))?;
        let end = self.expect(P::RBrace, "`}`")?;
        let info = FnInfo {
            name: None, params, param_types, type_params: Vec::new(), result_type: None, slot_type: None,
            slot_name: None, body: Body::Seq(items), nlocals: 0, captures: Vec::new(), def_slot: 0,
        };
        Ok(self.add(Node::Lambda(info), sp.join(end)))
    }

    fn record(&mut self) -> PResult<NodeId> {
        let sp = self.expect(P::LBrace, "`{`")?;
        let mut fields: Vec<(Rc<str>, NodeId)> = Vec::new();
        let mut seen: HashSet<Rc<str>> = HashSet::new();
        if !self.at(P::RBrace) {
            loop {
                let key_span = self.span();
                let key: Rc<str>;
                let value: NodeId;
                match self.peek().clone() {
                    Tok::Str(s) => {
                        self.advance();
                        self.expect(P::Colon, "`:`")?;
                        key = s;
                        value = self.expr()?;
                    }
                    Tok::Ident(s) if !matches!(self.peek_at(1), Tok::P(P::Colon)) => {
                        // shorthand field
                        let nsp = self.advance().span;
                        key = s.clone();
                        value = self.add(Node::Name { name: s, var: Var::Unresolved }, nsp);
                    }
                    Tok::Ident(_) | Tok::Kw(_) => {
                        let (w, _) = self.word()?;
                        self.expect(P::Colon, "`:`")?;
                        key = w;
                        value = self.expr()?;
                    }
                    _ => return Err(self.unexpected("record field")),
                }
                if !seen.insert(key.clone()) {
                    return Err(Diagnostic::new(
                        "parse",
                        "duplicate_key",
                        format!("duplicate record key {key:?}"),
                        key_span,
                    ));
                }
                fields.push((key, value));
                if self.at(P::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        let end = self.expect(P::RBrace, "`,` or `}`")?;
        Ok(self.add(Node::Record(fields), sp.join(end)))
    }

    fn primary(&mut self) -> PResult<NodeId> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Kw(Kw::Null) => {
                self.advance();
                Ok(self.add(Node::Lit(Value::Null), sp))
            }
            Tok::Kw(Kw::True) => {
                self.advance();
                Ok(self.add(Node::Lit(Value::Bool(true)), sp))
            }
            Tok::Kw(Kw::False) => {
                self.advance();
                Ok(self.add(Node::Lit(Value::Bool(false)), sp))
            }
            Tok::Num(n) => {
                self.advance();
                Ok(match n {
                    Some(d) => self.add(Node::Lit(Value::Num(d)), sp),
                    None => self.add(Node::NumOverflow, sp),
                })
            }
            Tok::Str(s) => {
                self.advance();
                Ok(self.add(Node::Lit(Value::Str(s)), sp))
            }
            Tok::Ident(name) => {
                self.advance();
                Ok(self.add(Node::Name { name, var: Var::Unresolved }, sp))
            }
            Tok::P(P::LBracket) => {
                self.advance();
                let mut elems = Vec::new();
                if !self.at(P::RBracket) {
                    elems.push(self.expr()?);
                    while self.at(P::Comma) {
                        self.advance();
                        elems.push(self.expr()?);
                    }
                }
                let end = self.expect(P::RBracket, "`,` or `]`")?;
                Ok(self.add(Node::Array(elems), sp.join(end)))
            }
            Tok::P(P::LBrace) => {
                if self.is_lambda_head() {
                    self.lambda()
                } else {
                    self.record()
                }
            }
            Tok::P(P::LParen) => {
                self.advance();
                let e = self.expr()?;
                let end = self.expect(P::RParen, "`)`")?;
                Ok(self.add(Node::Group(e), sp.join(end)))
            }
            Tok::Kw(Kw::Do) => {
                self.advance();
                self.expect(P::LBrace, "`{` after do")?;
                let items = self.sequence(|t| matches!(t, Tok::P(P::RBrace)))?;
                let end = self.expect(P::RBrace, "`}`")?;
                Ok(self.add(Node::Do { items }, sp.join(end)))
            }
            Tok::Query(q) => {
                self.advance();
                self.query_node(q, sp)
            }
            Tok::Kw(Kw::If) | Tok::Kw(Kw::Try) => Err(syntax(
                "`if`/`try` must begin an expression; parenthesize it when used as an operand",
                sp,
            )),
            _ => Err(self.unexpected("expression")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn code(src: &str) -> &'static str {
        match tokenize(src).and_then(|t| parse(&t)) {
            Ok(_) => "ok",
            Err(d) => d.code,
        }
    }

    #[test]
    fn syntax_constraints() {
        for s in [
            "[1,]",
            "1;;2",
            "{(x)=>x}",
            "map([1],{x=>x})",
            "map([1],(({x=>x})))",
            "g={x=>x};map([1]){g}",
            "map([1]){x=>x}{y=>y}",
            "1<2==true",
            "[1] | map",
            "[1] | (map)(){x=>x}",
            "q`select ${}`",
            "1+if true then 2 else 3",
            "fun f(x=1) x end fun f;f()",
            ";1",
            "a<b<c",
            "a==b==c",
        ] {
            assert_eq!(code(s), "syntax", "{s}");
        }
        assert_eq!(code("{a:1,\"\\u0061\":2}"), "duplicate_key");
    }

    #[test]
    fn valid_forms() {
        for s in [
            "",
            "1;",
            "fun apply(x){f} f(x) end fun apply apply(10){v=>v+1}",
            "f(){x=>x}(3)",
            "(a<b)==true",
            "z={() =>7};a={(x,y)=>x+y}",
            "(try a catch {e=>b}) | f()",
            "x=9;r={if:1,\"0\":2,x}",
            "if c then a else b | f()",
            "ns.map",
            "([1] | ns.map(){x=>x}).sum()",
            "q`select ${do{x=2;{n:x}}}`",
            "do{fun f() 1 end fun f }",
        ] {
            assert_eq!(code(s), "ok", "{s}");
        }
    }

    #[test]
    fn named_function_delimiters() {
        for s in [
            "f(x)=x", "fun f(x)=x", "fun f(x) then x end fun f",
            "fun f() 1", "fun f() 1 end", "fun f() 1 end fun",
            "fun f() 1 end f", "fun f() 1 end fun g",
            "fun f() 1 end fun F", "fun f() 1 end fun f()",
            "fun f() fun g() 1 end fun f end fun g",
            "fun f() ; end fun f", "fun f(x,) x end fun f",
            "fun f(x=1) x end fun f", "x=fun f() 1 end fun f",
            "fun=1", "end=1", "fun f(end) 1 end fun f",
            "fun f(){fun} 1 end fun f", "end fun f",
        ] {
            assert_eq!(code(s), "syntax", "{s}");
        }
        for s in [
            "fun f() end fun f", "fun f(x) y=x+1;y end fun f f(2)",
            "fun f() fun f() 1 end fun f f() end fun f",
            "fun f(x){k} k(x); end fun f f(1){x=>x}",
            "fun // header\nf() 1 end // end marker\nfun // name\nf",
            "{fun:1,end:2}.fun", "{end:2}?.end",
        ] {
            assert_eq!(code(s), "ok", "{s}");
        }
        let src = "// あ\nfun fac(n) n end fun other";
        let d = parse(&tokenize(src).unwrap()).unwrap_err();
        assert!(d.message.contains("end fun fac"));
        assert_eq!(d.span.start, src.find("other").unwrap());
        let d = parse(&tokenize("fun fac(n) n").unwrap()).unwrap_err();
        assert!(d.message.contains("end fun fac"));
    }
}
