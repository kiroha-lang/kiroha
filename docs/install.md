# Install Kiroha

You can get started with a ready-to-run binary. You don't need to set up a
Rust toolchain or compile the language yourself.

Open a terminal and run these commands to install the latest release:

```sh
curl -fsSL https://github.com/kiroha-lang/kiroha/releases/latest/download/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
kiroha --version
```

If the version command prints a JSON line, you're ready to
[write your first program](getting-started.md). If your terminal can't find
`kiroha`, run the PATH command above and try again.

No Rust toolchain, C compiler or libclang is needed for prebuilt binaries.
The installer detects the OS and CPU, downloads the matching archive, verifies
its SHA-256 against the release's `SHA256SUMS`, and checks the executable before
replacing an existing installation. It installs without sudo in
`~/.local/bin`. If necessary, add the PATH line above to your shell startup file.

| Platform | CPU | Minimum runtime |
| --- | --- | --- |
| Linux, including WSL | x86_64 / ARM64 | glibc 2.35+ (e.g. Ubuntu 22.04+) |
| macOS | Intel / Apple Silicon | macOS 13+ |

For Windows, run the Linux installer inside WSL. Other targets can use a
source build where the POSIX host adapters are supported.

## Choose a version or directory

Pin both the installer and executable to a release:

```sh
curl -fsSL https://github.com/kiroha-lang/kiroha/releases/download/v0.4.0/install.sh \
  | bash -s -- --version 0.4.0
```

Choose a different directory:

```sh
curl -fsSL https://github.com/kiroha-lang/kiroha/releases/latest/download/install.sh \
  | bash -s -- --install-dir "$HOME/bin"
```

`KIROHA_VERSION` and `KIROHA_INSTALL_DIR` provide the same defaults; command-line
options take precedence. `--help` lists the options. Re-running the installer
updates the executable; failed downloads, checksums or executable checks leave
the previous installation in place. The script does not edit shell startup
files. `KIROHA_RELEASE_BASE_URL` can point to a release mirror with the same
`latest/download` and `download/vVERSION` layout.

## Build from source

Requires Rust 1.88+ (tested with 1.96), a C compiler and libclang for
`pg_query`. On Debian/Ubuntu, `sudo apt-get install build-essential libclang-dev`
provides the native build requirements. On macOS, use the Xcode command-line
tools and `brew install llvm protobuf`, then export
`LIBCLANG_PATH="$(brew --prefix llvm)/lib"` if automatic discovery fails.
Set `PROTOC_NO_VENDOR=1` and `PROTOC="$(brew --prefix protobuf)/bin/protoc"`
to use Homebrew's compiler instead of the dependency's older bundled protobuf,
whose build configuration is incompatible with CMake 4.
The audited `pg_query` 5 dependency also needs an SDK older than macOS 15.4
to avoid a conflicting `strchrnul` declaration. The Intel release runner
selects Xcode 16.2 (macOS 15.2 SDK) and targets macOS 13; source builds with
newer SDKs can select a compatible Xcode using `DEVELOPER_DIR` and `SDKROOT`.

```sh
git clone https://github.com/kiroha-lang/kiroha.git
cd kiroha
cargo install --path . --locked
kiroha --version
```

## Release verification

The release workflow runs the full PostgreSQL-backed verification suite and
builds each target on a native runner, executes `--version`, the
embedded conformance cases and an exact-decimal program, packages binaries and
the license, and publishes the release only after every target passes.
Installer tests use a local release server and cover platform selection,
version pinning, paths with spaces, failed downloads/checksums, and preservation
of an existing executable.

[Kiroha v0.3.0](https://github.com/kiroha-lang/kiroha/releases/tag/v0.3.0)
passed the [four-platform release workflow](https://github.com/kiroha-lang/kiroha/actions/runs/40269471769).
Latest and pinned installers were then executed from the public download URLs;
both installed binaries passed all 145 conformance cases. The
[publication record](completion.md#public-distribution) contains the evidence
and installed Linux binary checksum.

The 0.4.0 language revision adds explicit named-function delimiters; see the
[migration guide](migration-0.4.md) before running 0.3 source on 0.4.
[v0.4.0](https://github.com/kiroha-lang/kiroha/releases/tag/v0.4.0) passed its
[four-platform release CI](https://github.com/kiroha-lang/kiroha/actions/runs/40271474229).
Public latest and pinned installers both installed 0.4.0 successfully, and
the installed binaries passed 175/175 cases and the named-end factorial example.

To remove a default installation, delete `~/.local/bin/kiroha`.
