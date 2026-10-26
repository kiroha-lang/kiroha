//! Database-neutral connection selection and value types.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    PostgreSql,
    MySql,
    MariaDb,
}

impl Backend {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "postgresql" => Some(Self::PostgreSql),
            "mysql" => Some(Self::MySql),
            "mariadb" => Some(Self::MariaDb),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::PostgreSql => "postgresql",
            Self::MySql => "mysql",
            Self::MariaDb => "mariadb",
        }
    }
    pub fn validate_params(self, params: &[SqlParam]) -> bool {
        // PostgreSQL text cannot contain NUL; MySQL's binary binding can.
        self != Self::PostgreSql
            || params.iter().all(|p| {
                p.kind != SqlType::Str || !p.value.as_ref().is_some_and(|s| s.contains('\0'))
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlType {
    Null,
    Bool,
    Int,
    Num,
    Str,
    Bytes,
    Json,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlParam {
    pub kind: SqlType,
    /// Exact decimal/JSON text, UTF-8 strings, or base64 for bytes.
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RawRows {
    pub columns: Vec<(String, SqlType)>,
    /// SQL NULL or normalized text. Booleans use true/false; bytes use base64.
    pub rows: Vec<Vec<Option<String>>>,
    pub count: String,
    pub outcome: String,
}
