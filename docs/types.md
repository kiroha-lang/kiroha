# Types and simple generics

Kiroha checks types before running a program. Type inference is local:
variable bindings use their initializer, lambdas use an expected function type
or parameter annotations, and a generic call matches its arguments. Named
function parameters are annotated. The checker does not infer a whole program's
signatures or automatically generalize unannotated functions.

**`any` is explicit.** Missing information produces a type error asking for an
annotation; it never silently becomes `any`.

## Declare a structural type

```kiroha
type Point = {x:num, y:num};
p:Point = {x:1, y:2};
p.x + p.y
// 3
```

`type` declares a type name. `Point = {x:num, y:num}` is still an ordinary
value binding containing schema values; it does not declare a type usable in
annotations. Type declarations need a semicolon before another sequence item.
They may appear in the same sequence positions as bindings and are lexical,
with no forward references or recursive aliases.

A type is identified by its structure, not its alias name. A function requiring
`{x:num, y:num}` also accepts a record with those fields and additional fields.
Immutable array elements are covariant. Function parameters are contravariant
and results covariant. An optional field may be absent; it is not automatically
nullable.

The type forms are:

| Form | Meaning |
| --- | --- |
| `str`, `num`, `int`, `bool`, `null`, `json` | String, number, integer-valued number, bool, null, arbitrary JSON |
| `bytes`, `query`, `database`, `transaction`, `reply` | Opaque runtime kinds |
| `[T]` | Immutable array with elements of type `T` |
| `{x:T, y:U}` | Structural record |
| `maybe(T)` | `T` or null |
| `{x:optional(T)}` | A field that may be absent |
| `open({x:T})` | A record with additional JSON fields |
| `(T, U)=>R` | A function with two ordinary arguments and no callback slot |
| `(T){(U)=>V}=>R` | A function with an ordinary argument and a separate required callback |
| `schema<T>` | A runtime validation descriptor whose successful result has type `T` |
| `any` | Explicit dynamic escape hatch |
| `never` | No normal return, for example a function that always fails |

`int` is a subtype of `num`. Heterogeneous arrays and conditional branches
can produce an internal union in an inferred type. Write `json` for a general
JSON annotation and `maybe(T)` for nullability; there is no general union syntax.

## Functions and lambdas

```kiroha
fun greet(name:str):str
  "Hello, " + name + "!"
end fun greet

greet("Ada")
// "Hello, Ada!"
```

Name each ordinary parameter's type. A non-recursive function can omit its
result annotation and use the type of its final expression. A recursive
function needs an explicit result type, so checking its body never requires
a fixed-point inference pass.

```kiroha
fun factorial(n:int):int
  if n <= 1 then 1 else n * factorial(n - 1)
end fun factorial

factorial(5)
// 120
```

A lambda can obtain its parameter types from context:

```kiroha
increment:(num)=>num = {n => n + 1};
[1, 2, 3].map{n => increment(n)}
// [2,3,4]
```

Without that context, annotate the lambda's parameters: `{n:num => n+1}` or
`{(a:num, b:num) => a+b}`. Its result is inferred locally. A standalone
`{x=>x}` has no parameter type and asks for an annotation; it does not create
an implicit polymorphic or dynamic function.

A named callback slot is annotated separately:

```kiroha
fun twice<T>(value:T){step:(T)=>T}:T
  step(step(value))
end fun twice

twice(2){n => n * 3}
// 18
```

The trailing callback is still a literal lambda and remains separate from
ordinary arguments. For a callback type alias, give the named function an
explicit result type, as in `fun apply(x:num){step:Mapper}:num ...`.
This distinguishes the header from a first-body record expression.

## Generic types and functions

```kiroha
type Box<T> = {value:T};

fun box<T>(value:T):Box<T>
  {value:value}
end fun box

box(3).value
// 3
```

A generic declaration introduces type parameters, not runtime variables.
Aliases can use several parameters and other aliases, for example
`type Pair<A,B> = {left:A, right:B}`. Type argument counts are checked.
There are no bounds, higher-kinded parameters or recursive type aliases.

Call arguments can determine a generic function's type arguments. Supply
`identity<num>(1)` when an explicit choice is useful. The same syntax works
for method type arguments, for example `xs.fold<num>(0){(a,x)=>a+x}`.
If a type argument cannot be determined locally, write it explicitly.

Generic function bodies are checked for every permitted instantiation. With
an unconstrained `T`, returning `value:T` is valid; doing `value + 1` is not.
Type parameters are erased at runtime. To validate generic data, pass a
`schema<T>` value rather than trying to use `T` as an expression:

```kiroha
type Point = {x:num, y:num};
fun validate<T>(value:json, description:schema<T>):T
  value.as(description)
end fun validate

validate({x:1, y:2}, Point)
// {"x":1,"y":2}
```

## Validate data at the boundary

`get` and `parse` return `json`, which is distinct from `any`. Validate unknown
JSON before using a particular record or array shape:

```kiroha
type Point = {x:num, y:num};
point = parse("{\"x\":1,\"y\":2}").as[Point];
point.x + point.y
// 3
```

`.as[Point]` validates the value at runtime and gives the result type `Point`.
For a schema value computed at runtime, including a `schema<T>` parameter,
use `value.as(description)`. Named concrete types can also be descriptors for
`input`, including instantiated aliases such as `Box<num>`. Function
types, `schema<T>` types and erased type parameters are annotation types;
they cannot themselves be reified as validation descriptors.

Static structural compatibility permits extra record fields. Explicit
runtime validation with `v.as[Point]` keeps the closed-record contract
and rejects extras; use `v.as[open(Point)]` when extras should be accepted. Runtime
validation also enforces data-dependent rules beyond static types.

Kiroha 0.7 used the earlier `check` function; see the
[0.8 migration guide](migration-0.8.md). A record field named `as` remains available as
`r.as`, and indexed access can be written `r["as"][key]` when a type-shaped
index would otherwise be parsed as validation. Use `r["as"](arg)` to call a
function stored in that field, since `r.as(arg)` is validation syntax.

## Use dynamic typing explicitly

```kiroha
value:any = 1;
try value + "x" catch {error => error.code}
// "type"
```

`any` allows assignments, calls and operations on that value to pass static
checking. Runtime checks still apply. It propagates through code that uses
it, including a lambda called by an explicitly dynamic callee. It does not
turn off checking of unrelated expressions: `value:any = 1 + "x"` still has
an invalid initializer. Use `.as[...]` rather than `any` when you want to
establish a data contract.

Type errors use the `check` phase and cannot be caught by `try`. Useful codes
include `type`, `type_name`, `type_arity`, `type_annotation_required`,
`duplicate_type_parameter`, and `type_reification`. Runtime errors reached
through explicit `any`, domain failures, permissions and I/O retain the
existing runtime error/abort behavior.
