# Functions, step by step

A function gives a small piece of work a name. You can call it again with
different values, while keeping the steps in one place.

Each example below is a complete program. Save one in `functions.kiroha`
and run `kiroha run functions.kiroha --json` when you'd like to try it.

## Start with a name

```kiroha
fun greet(name) then
  "Hello, " + name + "!"
end fun greet;

greet("Ada")
// "Hello, Ada!"
```

The parts are:

| Part | What it means |
| --- | --- |
| `fun greet(name) then` | Define `greet`, with one input called `name`. |
| `"Hello, " + name + "!"` | Calculate the result. |
| `end fun greet` | Finish the definition, repeating its name. |
| `greet("Ada")` | Call the function with a value. |

The name after `end fun` must match exactly, including capital letters.
Repeating it makes the boundary clear when you're reading or generating code.
A call always uses parentheses. Supply one ordinary input for each parameter,
in the same order as the definition.

## Keep a few local values

A body can contain several steps, separated by semicolons:

```kiroha
fun total(price, quantity) then
  subtotal = price * quantity;
  tax = subtotal * 0.1;
  subtotal + tax
end fun total;

total(12.5, 2)
// 27.5
```

`subtotal` and `tax` belong to this call. The last expression is returned.
You don't write `return`.

A name is immutable: it keeps the value you gave it. Use a new name for a new
step. Local names share a scope with the function's parameters, so choose a
different name rather than redefining a parameter.

An empty body, or one ending in a binding or another definition, returns
`null`. A trailing semicolon after the last expression keeps that expression's
value.

## Call the function from itself

A named function can call itself. Here is a small factorial:

```kiroha
fun fac(n) then
  if n <= 1 then 1 else n * fac(n - 1)
end fun fac;

fac(5)
// 120
```

Read the `if` as an expression: choose `1` when `n <= 1`, or calculate the
other branch. Only the chosen branch runs.

For long loops, a call in tail position uses bounded continuation space.
The [language guide](../language-guide.md#4-sequences-bindings-and-functions)
includes a counting example when you're ready to explore that.

## Pass a small piece of work

Functions such as `map` take a trailing callback:

```kiroha
[1, 2, 3] | map(){n => n * 2}
// [2,4,6]
```

The braces describe what to do for each item. `n` is the current item, and
`n * 2` is its result. The callback comes after the call's parentheses.

You can declare a callback slot on your own function too:

```kiroha
fun twice(value){step} then
  first = step(value);
  step(first)
end fun twice;

twice(2){n => n * 3}
// 18
```

`value` is an ordinary input. `{step}` declares a separate, required callback
slot. At the call, `{n => n * 3}` fills that slot. The function applies it twice.
A callback with two inputs uses `{(a, b) => ...}`; a callback with no inputs
uses `{() => ...}`.

## Keep definitions close to their use

A function can see earlier bindings and its own name. It can't refer to a
name defined later in the same scope. Put a helper before the function that
uses it, or define it inside that function.

```kiroha
fun welcome(name) then
  fun decorate(text) then
    "[" + text + "]"
  end fun decorate;
  decorate("Welcome, " + name)
end fun welcome;

welcome("Lin")
// "[Welcome, Lin]"
```

The inner definition has its own matching end. If a name is missing or
mismatched, Kiroha points to the syntax error before running the program.

Next, try these ideas on a list in [Work with data](data.md).
If you're updating an older program, the [0.4 migration guide](../migration-0.4.md)
shows how to replace bare function definitions.
