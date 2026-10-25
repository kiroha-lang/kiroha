//! PostgreSQL-aware query translation and statement validation (spec §11.2).
//!
//! The lexer tracks PostgreSQL lexical states (single/E/U&/B/X strings,
//! quoted identifiers, nested block comments, line comments, dollar quotes)
//! across query segments, so a hole's lexical context is known exactly.

#[derive(Debug, PartialEq, Eq)]
pub enum Reject {
    Statement,
    MultipleStatements,
    HoleContext,
    LiteralParameter,
}

impl Reject {
    pub fn reason(&self) -> &'static str {
        match self {
            Reject::Statement => "statement",
            Reject::MultipleStatements => "multiple_statements",
            Reject::HoleContext => "hole_context",
            Reject::LiteralParameter => "literal_parameter",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Ordinary,
    Single { backslash: bool },
    Ident,
    LineComment,
    Block(u32),
    Dollar(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    Param,
    LParen,
    RParen,
    Semi,
    Other,
}

/// Translate segments + holes into SQL text with ` $n ` markers and
/// validate the statement. Returns the translated SQL.
pub fn translate(segments: &[String]) -> Result<String, Reject> {
    let mut sql = String::new();
    let mut state = State::Ordinary;
    let mut toks: Vec<Tok> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        lex_segment(seg, &mut state, &mut toks)?;
        sql.push_str(seg);
        if i + 1 < segments.len() {
            if state != State::Ordinary {
                return Err(Reject::HoleContext);
            }
            sql.push_str(&format!(" ${} ", i + 1));
            toks.push(Tok::Param);
        }
    }
    if !matches!(state, State::Ordinary | State::LineComment) {
        return Err(Reject::Statement);
    }
    validate_tokens(&toks)?;
    validate_parsed(&sql, segments.len() - 1)?;
    Ok(sql)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_ident_char(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '$'
}

fn lex_segment(seg: &str, state: &mut State, toks: &mut Vec<Tok>) -> Result<(), Reject> {
    let chars: Vec<char> = seg.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match state.clone() {
            State::Ordinary => {
                if c.is_whitespace() {
                    i += 1;
                } else if c == '-' && next == Some('-') {
                    *state = State::LineComment;
                    i += 2;
                } else if c == '/' && next == Some('*') {
                    *state = State::Block(1);
                    i += 2;
                } else if c == '\'' {
                    *state = State::Single { backslash: false };
                    toks.push(Tok::Other);
                    i += 1;
                } else if c == '"' {
                    *state = State::Ident;
                    toks.push(Tok::Word(String::new()));
                    i += 1;
                } else if c == '$' {
                    if next.is_some_and(|d| d.is_ascii_digit()) {
                        return Err(Reject::LiteralParameter);
                    }
                    // dollar quote $tag$
                    let mut j = i + 1;
                    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || (chars[j] as u32) >= 0x80) {
                        j += 1;
                    }
                    let tag: String = chars[i + 1..j].iter().collect();
                    if j < chars.len() && chars[j] == '$' && !tag.starts_with(|d: char| d.is_ascii_digit()) {
                        *state = State::Dollar(tag);
                        toks.push(Tok::Other);
                        i = j + 1;
                    } else {
                        toks.push(Tok::Other);
                        i += 1;
                    }
                } else if is_ident_start(c) {
                    let mut j = i;
                    while j < chars.len() && is_ident_char(chars[j]) {
                        j += 1;
                    }
                    let word: String = chars[i..j].iter().collect();
                    // String prefixes: E'..', B'..', X'..', U&'..', N'..'
                    let lower = word.to_ascii_lowercase();
                    if j < chars.len() && chars[j] == '\'' && matches!(lower.as_str(), "e" | "b" | "x" | "n") {
                        *state = State::Single { backslash: lower == "e" };
                        toks.push(Tok::Other);
                        i = j + 1;
                    } else if lower == "u" && chars.get(j) == Some(&'&') && matches!(chars.get(j + 1), Some('\'') | Some('"')) {
                        if chars[j + 1] == '\'' {
                            *state = State::Single { backslash: false };
                            toks.push(Tok::Other);
                        } else {
                            *state = State::Ident;
                            toks.push(Tok::Word(String::new()));
                        }
                        i = j + 2;
                    } else {
                        toks.push(Tok::Word(lower));
                        i = j;
                    }
                } else if c.is_ascii_digit() {
                    let mut j = i;
                    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_') {
                        j += 1;
                    }
                    toks.push(Tok::Other);
                    i = j;
                } else {
                    toks.push(match c {
                        '(' => Tok::LParen,
                        ')' => Tok::RParen,
                        ';' => Tok::Semi,
                        _ => Tok::Other,
                    });
                    i += 1;
                }
            }
            State::Single { backslash } => {
                if backslash && c == '\\' {
                    i += 2;
                } else if c == '\'' {
                    if next == Some('\'') {
                        i += 2;
                    } else {
                        *state = State::Ordinary;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            State::Ident => {
                if c == '"' {
                    if next == Some('"') {
                        i += 2;
                    } else {
                        *state = State::Ordinary;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            State::LineComment => {
                if c == '\n' || c == '\r' {
                    *state = State::Ordinary;
                }
                i += 1;
            }
            State::Block(depth) => {
                if c == '/' && next == Some('*') {
                    *state = State::Block(depth + 1);
                    i += 2;
                } else if c == '*' && next == Some('/') {
                    *state = if depth == 1 { State::Ordinary } else { State::Block(depth - 1) };
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::Dollar(tag) => {
                let close: Vec<char> = format!("${tag}$").chars().collect();
                if chars[i..].starts_with(&close) {
                    *state = State::Ordinary;
                    i += close.len();
                } else {
                    i += 1;
                }
            }
        }
    }
    Ok(())
}

/// Full PostgreSQL grammar check (libpg_query): exactly one statement of an
/// allowed kind, no SELECT INTO, and every generated marker parsed exactly
/// once as a ParamRef. A syntax error at a generated marker is `hole_context`.
fn validate_parsed(sql: &str, holes: usize) -> Result<(), Reject> {
    use pg_query::protobuf::node::Node as N;
    let parsed = match pg_query::parse(sql) {
        Ok(p) => p,
        Err(e) => {
            let msg = e.to_string();
            let at_marker = (1..=holes).any(|n| msg.contains(&format!("\"${n}\"")));
            return Err(if at_marker { Reject::HoleContext } else { Reject::Statement });
        }
    };
    let stmts = &parsed.protobuf.stmts;
    match stmts.len() {
        0 => return Err(Reject::Statement),
        1 => {}
        _ => return Err(Reject::MultipleStatements),
    }
    let node = stmts[0].stmt.as_ref().and_then(|n| n.node.as_ref()).ok_or(Reject::Statement)?;
    let allowed_dml = |n: &N| matches!(n, N::InsertStmt(_) | N::UpdateStmt(_) | N::DeleteStmt(_) | N::MergeStmt(_));
    match node {
        N::SelectStmt(sel) => {
            if sel.into_clause.is_some() || !sel.values_lists.is_empty() {
                return Err(Reject::Statement);
            }
        }
        n if allowed_dml(n) => {}
        _ => return Err(Reject::Statement),
    }
    let mut counts = vec![0usize; holes + 1];
    for (n, _, _, _) in parsed.protobuf.nodes() {
        match n {
            pg_query::NodeRef::ParamRef(p) => {
                let k = p.number as usize;
                if k == 0 || k > holes {
                    return Err(Reject::LiteralParameter);
                }
                counts[k] += 1;
            }
            pg_query::NodeRef::SelectStmt(s) if s.into_clause.is_some() => return Err(Reject::Statement),
            _ => {}
        }
    }
    if counts[1..].iter().any(|c| *c != 1) {
        return Err(Reject::HoleContext);
    }
    Ok(())
}

const ALLOWED: &[&str] = &["select", "insert", "update", "delete", "merge"];

/// Lexical pre-check: the statement must start with an allowed command word
/// (after optional parentheses). This excludes top-level TABLE/VALUES
/// shorthands, which libpg_query represents as ordinary SELECTs.
fn validate_tokens(toks: &[Tok]) -> Result<(), Reject> {
    let mut k = 0;
    while k < toks.len() && toks[k] == Tok::LParen {
        k += 1;
    }
    match toks.get(k) {
        Some(Tok::Word(w)) if ALLOWED.contains(&w.as_str()) || w == "with" => Ok(()),
        Some(_) => Err(Reject::Statement),
        None => Err(Reject::Statement),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn translation() {
        assert_eq!(
            translate(&s(&["select ", "::text as z, ", "::boolean as b"])).unwrap(),
            "select  $1 ::text as z,  $2 ::boolean as b"
        );
        assert_eq!(translate(&s(&["select 1;select 2"])), Err(Reject::MultipleStatements));
        assert_eq!(translate(&s(&["select $1"])), Err(Reject::LiteralParameter));
        assert_eq!(translate(&s(&["select '", "'"])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select 'a;b'; -- c"])), Ok("select 'a;b'; -- c".into()));
        assert_eq!(translate(&s(&["select $$a;b$$"])).is_ok(), true);
        assert_eq!(translate(&s(&["select /* ", " */"])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["create table t(x int)"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["values (1)"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["select 1 into t"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["begin"])), Err(Reject::Statement));
        assert!(translate(&s(&["insert into t(v) values (1) returning id"])).is_ok());
        assert!(translate(&s(&["with x as (delete from t returning *) select * from x"])).is_ok());
        assert!(translate(&s(&["with x as (select 1) insert into t select * from x"])).is_ok());
        assert!(translate(&s(&["(select 1)"])).is_ok());
        assert_eq!(translate(&s(&["  "])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["select E'\\'' || ", ""])).is_ok(), true);
        assert_eq!(translate(&s(&["select * from ", ""])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select ", "", ""])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select t.", " from t"])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select 1 ", " 2"])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select (1;2)"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["(delete from t)"])), Err(Reject::Statement));
        assert!(translate(&s(&["with recursive t(n) as (select 1 union all select n+1 from t where n<5) search depth first by n set ord select * from t"])).is_ok());
        assert!(translate(&s(&["with recursive t(n) as (select 1 union all select n+1 from t where n<5) cycle n set is_cycle using path select * from t"])).is_ok());
        assert!(translate(&s(&["with a as (select 1), b as (insert into t select 2 returning *) select * from b"])).is_ok());
        assert_eq!(translate(&s(&["table t"])), Err(Reject::Statement));
    }
}
