# Move from 0.7 to 0.8

Kiroha 0.8 adds SQL blocks and changes how programs validate values. It removes
`check` as a global function; the `kiroha check` command still checks source
without running it. Programs using the old global need a small edit.

## Validate with a type

Replace a `check` call with `.as[Type]`:

```kiroha
type Point = {x:num, y:num};
point = parse("{\"x\":1,\"y\":2}").as[Point];
point.x + point.y
// 3
```

For an array, write `value.as[[Point]]`. Validation returns the original value
unchanged or raises `validation` with the path of the first mismatch. A closed
record still rejects extra fields. Use `.as[open(Point)]` if extras are allowed.
There is no implicit conversion and no implicit `any`.

When a function receives a schema value rather than a concrete type, use
`.as(schema)`:

```kiroha
type Point = {x:num, y:num};
fun validate<T>(value:json, schema:schema<T>):T
  value.as(schema)
end fun validate
validate({x:1, y:2}, Point).x
// 1
```

Type parameters are erased; `value.as[T]` inside that generic function cannot
reify `T`. Pass `schema<T>` explicitly instead. `input(request, schema)` is
unchanged.

`.as(...)` and a type-shaped `.as[...]` are validation syntax. A record field
named `as` is still available via `record["as"]`; use `record["as"](arg)` for a
function field and `record["as"][key]` for ambiguous indexing.

## Use SQL blocks

A typed SQL block validates every returned row and returns an array:

```kiroha
type Name = {name:str};
d = db("main");
name = "Ada";
names = sql[Name](d){select ${name} as name};
names.one().name
```

The existing ``sql(d, q`…`)`` API remains available. An untyped block,
`sql(d){...}`, returns its usual `{rows, count}`. SQL holes remain bound
parameters. Typed-row validation takes place **after** SQL executes; catching a
validation error inside a transaction does not poison the transaction. See the
[database guide](guides/databases.md#sql-blocks) for options, validation and
SQL delimiter rules.
