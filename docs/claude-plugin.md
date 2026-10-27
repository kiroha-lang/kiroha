# Write Kiroha with Claude Code

The Kiroha plugin gives Claude Code the same Kiroha 0.8 language card,
references and runnable examples as the Codex plugin. Its `write-kiroha` skill
covers types, `.as` validation, SQL blocks, HTTP, files and migration from 0.7.
It does not install the Kiroha runtime or require an MCP server.

## Install

In a terminal with Claude Code installed, add the Kiroha marketplace and install
the plugin:

```sh
claude plugin marketplace add kiroha-lang/kiroha
claude plugin install kiroha@kiroha-lang
```

Alternatively, inside Claude Code run `/plugin marketplace add
kiroha-lang/kiroha`, followed by `/plugin install kiroha@kiroha-lang`.
Restart the session if needed. To inspect the installed plugin, run
`claude plugin list` and `claude plugin details kiroha@kiroha-lang`.

## Use it

Invoke the namespaced skill in Claude Code:

```text
/kiroha:write-kiroha Write a Kiroha program that validates JSON and sums prices.
```

The skill can also be selected automatically when the task is about Kiroha.
For executable checks, install the matching [Kiroha 0.8 runtime](install.md)
and provide the host capabilities needed by the program.

## Update and verify

```sh
claude plugin marketplace update kiroha-lang
claude plugin update kiroha@kiroha-lang
```

Restart Claude Code to load updates. Plugin 0.4.0 and Kiroha 0.8.0 share the
same checked language documentation with the [Codex plugin](codex-plugin.md).
The source marketplace and plugin manifests are validated by
`claude plugin validate .` and `claude plugin validate plugins/kiroha`.
See the [Claude Code plugin documentation](https://code.claude.com/docs/en/plugins)
for plugin scope and management.
