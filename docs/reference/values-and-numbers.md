# Values and numbers

Normative text: spec §3.

## Value kinds

| Kind | JSON? | Description |
| --- | --- | --- |
| `null` | yes | The single null value |
| `bool` | yes | `true` / `false` |
| `number` | yes | Finite arbitrary-precision decimal |
| `string` | yes | Immutable sequence of Unicode scalar values (no lone surrogates) |
| `array` | if all items are | Immutable ordered sequence of any values |
| `record` | if all values are | Immutable ordered map from distinct strings to values |
| `function` | no | Builtin, named function, or lambda closure |
| `bytes` | no | Immutable octet sequence (no indexing or equality) |
| `schema` | no | Atom or `maybe`/`optional`/`open` wrapper |
| `query` | no | SQL query literal: text segments + hole values |
| `database` | no | Pool handle from `db` |
| `transaction` | no | Task-bound, expiring handle from `tx` |
| `reply` | no | HTTP response value from `reply` |

There is no undefined value, no object identity and no mutation. Keys such as
`"__proto__"` are ordinary keys. A *JSON value* is one that recursively
contains only the first six kinds.

## Exact decimals

A nonzero number is mathematically `c × 10^e` (integer `c` not divisible by
ten). Parsing never rounds.

| Operation | Rule |
| --- | --- |
| `+ - *`, unary `-`, comparisons, `sum` | Exact |
| `a / b` | Exact rational quotient rounded to **34 significant digits**, ties to even; exact results are preserved |
| `a % b` | `a - trunc(a/b)*b` with the *exact* quotient truncated toward zero; sign of `a` |
| division by zero | `number` error, `data.reason = "division_by_zero"` (both `/` and `%`) |

There is no NaN, Infinity or negative zero. This implementation limits
coefficients to 100,000 decimal digits as a host resource quota (exceeding it
is a `resource` abort, never rounding); exponents may be arbitrarily large
within 64-bit range. "Integer" means a number with no
fractional part; there is no separate integer type.

```kiri
[0.1+0.2, 1/3, 2/3, -5%2, 5%-2, 1e100*1e-100]
// [0.3,0.3333333333333333333333333333333333,0.6666666666666666666666666666666667,-1,1,1]
```

## Canonical spelling

Used by `text`, `dump`, diagnostics and the CLI wire format. With digits of
`|c|` of length `n` and `a = e + n - 1`:

- if `-6 <= a < 21`: plain decimal notation;
- otherwise scientific: first digit, optional `.rest`, `e`, signed exponent.

No trailing fractional zeros, no `+` in exponents, `0` for zero.

| Input | Canonical |
| --- | --- |
| `1.2300` | `1.23` |
| `1e20` | `100000000000000000000` |
| `1e21` | `1e21` |
| `0.000001` | `0.000001` |
| `1e-7` | `1e-7` |
| `-0.0` | `0` |

## Strings

Length, ordering and comparison use Unicode scalar values: `len("😀")` is 1,
`"a" < "aa"` (a proper prefix sorts first), and ordering is by code point, so
`sort(["😀","","a"])` is `["","a","😀"]`. No normalization or locale rules
apply.

## JSON

`parse` accepts exactly one JSON value surrounded by optional space/tab/CR/LF.
It rejects comments, trailing commas, `NaN`, duplicate keys (after escape
decoding), lone surrogate escapes, a leading BOM and trailing material. Key
order and exact numbers are preserved.

`dump` produces canonical compact JSON: keys in insertion order, numbers in
canonical spelling, strings escaping only `"`, `\`, `\b \f \n \r \t` and other
U+0000–U+001F as lowercase `\u00hh`. Everything else (including `/`, U+2028
and non-ASCII) is emitted literally as UTF-8. `parse(dump(v)) == v` for every
JSON value.

## Equality and ordering

`==` and `!=` first check that **both whole operands** are JSON; a function or
bytes value anywhere inside is a `type` error. Then:

- numbers compare by exact value (`1 == 1.0`);
- strings by scalar sequence;
- arrays by length and position;
- records by key/value sets, ignoring order (`{a:1,b:2} == {b:2,a:1}`);
- different kinds are unequal (`1 == "1"` and `1 == true` are `false`).

`< <= > >=` accept two numbers or two strings only.

`group`, `unique` and `pages` cursor detection use the same JSON equality.
