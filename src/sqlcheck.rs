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
    Comma,
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
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c') {
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
                        ',' => Tok::Comma,
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
    // Inspect the *entire* tree through its derived Debug form, which visits
    // every field (VALUES lists, LIMIT/OFFSET, RETURNING, MERGE actions,
    // window/filter clauses, arrays, ...). String contents (SQL literals and
    // identifiers) are skipped so they cannot be mistaken for nodes.
    let tree = format!("{:?}", parsed.protobuf);
    let structure = strip_debug_strings(&tree);
    // SELECT INTO anywhere (PostgreSQL honours it in the leftmost branch of a
    // set operation) is excluded.
    if structure.contains("into_clause: Some(") {
        return Err(Reject::Statement);
    }
    // A marker is one source location; multi-column SET shares one source
    // expression between columns, so count distinct (number, location).
    let mut seen: Vec<(usize, String)> = Vec::new();
    let needle = "ParamRef(ParamRef { number: ";
    let mut rest = structure.as_str();
    while let Some(p) = rest.find(needle) {
        rest = &rest[p + needle.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let k: usize = digits.parse().map_err(|_| Reject::Statement)?;
        if k == 0 || k > holes {
            return Err(Reject::LiteralParameter);
        }
        let loc: String = rest
            .find("location: ")
            .map(|l| rest[l + 10..].chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect())
            .unwrap_or_default();
        if !seen.contains(&(k, loc.clone())) {
            seen.push((k, loc));
        }
    }
    // libpg_query accepts `typename $n` (e.g. `select x $1`) as a typed
    // parameter, which the PostgreSQL 16 server rejects: a hole right after a
    // name is not a value position. Such a TypeCast has its own location
    // before the ParamRef and does not start with CAST.
    if typed_param_extension(&structure, sql) {
        return Err(Reject::HoleContext);
    }
    let mut counts = vec![0usize; holes + 1];
    for (k, _) in &seen {
        counts[*k] += 1;
    }
    if counts[1..].iter().any(|c| *c != 1) {
        return Err(Reject::HoleContext);
    }
    Ok(())
}

const ALLOWED: &[&str] = &["select", "insert", "update", "delete", "merge"];

/// Lexical pre-check of the statement shape: at most one top-level `;`
/// (only at the end), and the main statement — after an optional WITH clause
/// and parentheses — starts with an allowed command. This excludes top-level
/// TABLE/VALUES shorthands, which libpg_query represents as ordinary SELECTs,
/// while allowing them inside a SELECT (e.g. `select 1 union table a`).
fn validate_tokens(toks: &[Tok]) -> Result<(), Reject> {
    let mut depth = 0i64;
    let mut top_semis = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Semi if depth <= 0 => top_semis.push(i),
            _ => {}
        }
    }
    if top_semis.len() > 1 || top_semis.first().is_some_and(|&i| i + 1 != toks.len()) {
        return Err(Reject::MultipleStatements);
    }
    let mut k = 0;
    while toks.get(k) == Some(&Tok::LParen) {
        k += 1;
    }
    if matches!(toks.get(k), Some(Tok::Word(w)) if w == "with") {
        k = skip_with(toks, k + 1)?;
        while toks.get(k) == Some(&Tok::LParen) {
            k += 1;
        }
    }
    match toks.get(k) {
        Some(Tok::Word(w)) if ALLOWED.contains(&w.as_str()) => Ok(()),
        _ => Err(Reject::Statement),
    }
}

/// Skip `[RECURSIVE] name [(cols)] AS [NOT] [MATERIALIZED] (body) [SEARCH ...]
/// [CYCLE ...] {, ...}` and return the index of the main statement.
fn skip_with(toks: &[Tok], mut k: usize) -> Result<usize, Reject> {
    if matches!(toks.get(k), Some(Tok::Word(w)) if w == "recursive") {
        k += 1;
    }
    loop {
        if !matches!(toks.get(k), Some(Tok::Word(_))) {
            return Err(Reject::Statement);
        }
        k += 1;
        // U&"name" UESCAPE 'c'
        if matches!(toks.get(k), Some(Tok::Word(w)) if w == "uescape") {
            k += 2;
        }
        if toks.get(k) == Some(&Tok::LParen) {
            k = skip_group(toks, k)?;
        }
        if !matches!(toks.get(k), Some(Tok::Word(w)) if w == "as") {
            return Err(Reject::Statement);
        }
        k += 1;
        while matches!(toks.get(k), Some(Tok::Word(w)) if w == "not" || w == "materialized") {
            k += 1;
        }
        if toks.get(k) != Some(&Tok::LParen) {
            return Err(Reject::Statement);
        }
        k = skip_group(toks, k)?;
        // SEARCH / CYCLE clauses: skip their words, column-list commas and
        // literals. A comma followed by `name [(cols)] AS` starts the next CTE;
        // a main command word or a parenthesis starts the main statement.
        let mut next_cte = false;
        if matches!(toks.get(k), Some(Tok::Word(w)) if w == "search" || w == "cycle") {
            while let Some(t) = toks.get(k) {
                match t {
                    Tok::Word(w) if MAIN_START.contains(&w.as_str()) => return Ok(k),
                    Tok::LParen => return Ok(k),
                    Tok::Comma if is_cte_start(toks, k + 1) => {
                        next_cte = true;
                        k += 1;
                        break;
                    }
                    _ => k += 1,
                }
            }
            if !next_cte {
                return Ok(k);
            }
            continue;
        }
        match toks.get(k) {
            Some(Tok::Comma) => k += 1,
            _ => return Ok(k),
        }
    }
}

/// `name [UESCAPE 'c'] [(cols)] AS` at index k.
fn is_cte_start(toks: &[Tok], mut k: usize) -> bool {
    if !matches!(toks.get(k), Some(Tok::Word(_))) {
        return false;
    }
    k += 1;
    if matches!(toks.get(k), Some(Tok::Word(w)) if w == "uescape") {
        k += 2;
    }
    if toks.get(k) == Some(&Tok::LParen) {
        match skip_group(toks, k) {
            Ok(e) => k = e,
            Err(_) => return false,
        }
    }
    matches!(toks.get(k), Some(Tok::Word(w)) if w == "as")
}

const MAIN_START: &[&str] = &["select", "insert", "update", "delete", "merge", "table", "values"];

fn skip_group(toks: &[Tok], mut k: usize) -> Result<usize, Reject> {
    let mut depth = 0i64;
    while let Some(t) = toks.get(k) {
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

fn number_after(s: &str, key: &str) -> Option<i64> {
    let p = s.find(key)?;
    let rest = &s[p + key.len()..];
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    n.parse().ok()
}

fn typed_param_extension(structure: &str, sql: &str) -> bool {
    let head = "TypeCast(TypeCast { arg: Some(Node { node: Some(ParamRef(ParamRef { ";
    let mut from = 0;
    while let Some(p) = structure[from..].find(head) {
        let start = from + p + "TypeCast(".len();
        from = start;
        // brace-match the TypeCast struct
        let bytes = structure.as_bytes();
        let mut depth = 0i64;
        let mut end = start;
        for (i, &b) in bytes.iter().enumerate().skip(start) {
            match b {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &structure[start..end];
        let param_loc = match number_after(body, "location: ") {
            Some(l) => l,
            None => continue,
        };
        // the TypeCast's own location is its last top-level field
        let own_loc = match body.rfind("location: ").and_then(|i| number_after(&body[i..], "location: ")) {
            Some(l) => l,
            None => continue,
        };
        // The grammar extension builds the cast without a source location.
        if own_loc < 0 {
            return true;
        }
        if own_loc < param_loc {
            let (a, b) = (own_loc as usize, param_loc as usize);
            let between = sql.get(a..b).unwrap_or("").trim_start().to_ascii_lowercase();
            if !between.starts_with("cast") {
                return true;
            }
        }
    }
    false
}

/// Remove the contents of double-quoted strings from Rust Debug output.
fn strip_debug_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => {
                    in_str = false;
                    out.push('"');
                }
                _ => {}
            }
        } else {
            if c == '"' {
                in_str = true;
            }
            out.push(c);
        }
    }
    out
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
        assert!(translate(&s(&["select $$a;b$$"])).is_ok());
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
        assert!(translate(&s(&["select E'\\'' || ", ""])).is_ok());
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
        assert_eq!(translate(&s(&["with a as (select 1) table a"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["with a as (select 1 as x) (table a)"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["((table a))"])), Err(Reject::Statement));
        assert!(translate(&s(&["select 1 union table a"])).is_ok());
        assert_eq!(translate(&s(&["select x", ""])), Err(Reject::HoleContext));
        assert_eq!(translate(&s(&["select int ", ""])), Err(Reject::HoleContext));
        assert!(translate(&s(&["select ", "::int"])).is_ok());
        assert!(translate(&s(&["select cast(", " as int)"])).is_ok());
        assert!(translate(&s(&["select CAST ( ", " AS numeric)"])).is_ok());
        assert_eq!(
            translate(&s(&["with recursive r(n) as (select 1 union all select n+1 from r where n<3) search depth first by n set ord, u as (select 1 as z) table u"])),
            Err(Reject::Statement)
        );
        assert_eq!(
            translate(&s(&["with recursive r(n) as (select 1) cycle n set c to true default false using p, u as (select 1 as z) (table u)"])),
            Err(Reject::Statement)
        );
        assert!(translate(&s(&["with recursive r(n) as (select 1 union all select n+1 from r where n<3) search depth first by n set ord, u as (select 1 as z) select * from u"])).is_ok());
        assert!(translate(&s(&["with recursive r(n) as (select 1 union all select n+1 from r where n<3) search breadth first by n, n set ord select * from r"])).is_ok());
        assert!(translate(&s(&["select 1 as \u{3000}$1"])).is_ok());
        assert!(translate(&s(&["with U&\"d!0061\" UESCAPE '!' as (select 1) select 1"])).is_ok());
        assert!(translate(&s(&["with a as (select 1 as x) select x from a union all table a"])).is_ok());
        assert_eq!(translate(&s(&["select 1 as x into kiroha_t union select 2"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["with a as (select 1) select 1 into t union select 2"])), Err(Reject::Statement));
        assert_eq!(translate(&s(&["select 1 as x;;"])), Err(Reject::MultipleStatements));
        assert_eq!(translate(&s(&["select 1; ;"])), Err(Reject::MultipleStatements));
        assert!(translate(&s(&["select 1; -- done"])).is_ok());
        assert!(translate(&s(&["update t set (a,b) = (", ", ", ")"])).is_ok());
        assert!(translate(&s(&["insert into t values (1) on conflict (a) do update set (a,b) = (", ", ", ")"])).is_ok());
        assert!(translate(&s(&["select ", "::text, 'ParamRef(ParamRef { number: 1'"])).is_ok());
        assert!(translate(&s(&["select 1 as \"ParamRef(ParamRef { number: 9\""])).is_ok());
        for ok in [
            vec!["insert into t values (", ")"],
            vec!["select 1 limit ", ""],
            vec!["select 1 offset ", ""],
            vec!["select (array[1,2])[", "]"],
            vec!["insert into t(v) values (1) returning ", ""],
            vec!["update t set v=1 returning ", ""],
            vec!["merge into t using s on t.id=s.id when matched then update set v=", ""],
            vec!["insert into t(v) values (1) on conflict (v) do update set v=", ""],
            vec!["select * from (values (", ")) v(x)"],
            vec!["select sum(x) over (order by ", ") from t"],
            vec!["select count(*) filter (where x=", ") from t"],
            vec!["select array[", "]"],
            vec!["select ", " collate \"C\""],
        ] {
            assert!(translate(&s(&ok)).is_ok(), "{ok:?}");
        }
    }
}
