#!/usr/bin/env bash
# Install a verified, prebuilt Kiroha release. Compatible with Bash 3.2+.
set -euo pipefail

kiroha_tmp=""
kiroha_stage=""
cleanup_installer() {
    if [ -n "$kiroha_tmp" ]; then rm -rf -- "$kiroha_tmp" || :; fi
    if [ -n "$kiroha_stage" ]; then rm -f -- "$kiroha_stage" || :; fi
}
trap cleanup_installer EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

main() {
    local version="${KIROHA_VERSION:-latest}"
    local install_dir="${KIROHA_INSTALL_DIR:-${HOME:?HOME is required}/.local/bin}"
    local base="${KIROHA_RELEASE_BASE_URL:-https://github.com/kiroha-lang/kiroha/releases}"
    local os arch target asset url expected actual info tool
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version|--install-dir)
                [ "$#" -ge 2 ] || { printf 'kiroha installer: %s needs a value\n' "$1" >&2; return 1; }
                if [ "$1" = --version ]; then version="$2"; else install_dir="$2"; fi
                shift 2 ;;
            --help|-h)
                printf '%s\n' 'Usage: install.sh [--version VERSION] [--install-dir DIRECTORY]' \
                    'Defaults: latest release, ~/.local/bin' \
                    'Environment: KIROHA_VERSION, KIROHA_INSTALL_DIR, KIROHA_RELEASE_BASE_URL'
                return 0 ;;
            *) printf 'kiroha installer: unknown option: %s\n' "$1" >&2; return 1 ;;
        esac
    done
    if [ "$version" != latest ]; then
        version="v${version#v}"
        local version_pattern='^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
        [[ "$version" =~ $version_pattern ]] || { printf 'kiroha installer: invalid version\n' >&2; return 1; }
    fi
    [ -n "$install_dir" ] || { printf 'kiroha installer: install directory is empty\n' >&2; return 1; }
    for tool in curl tar awk mktemp install; do
        command -v "$tool" >/dev/null 2>&1 || { printf 'kiroha installer: %s is required\n' "$tool" >&2; return 1; }
    done
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        printf 'kiroha installer: sha256sum or shasum is required\n' >&2; return 1
    fi
    os=$(uname -s); arch=$(uname -m)
    case "$os" in Linux) os=unknown-linux-gnu ;; Darwin) os=apple-darwin ;;
        *) printf 'kiroha installer: unsupported OS: %s (use Linux/WSL or macOS)\n' "$os" >&2; return 1 ;; esac
    case "$arch" in x86_64|amd64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;;
        *) printf 'kiroha installer: unsupported CPU: %s\n' "$arch" >&2; return 1 ;; esac
    target="$arch-$os"; asset="kiroha-$target.tar.gz"
    base="${base%/}"
    if [ "$version" = latest ]; then url="$base/latest/download"; else url="$base/download/$version"; fi
    kiroha_tmp=$(mktemp -d "${TMPDIR:-/tmp}/kiroha-install.XXXXXXXX")
    printf 'Downloading Kiroha for %s...\n' "$target"
    curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 180 \
        "$url/$asset" -o "$kiroha_tmp/$asset"
    curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 60 \
        "$url/SHA256SUMS" -o "$kiroha_tmp/SHA256SUMS"
    expected=$(awk -v name="$asset" '$2 == name {print $1}' "$kiroha_tmp/SHA256SUMS")
    [[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || { printf 'kiroha installer: missing or ambiguous checksum for %s\n' "$asset" >&2; return 1; }
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum < "$kiroha_tmp/$asset");
    else actual=$(shasum -a 256 < "$kiroha_tmp/$asset"); fi
    actual="${actual%% *}"
    [ "$actual" = "$expected" ] || { printf 'kiroha installer: SHA-256 checksum mismatch\n' >&2; return 1; }
    mkdir "$kiroha_tmp/unpack"
    tar -xzf "$kiroha_tmp/$asset" -C "$kiroha_tmp/unpack" -- kiroha
    [ -f "$kiroha_tmp/unpack/kiroha" ] && [ ! -L "$kiroha_tmp/unpack/kiroha" ] || { printf 'kiroha installer: archive must contain a regular kiroha executable\n' >&2; return 1; }
    chmod 755 "$kiroha_tmp/unpack/kiroha"
    info=$("$kiroha_tmp/unpack/kiroha" --version </dev/null) || { printf 'kiroha installer: release cannot run on this system; see the source build instructions\n' >&2; return 1; }
    case "$info" in *'"implementation":"kiroha-rs/'*'"'*) ;; *) printf 'kiroha installer: invalid executable version response\n' >&2; return 1 ;; esac
    if [ "$version" != latest ]; then
        case "$info" in *"\"implementation\":\"kiroha-rs/${version#v}\""*) ;;
            *) printf 'kiroha installer: executable version does not match requested release\n' >&2; return 1 ;; esac
    fi
    mkdir -p "$install_dir"
    [ ! -d "$install_dir/kiroha" ] || { printf 'kiroha installer: destination is a directory\n' >&2; return 1; }
    kiroha_stage=$(mktemp "$install_dir/.kiroha-install.XXXXXXXX")
    install -m 755 "$kiroha_tmp/unpack/kiroha" "$kiroha_stage"
    mv -f -- "$kiroha_stage" "$install_dir/kiroha"
    kiroha_stage=""
    printf 'Installed Kiroha to %s/kiroha\n' "$install_dir"
    printf '%s\n' "$info"
    case ":${PATH:-}:" in *":$install_dir:"*) ;;
        *) printf 'Add it to your shell PATH:\n  export PATH='; printf '%q' "$install_dir"; printf ':"$PATH"\n' ;;
    esac
    cleanup_installer
    kiroha_tmp=""
    trap - EXIT INT TERM
}

main "$@"
