<!-- Generated from docs/migration-0.6.md; run python3 scripts/plugin.py sync. -->

# Move from 0.5 to 0.6

Kiroha 0.6 adds structural type checking and simple generics. `kiroha check`
now checks types as well as syntax and names, before any program effect.
See [Types and simple generics](https://kiroha-lang.github.io/kiroha/types/) for the full rules.

## Make type declarations explicit

| Before | Now |
| --- | --- |
| `Point = {x:num, y:num};` | `type Point = {x:num, y:num};` |
| `User = open({id:int, name:str});` | `type User = open({id:int, name:str});` |

Anonymous schema descriptions still work in `check`, `input`, `maybe`,
`optional` and `open`. An ordinary value binding does not introduce a type
name for annotations. Use `type Box<T> = {value:T};` for a generic alias.

## Annotate named function parameters

```kiroha
fun greet(name:str):str
  "Hello, " + name + "!"
end fun greet

greet("Ada")
```

All named function parameters and callback slots need types. Non-recursive
results can be inferred from the body. Recursive results must be annotated:
`fun fac(n:int):int ... end fun fac`. A callback slot can be declared as
`fun apply<T,U>(x:T){step:(T)=>U}:U ... end fun apply`.

Bindings infer the type of their initializer. Lambdas take parameter types
from their call/annotation context or explicit parameter annotations. For a
standalone lambda, change `inc={x=>x+1}` to `inc={x:num=>x+1}` or give `inc`
a function type. There is no automatic generalization or implicit `any`.

## Validate external data

`get` and `parse` produce `json`. Use `check(value, TypeName)` before reading
a particular data shape, and use `input(request, TypeName)` for requests.
SQL rows contain dynamically described column values; validate a row's
expected schema before using specific columns as numbers, strings or bytes.

A statically known wrong argument, missing field or incompatible callback
now fails in the `check` phase. For code intentionally probing dynamic runtime
behavior, use an explicit `any` annotation on the relevant value/callee.
`any` is an escape hatch, not a default for missing information.

## What carries over

Collection methods, `.size`, named function ends, the trailing callback
channel, exact decimal arithmetic and host capabilities retain their existing
runtime behavior. Function headers still omit `then`; conditional `if` keeps
`then`. Type declarations use a semicolon before the next item; a matching
`end fun name` still delimits a function definition by itself.
