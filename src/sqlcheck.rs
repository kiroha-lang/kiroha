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

const ALLOWED: &[&str] = &["select", "insert", "update", "delete", "merge"];

fn validate_tokens(toks: &[Tok]) -> Result<(), Reject> {
    // Split at top-level semicolons.
    let mut depth = 0i64;
    let mut semi_at = None;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Semi if depth <= 0 => {
                semi_at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let stmt = match semi_at {
        Some(i) => {
            if i + 1 < toks.len() {
                return Err(Reject::MultipleStatements);
            }
            &toks[..i]
        }
        None => toks,
    };
    if stmt.is_empty() {
        return Err(Reject::Statement);
    }
    // Leading parentheses may wrap a SELECT.
    let mut k = 0;
    while k < stmt.len() && stmt[k] == Tok::LParen {
        k += 1;
    }
    let first = match stmt.get(k) {
        Some(Tok::Word(w)) => w.as_str(),
        _ => return Err(Reject::Statement),
    };
    let main = if first == "with" {
        main_after_with(stmt, k + 1)?
    } else {
        first.to_string()
    };
    if !ALLOWED.contains(&main.as_str()) {
        return Err(Reject::Statement);
    }
    if (main == "select") && has_top_level_into(stmt) {
        return Err(Reject::Statement);
    }
    Ok(())
}

/// Skip a WITH clause and return the main statement keyword.
fn main_after_with(stmt: &[Tok], mut k: usize) -> Result<String, Reject> {
    if let Some(Tok::Word(w)) = stmt.get(k) {
        if w == "recursive" {
            k += 1;
        }
    }
    loop {
        // name
        match stmt.get(k) {
            Some(Tok::Word(_)) => k += 1,
            _ => return Err(Reject::Statement),
        }
        // optional column list
        if stmt.get(k) == Some(&Tok::LParen) {
            k = skip_parens(stmt, k)?;
        }
        match stmt.get(k) {
            Some(Tok::Word(w)) if w == "as" => k += 1,
            _ => return Err(Reject::Statement),
        }
        while let Some(Tok::Word(w)) = stmt.get(k) {
            if w == "not" || w == "materialized" {
                k += 1;
            } else {
                break;
            }
        }
        if stmt.get(k) != Some(&Tok::LParen) {
            return Err(Reject::Statement);
        }
        // CTE body must itself be an allowed statement.
        match stmt.get(k + 1) {
            Some(Tok::Word(w)) if ALLOWED.contains(&w.as_str()) || w == "with" || w == "values" => {}
            Some(Tok::LParen) => {}
            _ => return Err(Reject::Statement),
        }
        k = skip_parens(stmt, k)?;
        // SEARCH/CYCLE clauses are skipped until a comma or main keyword.
        while let Some(t) = stmt.get(k) {
            match t {
                Tok::Other => {
                    k += 1;
                    break;
                }
                Tok::Word(w) if ALLOWED.contains(&w.as_str()) => return Ok(w.clone()),
                Tok::LParen => {
                    let mut j = k;
                    while stmt.get(j) == Some(&Tok::LParen) {
                        j += 1;
                    }
                    if let Some(Tok::Word(w)) = stmt.get(j) {
                        if w == "select" {
                            return Ok("select".into());
                        }
                    }
                    return Err(Reject::Statement);
                }
                _ => return Err(Reject::Statement),
            }
        }
    }
}

fn skip_parens(stmt: &[Tok], mut k: usize) -> Result<usize, Reject> {
    let mut depth = 0;
    while let Some(t) = stmt.get(k) {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => {
                depth -= 1;
                if depth == 0 {
                    return Ok(k + 1);
                }
            }
            _ => {}
        }
        k += 1;
    }
    Err(Reject::Statement)
}

fn has_top_level_into(stmt: &[Tok]) -> bool {
    let mut depth = 0;
    let mut seen_select_body = false;
    for t in stmt {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Word(w) if depth <= 1 && w == "select" => seen_select_body = true,
            Tok::Word(w) if w == "into" && seen_select_body && depth <= 1 => return true,
            _ => {}
        }
    }
    false
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
    }
}
