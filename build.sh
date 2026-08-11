#!/bin/sh
# Build the two binaries into dist/, and optionally install them.
#
#   treeserve  - the HTTP server and CLI. Pure Rust, one file, no runtime deps.
#   treesight  - the desktop app. One file too, but it loads the platform
#                webview at run time: WebKitGTK on Linux (so build it on the
#                distro you will run it on), WKWebView on macOS, WebView2 on
#                Windows.
#
# Neither binary reads anything from disk beyond the folder you point it at:
# stylesheets, all syntax themes, the Markdown parser and the LaTeX renderer
# are compiled in, and nothing is fetched at run time.
#
# Usage:
#   ./build.sh              server + app       -> dist/
#   ./build.sh server       server only        -> dist/treeserve
#   ./build.sh static       server, fully static (musl, no libc at all)
#   ./build.sh install      copy dist/* to ~/bin   (PREFIX=... to override)
#   ./build.sh bundle       server + app + installers (needs cargo-tauri)
set -eu

target=${1:-all}
prefix=${PREFIX:-$HOME/bin}
cd "$(dirname "$0")"

build_server() {
    echo "[build] treeserve (server + CLI)"
    cargo build --release
    mkdir -p dist
    cp target/release/treeserve dist/
}

build_app() {
    echo "[build] treesight (desktop app)"
    cargo build --release -p treesight
    mkdir -p dist
    cp target/release/treesight dist/
}

case $target in
all)
    build_server
    build_app
    ;;
server)
    build_server
    ;;
static)
    # musl + the pure-Rust highlighting engine: no libc, no C dependency, so
    # the result runs on any Linux of this architecture. Highlighting is about
    # twice as slow as the default build; output is identical.
    triple="$(uname -m)-unknown-linux-musl"
    if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
        echo "error: target $triple is not installed. Add it with:" >&2
        echo "    rustup target add $triple" >&2
        exit 1
    fi
    echo "[build] treeserve, static ($triple)"
    cargo build --release --target "$triple" --no-default-features --features pure
    mkdir -p dist
    cp "target/$triple/release/treeserve" dist/
    ;;
install)
    [ -x dist/treeserve ] || build_server
    mkdir -p "$prefix"
    for f in dist/*; do
        [ -f "$f" ] || continue
        cp "$f" "$prefix/"
        echo "[install] $prefix/$(basename "$f")"
    done
    case ":$PATH:" in
    *":$prefix:"*) ;;
    *) echo "note: $prefix is not on your PATH" ;;
    esac
    exit 0
    ;;
bundle)
    build_server
    build_app
    if ! command -v cargo-tauri >/dev/null 2>&1; then
        echo "error: cargo-tauri not found. Install it with one of:" >&2
        echo "    cargo install tauri-cli --version '^2' --locked" >&2
        echo "    cargo binstall tauri-cli" >&2
        exit 1
    fi
    echo "[build] installers"
    # Fetches the NSIS/AppImage tooling on first run; output lands in
    # target/release/bundle/.
    (cd app && cargo tauri build)
    ;;
*)
    echo "usage: $0 [all|server|static|install|bundle]" >&2
    exit 2
    ;;
esac

echo
echo "built:"
for f in dist/*; do
    [ -f "$f" ] && echo "    $f  ($(wc -c <"$f" | tr -d ' ') bytes)"
done
[ "$target" = bundle ] && echo "    target/release/bundle/ (installers)"
echo
echo "install with: $0 install    (PREFIX=$prefix)"
exit 0
