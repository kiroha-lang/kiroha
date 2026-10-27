# Write Kiroha with Codex

The **Kiroha** plugin gives an agent the language guidance it needs to write,
review, migrate and debug `.kiroha` programs. Its `write-kiroha` skill targets
Kiroha 0.8.0: explicit structural types, simple generics, compact named functions and collection methods.

Version 0.4 of the plugin targets Kiroha 0.8 and covers `.as` validation and SQL blocks. The same skill is also available for [Claude Code](claude-plugin.md).

It includes the canonical generation card, API and host references, a migration
guide and eight complete example programs. References travel with the plugin,
so the agent can read them without a documentation server. The plugin needs
no MCP server or connected account. Running programs uses the regular Kiroha
CLI and the task's host permissions.

## Install

With a Codex CLI that supports `codex plugin`:

```sh
codex plugin marketplace add kiroha-lang/kiroha
codex plugin add kiroha@kiroha-lang
```

This registers the repository's marketplace and installs its Kiroha plugin.
The package uses the portable layout described in the
[official OpenAI documentation](https://developers.openai.com/plugins/build/plugins).
In a desktop client supporting repo marketplaces, the same source is declared
in `.agents/plugins/marketplace.json`.

Install the [Kiroha runtime](install.md) separately when you want to execute
programs. The plugin can also help write or review a snippet without installing
an executable.

## Use it

Invoke the skill explicitly, or let Codex select it when working on Kiroha:

```text
Use $kiroha:write-kiroha to write an orders.kiroha program that groups these
orders by customer and calculates exact totals. Check and run it locally.
```

```text
Use $kiroha:write-kiroha to migrate this 0.7 program to 0.8, keeping its behavior.
```

```text
Use $kiroha:write-kiroha to build a POST /total service with input validation.
Use a local test server to verify both valid and invalid requests.
```

The agent reads the compact generation card first and loads the API, runtime,
host or migration details only when needed. It checks runnable source with
`kiroha check ... --json` and compares results when execution is part of the
task. Static checking resolves and type-checks the whole program without performing I/O.
It distinguishes syntax errors, missing permissions, dependency failures
and runtime aborts.

## Update

```sh
codex plugin marketplace upgrade kiroha-lang
codex plugin add kiroha@kiroha-lang
```

The plugin version is **0.4.0**, independently of the language version **0.8.0**.
Its examples and bundled card are checked against that language version.

## Develop the plugin

The source is [plugins/kiroha](https://github.com/kiroha-lang/kiroha/tree/main/plugins/kiroha).
For a checkout on your machine, register the checkout path instead of the GitHub
name to test local changes:

```sh
codex plugin marketplace add /absolute/path/to/kiroha
codex plugin add kiroha@kiroha-lang
```

Keep `SKILL.md` focused on decisions that help write Kiroha. Edit the canonical
specification and documentation before regenerating their bundled copies:

```sh
python3 scripts/plugin.py sync
python3 scripts/plugin.py sync --check
python3 scripts/plugin.py check --kiroha target/release/kiroha
python3 scripts/plugin.py package
```

The check/package commands use Python 3.11+ and `PyYAML==6.0.3`, also included
in the documentation requirements. Build the runtime first if needed with
`cargo build --release --locked`.

The verifier checks package metadata and contained references, compares the
bundled generation card byte for byte with the runtime, statically checks all
eight programs, and executes pure examples, HTTP/pagination fixtures, file I/O
and a live HTTP service. The database example is statically checked and documents
its expected table and write; it is not run against an arbitrary database.

The package command writes and inspects `target/plugins/kiroha-0.4.0.zip`.
The ZIP contains only the self-contained plugin, including its license. It
excludes the marketplace, repository, runtime binary and dependencies. Normal
CI checks synchronization, executes the examples and builds the package.

When Codex CLI is installed, check the actual install and discovery path too:

```sh
python3 scripts/check_plugin_codex.py
```

This uses temporary Codex state, installs the package through the marketplace,
compares every installed file, and asks the real app-server `skills/list` endpoint
for the enabled `kiroha:write-kiroha` skill. It makes no model calls and leaves
existing user configuration untouched. Codex CLI 0.163.0 was verified.
