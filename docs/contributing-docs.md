# Help improve these docs

If a step was confusing, that is useful feedback. You can
[open an issue](https://github.com/kiroha-lang/kiroha/issues) or use the edit
button on a page to suggest a change. Small fixes are welcome.

These pages aim to help someone make progress without already knowing the
language. Start with the task, explain one new idea at a time, and give a
complete example whenever possible. Describe an error alongside a way to fix it.

## Preview a change

The site is built from this repository's Markdown with MkDocs Material.
You need Python 3.12 or newer for the documentation tooling:

```sh
python3 -m venv .venv-docs
. .venv-docs/bin/activate
python -m pip install -r requirements-docs.txt
python -m mkdocs serve
```

Open the local URL printed in the terminal. To run the same build checks as CI:

```sh
python -m mkdocs build --strict
python scripts/docs/check_site.py site
python scripts/docs/check_examples.py --kiroha target/release/kiroha
```

The last command needs a built or installed Kiroha 0.4.0 executable. Pass its
path after `--kiroha`, or use `--kiroha kiroha` if it is on PATH.
The beginner examples are executed, and the HTTP walkthrough is checked
against a temporary local server.

## What gets published

A push to `main` that changes the documentation runs the
[Pages workflow](../.github/workflows/pages.yml). Pull requests build and check
the site too; deployment happens only from `main`.

The published site is [kiroha-lang.github.io/kiroha](https://kiroha-lang.github.io/kiroha/).
The language specification and example index are brought into the site from
their original repository files at build time, so they have one source of truth.
Reference pages remain ordinary Markdown that you can also read on GitHub.

Before changing a language rule, check the [specification](../spec.md).
A wording change should make the existing behavior easier to understand;
changes to behavior need an implementation and conformance tests as well.
