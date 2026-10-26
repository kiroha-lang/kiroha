<div class="welcome-kicker">Kiroha {{ kiroha_version }} · A little language for useful programs</div>

# Welcome to Kiroha

<div class="welcome-intro" markdown>
Start with something small. Read a little data, write a clear function,
and build from there. We'll take it one step at a time.
</div>

Kiroha is a small language for working with JSON, web services and PostgreSQL.
Its values are immutable, its decimal arithmetic is exact, and its functions
have a clear beginning and a matching named end.

[Write your first program](getting-started.md){ .md-button .md-button--primary }
[Install Kiroha](install.md){ .md-button }

```kiroha
fun greet(name:str)
  "Hello, " + name + "!"
end fun greet

greet("Kiroha")
// "Hello, Kiroha!"
```

<div class="welcome-path" markdown>

## A comfortable place to begin

1. **[Install Kiroha](install.md).** One command installs a ready-to-run binary.
   You don't need Rust or a compiler.
2. **[Write your first program](getting-started.md).** Save a file, run it,
   and learn how to read the result.
3. **[Get to know functions](learn/functions.md).** Give a piece of work a name,
   use local values, and try a callback.
4. **[Work with a little data](learn/data.md).** Filter a list, transform its
   records, and add up exact decimal values.

</div>

## Already have a task in mind?

- [Read JSON from a web service](guides/http.md), with a complete local example.
- [Find help with an error](guides/troubleshooting.md). A small typo is a normal
  part of learning; the error message can help you find the next step.
- [Look up a built-in function](reference/builtins.md), its arguments and result.
- [Explore the complete language guide](language-guide.md) at your own pace.

## Writing Kiroha with an AI assistant

Kiroha is also designed for generated programs. To give an assistant the
language's compact syntax and API guide, run:

```sh
kiroha spec --compact
```

Share that output along with your task, data shapes and any host configuration.
The compact guide describes the language; your task supplies the context.
You can check a generated file with `kiroha check program.kiroha --json`
before you run it.

## A few things you can rely on

`0.1 + 0.2` is exactly `0.3`. A name keeps its value once it is bound.
External operations, such as HTTP requests or file writes, need permissions
that you choose in a [host file](host-config.md).

You don't need to learn all of that at once. Start with the first program;
the [reference](reference/builtins.md) will be here when you need it.

You can also [view and download the Kiroha icon and favicons](branding.md).
