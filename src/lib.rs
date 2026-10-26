//! Kiroha 0.3.0 / Web1 language and runtime.
//!
//! Pipeline: [`lexer`] → [`parser`] → [`resolve`] produce a resolved
//! [`ast::Program`]; [`runtime::Runtime`] evaluates it against a
//! [`host::Host`] (the deterministic [`host::virtual_host`] for conformance,
//! or [`host::real`] for production).

pub mod alloc;
pub mod ast;
pub mod builtins;
pub mod cli;
pub mod conformance;
pub mod decimal;
pub mod diag;
pub mod host;
pub mod interrupt;
pub mod json;
pub mod lexer;
pub mod parser;
pub mod resolve;
pub mod runtime;
pub mod schema;
pub mod sqlcheck;
pub mod url;
pub mod value;
pub mod wire;

pub const IMPLEMENTATION: &str = concat!("kiroha-rs/", env!("CARGO_PKG_VERSION"));

/// Lex, parse and resolve a complete program.
pub fn compile(src: &str) -> Result<ast::Program, diag::Diagnostic> {
    let toks = lexer::tokenize(src)?;
    let mut prog = parser::parse(&toks)?;
    resolve::resolve(&mut prog, &builtins::prelude_index)?;
    Ok(prog)
}
