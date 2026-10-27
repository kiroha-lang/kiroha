# When something goes wrong

An error message is information about the next thing to fix. It doesn't mean
you need to start over. Keep the example small, read the phase and message,
and change one thing at a time.

## Read the phase first

Kiroha reports a JSON result with `ok: false` when a check or run fails.
The `phase` field tells you how far the program got:

| Phase | Where to look |
| --- | --- |
| `lex` | A character, number or string couldn't be read. |
| `parse` | The pieces don't form valid syntax yet. |
| `check` | A name is unknown or declared twice in one scope. |
| `runtime` | The program started, but an operation couldn't finish normally. |
| `abort` | A cancellation or resource/deadline limit stopped the task. |

A source diagnostic also includes a byte span and a one-based line and column.
The reported location is a useful place to start looking.

## “Command not found”

If your shell can't find `kiroha`, check the installation path:

```sh
export PATH="$HOME/.local/bin:$PATH"
kiroha --version
```

You may need to add the PATH line to your shell's startup file for future
terminals. [Installation options](../install.md) explain alternate directories.

## A function won't close

Repeat the opening name after `end fun`, with the same spelling:

```kiroha
fun double(n:num)
  n * 2
end fun double

double(3)
// 6
```

If you wrote `end fun other`, the parser will say it expected `end fun double`.
If you're using a pre-0.4 example such as `double(n)=n*2`, follow the
[migration guide](../migration-0.4.md).

## Two lines won't parse

Newlines are whitespace. Put a semicolon between sequence items:

```kiroha
x = 2;
y = 3;
x + y
// 5
```

Also check for a trailing comma: `[1,2]` is valid, while `[1,2,]` isn't.

## A name is unknown

Check the spelling and capital letters. A function can see earlier bindings
and itself, but not names defined later. Move a helper before its first use,
or define it inside the function that needs it.

Kiroha checks every branch before running. An unknown name in an unused
branch is still a check error.

## A request says `permission`

Pass the host file with `--host`. For HTTP, its allowed origin must match the
request's scheme, host and port. The [local HTTP walkthrough](http.md) shows a
complete program and matching configuration.

For other effects, the [host reference](../host-config.md) lists the permission
to grant. Pure calculations don't need a host file.

## A value has the wrong kind

Kiroha doesn't silently coerce values. For example, `1 + "2"` is a type error.
Convert deliberately when that's what you want:

```kiroha
number("2") + 1
// 3
```

Use `(value).as(schema)` near the point where data enters your program.
A validation error includes a path, expected shape and actual kind, so you
can follow it to the field that needs attention.

## Handle an ordinary runtime error

Use `try ... catch` when an expected operation might fail:

```kiroha
try number("not a number") catch {err => err.code}
// "number"
```

The handler receives an error record. `try` catches ordinary runtime errors;
it doesn't catch syntax/check failures or cancellation/resource/deadline aborts.
The [error reference](../reference/errors.md) has the full list.

## Need another pair of eyes?

When you [open an issue](https://github.com/kiroha-lang/kiroha/issues), a small
program, the command you ran, your Kiroha version and the error output are
helpful. Leave credentials and private data out of the example. A small
reproduction is enough; you don't need to solve it before asking.
