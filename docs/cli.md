# The `kiroha` command

Normative text: spec §15. Options may appear in any order; duplicates and
unknown options are usage errors. `SOURCE` is a file path or `-` for stdin.
`--json` is mandatory for `check`, `run` and `conformance`.

| Command | Behavior |
| --- | --- |
| `kiroha --version` | One JSON line: `{"implementation","language":"0.6.0","profile":"Web1","cli":"kiroha-cli/1","conformance":"kiroha-conformance/1"}` |
| `kiroha check SOURCE --json` | Lex, parse, resolve and type-check — no evaluation, no effects |
| `kiroha run SOURCE --json [--host HOST.json]` | Evaluate with the given (or default, deny-all) host |
| `kiroha spec --compact` | Print the embedded generation card exactly, with its final LF |
| `kiroha conformance spec.md --json` | Extract and run the embedded conformance manifest with the deterministic fixture host |

## Envelopes

Exactly one JSON line is written to stdout; logs and diagnostics go to stderr.

```text
check ok     {"protocol":"kiroha-cli/1","ok":true,"phase":"check"}
run ok       {"protocol":"kiroha-cli/1","ok":true,"result":WireValue}
failure      {"protocol":"kiroha-cli/1","ok":false,"phase":"lex|parse|check|runtime","error":{code,message,data},"span":Span|null}
abort        {"protocol":"kiroha-cli/1","ok":false,"phase":"abort","reason":"cancelled|deadline|resource","data":{...}}
cli failure  {"protocol":"kiroha-cli/1","ok":false,"phase":"cli","error":{code,...},"span":null}
```

`Span` is `{start, end, line, column}` (UTF-8 byte offsets, end exclusive,
one-based line, column in scalars). Runtime spans point at the expression
that raised; host-originated failures may have `null`.

CLI failure codes: `usage`, `configuration` (host file), `source_io`,
`source_limit`, `harness_format` (conformance manifest). Outside a JSON mode,
usage errors print to stderr and stdout stays empty.

## Exit status

| Code | Meaning |
| --- | --- |
| 0 | Success / all conformance cases passed |
| 1 | Frontend or runtime failure / a conformance case failed |
| 2 | CLI, configuration, source read or harness-format failure |
| 3 | Root abort |

`run` reads and validates the host file first, then applies its
`source_bytes` limit; `check` uses the default limit. Invalid UTF-8 source is
a `lex` failure (exit 1); an unreadable file is `source_io` (exit 2).

SIGINT/SIGTERM request a `cancelled` abort: running work is stopped, servers
stop accepting, transactions roll back, and the abort envelope is printed
(exit 3). A `serve` program runs until such a signal.

## Wire values

Results use tagged JSON so no precision, bytes or order is lost:

```text
{"t":"null"}
{"t":"bool","v":true}
{"t":"number","v":"0.3"}                 canonical decimal string
{"t":"string","v":"hello"}
{"t":"bytes","base64":"AAE="}            standard padded base64
{"t":"array","v":[WireValue,...]}
{"t":"record","v":[["key",WireValue],...]}
{"t":"opaque","kind":"function|schema|query|database|transaction|reply"}
```

Error `data` is always JSON and is printed with Kiroha's exact serializer, so
numbers in it must be parsed as exact decimals.

## Conformance report

`kiroha conformance spec.md --json` prints:

```text
{"protocol":"kiroha-conformance/1","total":N,"passed":P,"failed":F,
 "cases":[{"id":...,"status":"pass|fail","observed":{phase,result|error|reason,trace,...},"mismatch"?:...}]}
```

The manifest is validated completely before any case runs; a malformed
manifest is exit 2 with `harness_format`. Each case runs in a fresh runtime
with a virtual clock and fixture host; unmatched effects or unconsumed
fixtures fail the case.
