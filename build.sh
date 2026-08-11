#!/bin/sh
# Build both binaries into dist/.
#
#   treeserve  - the HTTP server and CLI. No system dependencies; runs anywhere.
#   treesight  - the desktop app. Needs the platform webview at build time:
#                WebKitGTK (libwebkit2gtk-4.1-dev) on Linux, WebView2 on
#                Windows, WKWebView on macOS.
#
# Both binaries are self-contained: stylesheets, syntax themes and the Markdown
# and math renderers are compiled in, and nothing is fetched at run time.
#
# Usage:
#   ./build.sh            server + app
#   ./build.sh server     server only (no webview libraries needed)
#   ./build.sh bundle     server + app + installers (needs cargo-tauri)
set -eu

target=${1:-all}
cd "$(dirname "$0")"

case $target in
all | server | bundle) ;;
*)
    echo "usage: $0 [all|server|bundle]" >&2
    exit 2
    ;;
esac

echo "[1/2] treeserve (server + CLI)"
cargo build --release

if [ "$target" != server ]; then
    echo "[2/2] treesight (desktop app)"
    cargo build --release -p treesight
fi

mkdir -p dist
cp target/release/treeserve dist/
[ "$target" = server ] || cp target/release/treesight dist/

if [ "$target" = bundle ]; then
    if ! command -v cargo-tauri >/dev/null 2>&1; then
        echo "error: cargo-tauri not found. Install it with one of:" >&2
        echo "    cargo install tauri-cli --version '^2' --locked" >&2
        echo "    cargo binstall tauri-cli" >&2
        exit 1
    fi
    echo "[3/3] installers"
    # Downloads the NSIS/AppImage tooling on first run; installers land in
    # target/release/bundle/.
    (cd app && cargo tauri build)
fi

echo
echo "built:"
echo "    dist/treeserve"
[ "$target" = server ] || echo "    dist/treesight"
[ "$target" = bundle ] && echo "    target/release/bundle/ (installers)"
exit 0
