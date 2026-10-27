//! The `kiroha` command-line protocol (spec §15.1).

use crate::diag::{LineIndex, Span};
use crate::host::config::{parse_host_file, HostConfig};
use crate::host::real::RealHost;
use crate::host::HostEvent;
use crate::runtime::{RootOutcome, Runtime};
use crate::value::{err, Value};
use std::io::{Read, Write};
use std::rc::Rc;

pub const SPEC: &str = include_str!("../spec.md");

pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

fn line(v: &Value) -> String {
    let mut s = crate::json::dump(v);
    s.push('\n');
    s
}

fn cli_failure(json_mode: bool, code: &str, message: &str) -> Output {
    if json_mode {
        let env = Value::rec(vec![
            ("protocol", Value::str("kiroha-cli/1")),
            ("ok", Value::Bool(false)),
            ("phase", Value::str("cli")),
            ("error", err::make(code, message, vec![])),
            ("span", Value::Null),
        ]);
        Output { stdout: line(&env), stderr: String::new(), code: 2 }
    } else {
        Output { stdout: String::new(), stderr: format!("kiroha: {message}\n"), code: 2 }
    }
}

fn span_value(src: &str, s: Span) -> Value {
    let (l, c) = LineIndex::new(src).line_col(s.start);
    Value::rec(vec![
        ("start", Value::int(s.start as i64)),
        ("end", Value::int(s.end as i64)),
        ("line", Value::int(l as i64)),
        ("column", Value::int(c as i64)),
    ])
}

fn failure_envelope(phase: &str, error: Value, span: Value) -> Value {
    Value::rec(vec![
        ("protocol", Value::str("kiroha-cli/1")),
        ("ok", Value::Bool(false)),
        ("phase", Value::str(phase)),
        ("error", error),
        ("span", span),
    ])
}

struct Parsed {
    command: String,
    positional: Vec<String>,
    json: bool,
    compact: bool,
    host: Option<String>,
}

/// JSON mode exists only for check/run/conformance (§15.1); elsewhere usage
/// errors go to stderr with stdout empty.
fn json_command(args: &[String]) -> bool {
    let mut i = 0;
    let mut command = None;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => i += 1,
            a if a.starts_with("--") => {}
            a => {
                command = Some(a);
                break;
            }
        }
        i += 1;
    }
    args.iter().any(|a| a == "--json") && matches!(command, Some("check" | "run" | "conformance"))
}

fn parse_args(args: &[String]) -> Result<Parsed, (bool, String)> {
    let json_mode = json_command(args);
    let mut p = Parsed { command: String::new(), positional: Vec::new(), json: false, compact: false, host: None };
    let mut i = 0;
    let mut seen: Vec<&str> = Vec::new();
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--json" | "--compact" | "--version" | "--host" => {
                if seen.contains(&a) {
                    return Err((json_mode, format!("duplicate option {a}")));
                }
                seen.push(a);
                match a {
                    "--json" => p.json = true,
                    "--compact" => p.compact = true,
                    "--version" => {
                        if !p.command.is_empty() {
                            return Err((json_mode, "--version takes no command".into()));
                        }
                        p.command = "--version".into();
                    }
                    _ => {
                        i += 1;
                        let v = args.get(i).ok_or((json_mode, "--host requires a path".to_string()))?;
                        p.host = Some(v.clone());
                    }
                }
            }
            s if s.starts_with("--") => return Err((json_mode, format!("unknown option {s}"))),
            s => {
                if p.command.is_empty() {
                    p.command = s.to_string();
                } else {
                    p.positional.push(s.to_string());
                }
            }
        }
        i += 1;
    }
    Ok(p)
}

fn read_source(path: &str, stdin: &mut dyn Read) -> Result<Vec<u8>, String> {
    if path == "-" {
        let mut b = Vec::new();
        stdin.read_to_end(&mut b).map_err(|e| e.to_string())?;
        Ok(b)
    } else {
        std::fs::read(path).map_err(|e| e.to_string())
    }
}

/// Run the CLI. `stdin` supplies `-` sources.
pub fn main_with(args: &[String], stdin: &mut dyn Read) -> Output {
    let p = match parse_args(args) {
        Ok(p) => p,
        Err((json, msg)) => return cli_failure(json, "usage", &msg),
    };
    match p.command.as_str() {
        "--version" => {
            if !p.positional.is_empty() || p.json || p.compact || p.host.is_some() {
                return cli_failure(false, "usage", "--version takes no other arguments");
            }
            let v = Value::rec(vec![
                ("implementation", Value::str(crate::IMPLEMENTATION)),
                ("language", Value::str("0.8.0")),
                ("profile", Value::str("Web1")),
                ("cli", Value::str("kiroha-cli/1")),
                ("conformance", Value::str("kiroha-conformance/1")),
            ]);
            Output { stdout: line(&v), stderr: String::new(), code: 0 }
        }
        "spec" => {
            if !p.compact || p.json || p.host.is_some() || !p.positional.is_empty() {
                return cli_failure(false, "usage", "usage: kiroha spec --compact");
            }
            match crate::conformance::extract_block(SPEC, "GENERATION_CARD", "text") {
                Ok(card) => Output { stdout: card, stderr: String::new(), code: 0 },
                Err(e) => cli_failure(false, "harness_format", &e),
            }
        }
        "check" => {
            if !p.json || p.compact || p.host.is_some() || p.positional.len() != 1 {
                return cli_failure(p.json, "usage", "usage: kiroha check SOURCE --json");
            }
            let bytes = match read_source(&p.positional[0], stdin) {
                Ok(b) => b,
                Err(e) => return cli_failure(true, "source_io", &format!("cannot read source: {e}")),
            };
            let limit = HostConfig::default_with_root(String::new()).limits.source_bytes;
            if bytes.len() as u64 > limit {
                return cli_failure(true, "source_limit", "source exceeds source_bytes");
            }
            check_source(&bytes)
        }
        "run" => {
            if !p.json || p.compact || p.positional.len() != 1 {
                return cli_failure(p.json, "usage", "usage: kiroha run SOURCE --json [--host HOST.json]");
            }
            let launch = std::env::current_dir().map(|d| d.to_string_lossy().to_string()).unwrap_or_else(|_| "/".into());
            let config = match &p.host {
                None => HostConfig::default_with_root(launch),
                Some(path) => {
                    let text = match std::fs::read(path) {
                        Ok(b) => b,
                        Err(e) => return cli_failure(true, "configuration", &format!("cannot read host file: {e}")),
                    };
                    let text = match std::str::from_utf8(&text) {
                        Ok(t) => t,
                        Err(_) => return cli_failure(true, "configuration", "host file is not UTF-8"),
                    };
                    let v = match crate::json::parse(text) {
                        Ok(v) => v,
                        Err(_) => return cli_failure(true, "configuration", "host file is not strict JSON"),
                    };
                    match parse_host_file(&v, &launch) {
                        Ok(c) => c,
                        Err(e) => return cli_failure(true, "configuration", &e),
                    }
                }
            };
            let bytes = match read_source(&p.positional[0], stdin) {
                Ok(b) => b,
                Err(e) => return cli_failure(true, "source_io", &format!("cannot read source: {e}")),
            };
            if bytes.len() as u64 > config.limits.source_bytes {
                return cli_failure(true, "source_limit", "source exceeds source_bytes");
            }
            run_source(&bytes, config)
        }
        "conformance" => {
            if !p.json || p.compact || p.host.is_some() || p.positional.len() != 1 {
                return cli_failure(p.json, "usage", "usage: kiroha conformance spec.md --json");
            }
            let text = match std::fs::read(&p.positional[0]) {
                Ok(b) => b,
                Err(e) => return cli_failure(true, "source_io", &format!("cannot read spec: {e}")),
            };
            let text = match String::from_utf8(text) {
                Ok(t) => t,
                Err(_) => return cli_failure(true, "harness_format", "spec is not UTF-8"),
            };
            let payload = match crate::conformance::extract_block(&text, "CONFORMANCE", "json") {
                Ok(p) => p,
                Err(e) => return cli_failure(true, "harness_format", &e),
            };
            let cases = match crate::conformance::parse_manifest(&payload) {
                Ok(c) => c,
                Err(e) => return cli_failure(true, "harness_format", &e),
            };
            let (report, all) = crate::conformance::run_all(&cases);
            Output { stdout: line(&report), stderr: String::new(), code: if all { 0 } else { 1 } }
        }
        "" => cli_failure(false, "usage", "usage: kiroha <check|run|spec|conformance|--version>"),
        other => cli_failure(false, "usage", &format!("unknown command {other}")),
    }
}

fn frontend(bytes: &[u8]) -> Result<(String, crate::ast::Program), Output> {
    let src = match crate::lexer::decode_source(bytes) {
        Ok(s) => s.to_string(),
        Err(d) => {
            let lossy = String::from_utf8_lossy(bytes).to_string();
            let env = failure_envelope(d.phase, d.error_value(), span_value(&lossy, d.span));
            return Err(Output { stdout: line(&env), stderr: String::new(), code: 1 });
        }
    };
    match crate::compile(&src) {
        Ok(p) => Ok((src, p)),
        Err(d) => {
            let env = failure_envelope(d.phase, d.error_value(), span_value(&src, d.span));
            Err(Output { stdout: line(&env), stderr: String::new(), code: 1 })
        }
    }
}

pub fn check_source(bytes: &[u8]) -> Output {
    match frontend(bytes) {
        Ok(_) => {
            let v = Value::rec(vec![("protocol", Value::str("kiroha-cli/1")), ("ok", Value::Bool(true)), ("phase", Value::str("check"))]);
            Output { stdout: line(&v), stderr: String::new(), code: 0 }
        }
        Err(o) => o,
    }
}

pub fn run_source(bytes: &[u8], config: HostConfig) -> Output {
    let started = std::time::Instant::now();
    let baseline = crate::alloc::live_bytes();
    let memory_limit = config.limits.memory_bytes;
    let deadline = config.limits.deadline_ms.as_ref().and_then(|d| d.to_u64());
    let (src, prog) = match frontend(bytes) {
        Ok(x) => x,
        Err(o) => return o,
    };
    let tokio_rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => return cli_failure(true, "configuration", &format!("cannot start runtime: {e}")),
    };
    // A raw flag set from the signal handler lets builtin work that holds
    // the single-threaded evaluator observe SIGINT/SIGTERM (§8.3, §15.1);
    // tokio's listeners below still deliver the event-loop cancellation.
    #[cfg(unix)]
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM] {
            let _ = signal_hook_registry::register(sig, crate::interrupt::request_cancel);
        }
    }
    let local = tokio::task::LocalSet::new();
    let outcome = local.block_on(&tokio_rt, async move {
        let config = Rc::new(config);
        let host = RealHost::new(config.clone());
        let signals = host.sender();
        tokio::task::spawn_local(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut int = signal(SignalKind::interrupt()).ok();
                let mut term = signal(SignalKind::terminate()).ok();
                loop {
                    tokio::select! {
                        _ = async { match int.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending::<()>().await } } => {}
                        _ = async { match term.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending::<()>().await } } => {}
                    }
                    if signals.send(HostEvent::CancelRoot).is_err() {
                        break;
                    }
                }
            }
        });
        let mut rt = Runtime::new(prog, host, (*config).clone());
        rt.start_root(None);
        rt.run().await
    });
    let (env, code) = match outcome {
        RootOutcome::Value(v) => {
            // Converting and serializing the result is still part of the run:
            // it honours the memory quota, the root deadline and SIGINT/SIGTERM.
            let at = deadline.map(|d| started + std::time::Duration::from_millis(d));
            crate::interrupt::arm(at, Some((baseline, memory_limit)));
            let wire = crate::wire::to_wire(&v);
            let env =
                Value::rec(vec![("protocol", Value::str("kiroha-cli/1")), ("ok", Value::Bool(true)), ("result", wire)]);
            let text = line(&env);
            match crate::interrupt::disarm() {
                None => return Output { stdout: text, stderr: String::new(), code: 0 },
                Some(trip) => {
                    let (reason, data) = match trip {
                        crate::interrupt::Trip::Deadline => ("deadline", Value::rec(vec![("scope", Value::str("root"))])),
                        crate::interrupt::Trip::Cancelled => ("cancelled", Value::rec(vec![("scope", Value::str("root"))])),
                        crate::interrupt::Trip::Memory => ("resource", Value::rec(vec![("resource", Value::str("memory"))])),
                    };
                    (
                        Value::rec(vec![
                            ("protocol", Value::str("kiroha-cli/1")),
                            ("ok", Value::Bool(false)),
                            ("phase", Value::str("abort")),
                            ("reason", Value::str(reason)),
                            ("data", data),
                        ]),
                        3,
                    )
                }
            }
        }
        RootOutcome::Error { error, span } => {
            let sp = span.map(|s| span_value(&src, s)).unwrap_or(Value::Null);
            (failure_envelope("runtime", error, sp), 1)
        }
        RootOutcome::Abort { reason, data } => (
            Value::rec(vec![
                ("protocol", Value::str("kiroha-cli/1")),
                ("ok", Value::Bool(false)),
                ("phase", Value::str("abort")),
                ("reason", Value::str(reason)),
                ("data", data),
            ]),
            3,
        ),
        RootOutcome::Harness(m) => return cli_failure(true, "configuration", &m),
    };
    Output { stdout: line(&env), stderr: String::new(), code }
}

/// Entry point used by the binary.
pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdin = std::io::stdin();
    let out = main_with(&args, &mut stdin);
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    out.code
}
