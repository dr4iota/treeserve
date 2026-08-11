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
#   ./build.sh                    server + app  -> dist/
#   ./build.sh server             server only   -> dist/treeserve
#   ./build.sh static             server, fully static (musl, no libc at all)
#   ./build.sh windows            Windows .exe, cross-compiled -> dist/windows/
#   ./build.sh install [DIR]      copy dist/* to DIR, default ~/bin
#   ./build.sh bundle             server + app + installers (needs cargo-tauri)
set -eu

target=${1:-all}
# install destination: argument, then $PREFIX, then ~/bin.
prefix=${2:-${PREFIX:-$HOME/bin}}
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

# Cross-compiles both .exe files from Linux. zig is the whole Windows
# toolchain here — it ships a mingw-w64 sysroot, a C compiler, a linker and a
# resource compiler, so this needs no distro packages and no root. Everything
# else is a translation layer: cargo-zigbuild points cargo at zig, and two
# shims answer for the binutils names that cargo and tauri-winres still
# spell out. Installers are Windows-only; this produces bare binaries.
build_windows() {
    triple=x86_64-pc-windows-gnu
    if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
        echo "error: target $triple is not installed. Add it with:" >&2
        echo "    rustup target add $triple" >&2
        exit 1
    fi
    if ! command -v zig >/dev/null 2>&1; then
        echo "error: zig not found. Unpack a release from https://ziglang.org/download/" >&2
        echo "       and put it on your PATH; 0.15 or newer works." >&2
        exit 1
    fi
    if ! command -v cargo-zigbuild >/dev/null 2>&1; then
        echo "error: cargo-zigbuild not found. Install it with one of:" >&2
        echo "    cargo install cargo-zigbuild --locked" >&2
        echo "    cargo binstall cargo-zigbuild" >&2
        exit 1
    fi

    shims=target/zig-shims
    mkdir -p "$shims"
    # rustc shells out to dlltool to build import libraries for the
    # raw-dylib imports in windows-sys.
    cat >"$shims/dlltool" <<'EOF'
#!/bin/sh
exec zig dlltool "$@"
EOF
    # The icon, the version info and the DPI-awareness manifest go through
    # tauri-winres, which will only drive a GNU windres. zig rc does the same
    # job — including its own preprocessing against the mingw headers — so the
    # shim claims that identity when probed and translates the arguments.
    cat >"$shims/windres" <<'EOF'
#!/bin/sh
case " $* " in *" -V "*|*" --version "*) echo "GNU windres (zig rc)"; exit 0 ;; esac
in=; out=; args=""
while [ $# -gt 0 ]; do
    case $1 in
    --input|-i) in=$2; shift 2 ;;
    --input=*) in=${1#*=}; shift ;;
    --output|-o) out=$2; shift 2 ;;
    --output=*) out=${1#*=}; shift ;;
    --include-dir|-I) args="$args /i '$2'"; shift 2 ;;
    --include-dir=*) args="$args /i '${1#*=}'"; shift ;;
    -D) args="$args /d '$2'"; shift 2 ;;
    -D*) args="$args /d '${1#-D}'"; shift ;;
    *) shift ;;
    esac
done
eval exec zig rc /:output-format coff /:target x86_64 /:auto-includes gnu \
    $args /fo "'$out'" -- "'$in'"
EOF
    chmod +x "$shims/dlltool" "$shims/windres"

    echo "[build] treeserve.exe + treesight.exe ($triple)"
    RUSTFLAGS="${RUSTFLAGS:-} -C dlltool=$PWD/$shims/dlltool" \
        RC_x86_64_pc_windows_gnu="$PWD/$shims/windres" \
        cargo zigbuild --release --target "$triple" -p treeserve -p treesight
    mkdir -p dist/windows
    cp "target/$triple/release/treeserve.exe" dist/windows/
    cp "target/$triple/release/treesight.exe" dist/windows/
    # treeserve.exe is one self-contained file. treesight.exe is not: this
    # target links Microsoft's WebView2 loader as a DLL rather than the static
    # library the MSVC build gets, so that one file has to travel with it.
    # It is the copy the crate vendors, which is what we just linked against.
    loader=$(ls -t "target/$triple"/release/build/webview2-com-sys-*/out/x64/WebView2Loader.dll 2>/dev/null | head -1)
    if [ -z "$loader" ]; then
        echo "error: WebView2Loader.dll not found under target/$triple/release/build/" >&2
        exit 1
    fi
    cp "$loader" dist/windows/
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
windows)
    build_windows
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
    echo "usage: $0 [all|server|static|windows|bundle]" >&2
    echo "       $0 install [DIR]        default: $HOME/bin" >&2
    exit 2
    ;;
esac

echo
echo "built:"
# An `if` rather than a `&&` list: a skipped last entry would otherwise make
# the loop fail, and `set -e` would end the script here.
for f in dist/* dist/windows/*; do
    if [ -f "$f" ]; then
        echo "    $f  ($(wc -c <"$f" | tr -d ' ') bytes)"
    fi
done
[ "$target" = bundle ] && echo "    target/release/bundle/ (installers)"
echo
if [ "$target" = windows ]; then
    echo "copy dist/windows/ to the Windows machine; treesight.exe needs"
    echo "WebView2Loader.dll in the same folder, treeserve.exe needs nothing."
else
    echo "install with: $0 install [DIR]    (default $prefix)"
fi
exit 0
