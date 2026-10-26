//! Backend-aware query translation and statement validation (spec §11.2).
//!
//! `postgres` tracks PostgreSQL lexical states (single/E/U&/B/X strings,
//! quoted identifiers, nested block comments, line comments, dollar quotes);
//! `mysql` does the same for MySQL/MariaDB (quotes, backticks, `#`/`-- `/block
//! comments, version-conditional comments rejected). Either way the lexer runs
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

mod mysql;
mod postgres;

pub fn translate(segments: &[String]) -> Result<String, Reject> {
    postgres::translate(segments)
}
pub fn translate_for(
    backend: crate::database::Backend,
    segments: &[String],
) -> Result<String, Reject> {
    match backend {
        crate::database::Backend::PostgreSql => postgres::translate(segments),
        _ => mysql::translate(segments),
    }
}
