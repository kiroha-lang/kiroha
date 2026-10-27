# Runtime workflow

The bundled card and examples target **Kiroha 0.8.0 / Web1**. This plugin
provides instructions and source, not a runtime binary or MCP tool.

## Find a matching executable

Use `kiroha --version` (or the project's executable path, such as
`target/release/kiroha`). It emits one JSON line with `implementation`,
`language`, `profile`, `cli` and `conformance`. Check `language`, not just the
package filename. Do not replace another installed version silently.

If installing a runtime is part of the task, the official installer supports
an isolated destination. Choose a task-local `INSTALL_DIR`, then use:

```sh
curl -fsSL https://github.com/kiroha-lang/kiroha/releases/download/v0.8.0/install.sh \
  | bash -s -- --version 0.8.0 --install-dir "$INSTALL_DIR"
"$INSTALL_DIR/kiroha" --version
```

`INSTALL_DIR` must be set to the chosen directory first. The installer checks
SHA-256. It supports Linux/WSL and macOS on x86_64/ARM64. If installation is
unavailable, writing or reviewing code can still use the bundled card; report
execution as unverified. For a different language version, use that runtime's
`kiroha spec --compact` and matching documentation rather than mixing APIs.

## Check and run

```sh
kiroha check program.kiroha --json
kiroha run program.kiroha --json
kiroha run program.kiroha --json --host program.host.json
```

`check` lexes, parses and resolves the entire source, including dead branches;
it does not evaluate or contact services. `run` evaluates it. All operations
return exactly one JSON envelope on stdout; logs go to stderr. Exit codes:
0 success, 1 language error, 2 CLI/configuration failure, 3 abort.

A successful check has `ok:true, phase:"check"`. A successful run has
`ok:true, result:WireValue`. Values are tagged: numbers are
`{"t":"number","v":"0.3"}` with **decimal strings**, strings are
`{"t":"string","v":"hello"}`, arrays wrap a list of tagged values, and
records wrap an ordered list of `[key, taggedValue]` pairs. Avoid converting
exact numeric strings through floating point. Non-JSON values have their
own tags; use `dump(value)` only for JSON values.

A language error includes `phase`, `error.code`, diagnostic `error.message`,
`error.data`, and a source `span`. Phases distinguish lex/parse/check/runtime.
Unresolved old globals such as `len` are check errors; old function-header
`then` is a parse error. Missing network capabilities raise permission errors
at runtime. The default host denies external effects. Deadlines and resource
limits use the separate abort envelope; `try ... catch` does not catch them.

Host files are strict JSON with `"format":"kiroha-host/1"` and closed keys.
Grant the actual task's origins, listener pairs, DB aliases or absolute file
roots. SQL credentials belong in the process environment variable selected
by `database_pools.<alias>.dsn_env`. Kiroha `env` values instead come from the
host's explicit `environment` map: these are distinct mechanisms.

## Focused diagnosis

- `syntax`: inspect named ends, separators, callback placement and grouping.
- `name`: check declaration order and removed globals. There is no hoisting.
- `arity`: distinguish ordinary arguments, required callback and callback signature.
- `validation`: use `error.data.path`, `expected`, `actual` to correct the data contract.
- `permission` or CLI configuration: inspect the host file and origin/path matching.
- `network`, `http`, `db` or `io`: retain the operation's outcome and the original error;
  fix the observed dependency problem rather than rewriting valid syntax.

Use the [API reference](builtins.md) for operation-specific errors and
[host configuration](host-config.md) for permissions and budgets.
