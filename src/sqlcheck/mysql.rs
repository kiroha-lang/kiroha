//! MySQL/MariaDB lexical contexts and syntax-only value-hole validation.
use super::Reject;
use sqlparser::ast::{Select, SetExpr, Statement, Value, ValueWithSpan, Visit, Visitor};
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ordinary,
    Quote(char),
    Line,
    Block,
}
pub fn translate(segments: &[String]) -> Result<String, Reject> {
    let mut sql = String::new();
    let mut state = State::Ordinary;
    for (index, seg) in segments.iter().enumerate() {
        let chars: Vec<char> = seg.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match state {
                State::Ordinary => match (c, next) {
                    ('?', _) => return Err(Reject::LiteralParameter),
                    ('\'' | '"' | '`', _) => state = State::Quote(c),
                    ('#', _) => state = State::Line,
                    ('-', Some('-'))
                        if chars
                            .get(i + 2)
                            .is_none_or(|c| c.is_ascii_whitespace() || c.is_control()) =>
                    {
                        state = State::Line;
                        i += 1;
                    }
                    ('/', Some('*')) => {
                        if chars.get(i + 2) == Some(&'!')
                            || chars
                                .get(i + 2)
                                .is_some_and(|c| c.eq_ignore_ascii_case(&'m'))
                                && chars.get(i + 3) == Some(&'!')
                        {
                            return Err(Reject::Statement);
                        }
                        state = State::Block;
                        i += 1;
                    }
                    _ => (),
                },
                State::Quote(q) => {
                    if c == '\\' && q != '`' {
                        i += 1;
                    } else if c == q {
                        if next == Some(q) {
                            i += 1;
                        } else {
                            state = State::Ordinary;
                        }
                    }
                }
                State::Line => {
                    if matches!(c, '\r' | '\n') {
                        state = State::Ordinary;
                    }
                }
                State::Block => {
                    if c == '*' && next == Some('/') {
                        state = State::Ordinary;
                        i += 1;
                    }
                }
            }
            i += 1;
        }
        sql.push_str(seg);
        if index + 1 < segments.len() {
            if state != State::Ordinary {
                return Err(Reject::HoleContext);
            }
            sql.push_str(" ? ");
        }
    }
    if !matches!(state, State::Ordinary | State::Line) {
        return Err(Reject::Statement);
    }
    let statements = Parser::parse_sql(&MySqlDialect {}, &sql).map_err(|_| Reject::Statement)?;
    if statements.len() > 1 {
        return Err(Reject::MultipleStatements);
    }
    let Some(stmt) = statements.first() else {
        return Err(Reject::Statement);
    };
    fn query_body(body: &SetExpr) -> bool {
        match body {
            SetExpr::Select(_) => true,
            SetExpr::Query(q) => query_body(&q.body),
            SetExpr::SetOperation { left, right, .. } => query_body(left) && query_body(right),
            _ => false,
        }
    }
    if !matches!(
        stmt,
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_)
    ) && !matches!(stmt,Statement::Query(q) if query_body(&q.body))
    {
        return Err(Reject::Statement);
    }
    struct Guard {
        holes: usize,
    }
    impl Visitor for Guard {
        type Break = Reject;
        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Reject> {
            if select.into.is_some() {
                ControlFlow::Break(Reject::Statement)
            } else {
                ControlFlow::Continue(())
            }
        }
        fn pre_visit_statement(&mut self, s: &Statement) -> ControlFlow<Reject> {
            if matches!(s,Statement::Insert(i) if i.replace_into) {
                return ControlFlow::Break(Reject::Statement);
            }
            if matches!(
                s,
                Statement::Query(_)
                    | Statement::Insert(_)
                    | Statement::Update(_)
                    | Statement::Delete(_)
            ) {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(Reject::Statement)
            }
        }
        fn post_visit_value(&mut self, v: &ValueWithSpan) -> ControlFlow<Reject> {
            if let Value::Placeholder(p) = &v.value {
                if p != "?" {
                    return ControlFlow::Break(Reject::LiteralParameter);
                }
                self.holes += 1;
            }
            ControlFlow::Continue(())
        }
    }
    let mut guard = Guard { holes: 0 };
    if let ControlFlow::Break(reject) = stmt.visit(&mut guard) {
        return Err(reject);
    }
    if guard.holes != segments.len().saturating_sub(1) {
        return Err(Reject::HoleContext);
    }
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parts(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn mysql_holes_are_values() {
        for source in [
            vec!["select ", " as n"],
            vec!["select * from `order` where id=", ""],
            vec!["insert into t(a,b) values (", ",", ")"],
            vec!["with t as (select ", " as x) select x from t"],
            vec!["select '?' as question, ", " as x -- done"],
        ] {
            assert!(translate(&parts(&source)).is_ok(), "{source:?}");
        }
        for source in [
            vec!["select '", "'"],
            vec!["select \"", "\""],
            vec!["select `", "`"],
            vec!["select 1 # ", ""],
            vec!["select 1 -- ", ""],
            vec!["select /* ", " */ 1"],
        ] {
            assert_eq!(
                translate(&parts(&source)),
                Err(Reject::HoleContext),
                "{source:?}"
            );
        }
        for source in [
            "select ?",
            "select 1;select 2",
            "create table t(x int)",
            "begin",
            "commit",
            "replace into t(x) values (1)",
            "set @x=1",
            "select 1 into outfile '/tmp/x'",
            "/*! select 1 */",
            "/*M! select 1 */",
        ] {
            assert!(translate(&parts(&[source])).is_err(), "{source}");
        }
        assert!(translate(&parts(&["select * from ", ""])).is_err());
    }
}
