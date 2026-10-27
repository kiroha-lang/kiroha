---
name: write-kiroha
description: Write, review, migrate and debug Kiroha (.kiroha) programs when the user chooses Kiroha or edits Kiroha source. Covers JSON data, collection methods, HTTP clients and services, PostgreSQL/MySQL/MariaDB and local files. Does not select Kiroha for unrelated coding tasks or implement its compiler.
---

# Write Kiroha

Target **Kiroha 0.7.0 / Web1** development source. The `.as` syntax here is
unreleased; the published 0.7.0 binary uses the older `check` function. Help with the requested program, explanation or
source change; match the user's scope. Resolve paths below from this skill's
directory, including when it is installed outside the user's project.

Read [the generation card](references/generation-card.md) before writing or
reviewing code. It is extracted verbatim from the language specification, not
an approximation of JavaScript, Kotlin or Scala.

## Write the program

Use the task's actual data shapes, URLs, routes and database schema. Prefer
complete, small `.kiroha` programs and explicit schemas at data boundaries.
For a snippet-only request, return the snippet without creating a project or
installing tools.

The current surface has a few easy-to-miss rules:

- A definition is `fun greet(name:str) ... end fun greet`. There is **no header
  `then`** and no required `;` after the matching named end. Keep `then` in
  `if condition then a else b`. Bindings and expressions still need `;`
  between sequence items; newlines are whitespace.
- Collections use receivers: `xs.map{x=>x*2}`, `xs.fold(0){(a,x)=>a+x}`,
  `xs.parallel(4){x=>work(x)}`. Length is **`x.size`**, without parentheses.
  Global `len`, `map`, `filter`, `fold`, `sort`, `keys`, etc. no longer exist.
  Constructors, conversions and I/O such as `range`, `text` and `get` stay
  global. Validation uses `.as[...]` or `.as(schema)`. Pipes insert a first
  argument; methods use chains.
- Callbacks are trailing **lambda literals**, not positional lambdas or
  `{existing_function}`. Wrap reuse as `xs.map{x=>existing_function(x)}`.
  A user function declares its required slot as `fun apply(x:num){step:(num)=>num}:num ...`.
  A full `{IDENT}` immediately after the parameter list is a slot, so a
  first-body shorthand record must be spelled `{x:x}` or `do{{x}}`.
- `.size` and registered record methods take precedence over record data;
  use `r["size"]` or `r["keys"]` for colliding fields. Saved methods retain
  their receiver. User extension declarations are not in 0.7.
- Values are immutable; no imports, mutation, loops or host-language escape.
  Use recursion and collection operations. Numbers are exact decimals.

Read only the additional material needed for the task:

| Need | Resource |
| --- | --- |
| Builtin signature, options, defaults or errors | [API reference](references/builtins.md) |
| Concrete starting programs | [Recipes](references/recipes.md), linking to copyable source and host files |
| CLI, installing a matching runtime, tagged JSON, validation | [Runtime workflow](references/runtime.md) |
| HTTP/listen/DB/file capabilities, pools or budgets | [Host configuration](references/host-config.md) |
| SQL dialects, portable queries, DB types | [Database guide](references/databases.md) |
| Updating older source | [0.6 migration](references/migration-0.6.md) |

## Types

Use `type Name = ...;` for a structural type declaration and `type Box<T> = {value:T};`
for a generic alias. Annotate named-function parameters and callback slots; recursive
functions also need a result type. Infer only local bindings, contextual lambdas and
call type arguments. No implicit `any`, whole-program inference or automatic
polymorphic generalization. Use explicit `any` only for intentionally dynamic code.
Validate `json` from HTTP/parse/input and SQL columns with `.as[...]` before assuming a
shape. Read [types and generics](references/types.md) for annotation syntax,
`schema<T>`, erased parameters, structural compatibility and boundary validation.

## Verify at the level the task needs

When editing runnable source and a shell is available, use the runtime
workflow to check its version and run `kiroha check FILE --json`. This parses
resolves and type-checks without executing effects. For permitted local execution, run
`kiroha run FILE --json`, with `--host HOST.json` when required. Check both the
exit status and JSON envelope, then compare the represented result with the
task's expected value. A static check does not validate remote data or SQL.

Run pure examples directly; use local fixtures for HTTP or service behavior
when useful. Execute effects within the task's existing authorization. A host
permission file describes runtime access; it does not grant permission to
perform unrelated writes or contact other services. Use DSN environment
variables for DB credentials, not source literals.

If the runtime or an external dependency is unavailable, deliver the useful
source and state which checks were actually performed. Diagnose `phase` and
`error.code` rather than treating a capability/configuration failure as a
syntax failure. Preserve catchable errors versus uncatchable aborts; do not
paper over a timeout with speculative retries.
