# Kiroha 0.3 → 0.4: explicit named functions

> This is the historical 0.3 → 0.4 guide. For current syntax, continue with
> [0.4 → 0.5](migration-0.5.md).

Kiroha 0.4.0 makes the start and end of each named function explicit:

```kiroha
fun fac(n) then
  if n <= 1 then 1 else n * fac(n - 1)
end fun fac;
fac(5)    // 120
```

The closing name must exactly match the opening name, including case.
All of `end`, `fun`, and the name are required. Each nested function closes
with its own name. The parser rejects a missing or mismatched delimiter
before any program effects run.

In 0.3, the corresponding definition was `fac(n)=if n<=1 then 1 else n*fac(n-1)`.
Replace the bare header with `fun fac(n) then`, and append `end fun fac`
after the old body. Old bare definitions and `fun fac(n)=...` are invalid.
Calls retain their existing spelling: `fac(5)`.

A body is now a sequence, so local bindings do not require a `do` wrapper:

```kiroha
fun apply_twice(x){body} then
  y = body(x);
  body(y)
end fun apply_twice;
apply_twice(2){x => x * 3}   // 18
```

A sequence returns its last expression; an empty body or a body ending in a
binding or definition returns `null`. A trailing semicolon is permitted.
Parameters, the callback-slot name and body locals share a scope. A local
binding cannot redeclare a parameter; an explicit `do` still creates a child
scope. An old `do` body can be retained to preserve that child scope.
Named recursion, lexical capture, static resolution, callback channels and
proper tail calls keep their existing behavior.

`fun` and `end` are now reserved: rename variables, functions, parameters or
callback slots that used either word. Record keys still allow reserved words,
including `{fun:1,end:2}`, `r.fun`, and `r?.end`. Strings, comments and SQL text
are unaffected.

The choice follows the language author's preliminary experiment with named
end markers for generation. Token measurements describe source length;
they do not independently establish a generation-quality improvement.

The CLI and conformance wire formats remain `kiroha-cli/1` and
`kiroha-conformance/1`. Language metadata is now `0.4.0` / `0.4.0/Web1`.
Use the [installer](install.md) to select a release; 0.3 programs require
migration or a pinned 0.3.0 executable.
