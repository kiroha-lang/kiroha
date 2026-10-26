# Your first Kiroha program

Let's write a small program and run it. You only need a text editor and a
terminal. If Kiroha isn't installed yet, the [installation guide](install.md)
will help you get ready.

## 1. Check that Kiroha is ready

Open a terminal and run:

```sh
kiroha --version
```

You should see one line of JSON that includes `kiroha-rs/0.5.0` and the language
version `0.5.0`. If your shell says `command not found`, try:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Then run the version command again. [More installation help](install.md).

## 2. Save a small program

Create a file called `hello.kiroha` in your editor and paste this:

```kiroha
greeting = "Hello";
name = "Kiroha";
greeting + ", " + name + "!"
```

The first two lines give names to values. A semicolon separates one item
from the next. The last expression is the program's result.

Newlines make the program easier to read, but don't replace semicolons.
You can leave a semicolon after the last expression too.

## 3. Check and run it

In the folder where you saved the file, run:

```sh
kiroha check hello.kiroha --json
```

This checks the syntax and names without running your program. A successful
check looks like this:

```json
{"protocol":"kiroha-cli/1","ok":true,"phase":"check"}
```

Now run it:

```sh
kiroha run hello.kiroha --json
```

The result is:

```json
{"protocol":"kiroha-cli/1","ok":true,"result":{"t":"string","v":"Hello, Kiroha!"}}
```

The wrapper is there so people and other programs can read results reliably.
`ok: true` means the program succeeded. Inside `result`, `t` is the value's
kind and `v` is its value. Here, the value is the string `Hello, Kiroha!`.
The `--json` flag is required in this release.

## 4. Try a number

Replace the contents of `hello.kiroha` with:

```kiroha
0.1 + 0.2
```

Run the same command again. The result now contains:

```json
{"t":"number","v":"0.3"}
```

Kiroha uses exact decimal numbers. The wire format stores a number's spelling
as a string so large or precise numbers survive the trip through JSON.
The Kiroha value itself is a number.

## 5. Give a calculation a name

Try this in the same file:

```kiroha
fun add_tax(price)
  tax = price * 0.1;
  price + tax
end fun add_tax

add_tax(20)
// 22
```

`fun add_tax(price)` starts the function. `end fun add_tax` closes it
with the same name. Between them, `tax` is a local value, and the last
expression becomes the function's result.

You've now saved, checked and run a program, read its result, and written a
function. A good next step is [Functions, step by step](learn/functions.md),
or [Work with data](learn/data.md) if you have a list you'd like to transform.

!!! tip "If your result looks different"
    Check that the file was saved, and that the terminal is in the same folder.
    If Kiroha reports an error, [this short guide](guides/troubleshooting.md)
    explains the message and a few common fixes.
