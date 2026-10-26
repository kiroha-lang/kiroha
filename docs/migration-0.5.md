# Move from 0.4 to 0.5

Version 0.5 makes function definitions smaller and puts collection operations
next to their values. These are source changes: update programs before running
them with the new runtime.

## Compact functions

Remove `then` from function headers and omit the separator after the named end:

```kiroha
fun greet(name)
  "Hello, " + name + "!"
end fun greet

greet("Ada")
```

The old header `then` is rejected. A single semicolon after `end fun name` is
still accepted. Bindings and expressions still need separators between items.
Conditional `if condition then a else b` keeps its `then`.

Callback slots stay after the parameter list: `fun apply(x){step} ... end fun apply`.
A complete `{IDENT}` there is a slot declaration. If the first body expression
is a shorthand record, spell it `{x:x}` or put it in `do{{x}}`.

## Collection members

| Before | Now |
| --- | --- |
| `len(x)` | `x.size` |
| `map(xs){f}` | `xs.map{f}` |
| `filter(xs){f}` | `xs.filter{f}` |
| `fold(xs, initial){f}` | `xs.fold(initial){f}` |
| `each(xs){f}` | `xs.each{f}` |
| `flat(xs)`, `take(xs, n)` | `xs.flat()`, `xs.take(n)` |
| `sort(xs)`, `sort(xs){f}` | `xs.sort()`, `xs.sort{f}` |
| `group(xs){f}` | `xs.group{f}` |
| `unique(xs)`, `unique(xs){f}` | `xs.unique()`, `xs.unique{f}` |
| `sum(xs)`, `one(xs)` | `xs.sum()`, `xs.one()` |
| `parallel(xs, n){f}` | `xs.parallel(n){f}` |
| `keys(r)`, `has(r, k)`, `pick(r, ks)` | `r.keys()`, `r.has(k)`, `r.pick(ks)` |
| `merge(a, b)` | `a.merge(b)` |
| `split(s, sep)`, `replace(s, old, new)` | `s.split(sep)`, `s.replace(old, new)` |
| `join(xs, sep)` | `xs.join(sep)` |

The old global collection names no longer resolve. Constructors, conversions,
schema functions and I/O remain global, for example `range`, `text`, `check`
and `get`. Pipes still insert their value as the first explicit argument of a
function; use method chains for collection operations.

```kiroha
range(1, 6)
  .filter{n => n > 2}
  .map{n => n * 10}
  .sum()
// 120
```

`.size` works on arrays, records, strings (Unicode scalars) and Bytes (octets).
It is a property, so omit parentheses. Other no-callback methods need `()`.
You may also write `xs.map(){f}`. Saved methods keep their receiver:
`m=xs.map; m(){x=>x+1}`.

Registered members take precedence over colliding record fields. Use bracket
lookup for data named `size`, `keys`, `has`, `pick` or `merge`. Ordinary field
calls still work. Methods are chosen by receiver kind, independently of local
bindings. Diagnostics number the receiver 0 and explicit arguments from 1.

This release prepares a shared bound-call path for future extension methods;
user-defined extension declarations are not part of 0.5.
