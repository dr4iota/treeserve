# treeserve

A single-binary file server in the spirit of `webfsd`, but with server-side
rendering: directory browsing with a file tree sidebar, syntax-highlighted
code, rendered Markdown, inline image/video/audio/PDF preview, and dark/light
themes. Plain HTML output, zero JavaScript, no runtime dependencies.

## Build

```sh
./build.sh            # or build.bat on Windows → dist/treeserve, dist/treesight
./build.sh server     # just the server, no webview libraries needed
./build.sh static     # server as one fully static file (musl, no libc)
./build.sh windows    # both .exe files, cross-compiled → dist/windows/
./build.sh install ~/bin       # copy dist/* there; ~/bin is the default
./build.sh bundle     # also the installers (needs cargo-tauri, see below)
```

On Windows the destination is required rather than defaulted, so nothing can
land somewhere you did not ask for:

```bat
build.bat install C:\WinApps
```

Both scripts print what they produced and warn when the destination is not on
your `PATH`.

Two crates, two binaries:

| | |
|---|---|
| `treeserve` | the HTTP server and its CLI — ~3 MB, no system dependencies |
| `treesight` | the desktop app: the same server in a native window |

Plain `cargo build` at the root builds only `treeserve`, so the desktop app's
webview libraries (`libwebkit2gtk-4.1-dev` on Linux, WebView2 on Windows,
WKWebView on macOS) are needed only when you ask for `treesight`.

**Both binaries run fully offline.** Stylesheets, all ~30 syntax themes, the
Markdown parser and the LaTeX→MathML renderer are compiled in; a served page
references no external host, so nothing is fetched at run time — no CDN, no
webfonts, no telemetry. The only network access is at build time: `cargo`
fetching crates, and, for `build.sh bundle`, the Tauri bundler fetching its
NSIS/AppImage tooling and the WebView2 runtime (see below).

## Portable binaries

Both binaries are single files with everything compiled in — no data directory,
no config file, nothing fetched at run time. How portable each one is depends
only on what it links against:

| | portability |
|---|---|
| `treeserve`, default build | one file, links the system libc; runs on the same or newer glibc |
| `treeserve`, `./build.sh static` | one file, **no libc at all** — copy it to any Linux of the same architecture |
| `treeserve.exe`, `build.bat` | one file; needs the Visual C++ runtime unless you uncomment the `crt-static` line |
| `treeserve.exe`, `./build.sh windows` | one file; imports only OS DLLs and the Universal CRT, so there is no runtime to install |
| `treesight.exe` | uses the WebView2 runtime that ships with Windows 11 and current Windows 10. One file from `build.bat`; cross-compiled it also needs `WebView2Loader.dll` beside it |
| `treesight` (macOS) | one file; WKWebView is part of the OS. A bare executable runs, but only a `.app` bundle gets a dock icon and a proper app name |
| `treesight` (Linux) | links WebKitGTK dynamically, so build it on the distro you will run it on; there is no realistic static option |

The static build also swaps the highlighting regex engine from oniguruma (C) to
fancy-regex (Rust) with `--no-default-features --features pure`, which is what
removes the last C dependency. Output is byte-identical either way; highlighting
is roughly twice as slow and the binary about 1.4 MB larger. That same flag also
makes cross-compiling `treeserve` painless anywhere you have no C toolchain for
the target, though the Windows cross build below does not need it. `treesight`
is otherwise best built natively per platform, since it links that platform's
webview.

### Cross-compiling for Windows from Linux

`./build.sh windows` produces both `.exe` files — binaries only, no installer;
that part of Tauri's bundler is Windows-only. Two tools, neither of which needs
root or distro packages:

```sh
rustup target add x86_64-pc-windows-gnu
cargo install cargo-zigbuild --locked   # or: cargo binstall cargo-zigbuild
# plus zig 0.15+ on your PATH, from https://ziglang.org/download/
```

zig is doing all the real work: it carries a mingw-w64 sysroot, a C compiler
(so oniguruma still builds, and the result keeps the faster default engine), a
linker and a resource compiler. `cargo-zigbuild` points cargo's linker and
`cc-rs` at it. `build.sh` then writes two shims into `target/zig-shims/`,
because a couple of build scripts still reach for binutils by name: `rustc`
wants a `dlltool` for the raw-dylib imports in `windows-sys`, and `tauri-winres`
will only drive a GNU `windres` for the icon, version info and DPI-awareness
manifest — `zig dlltool` and `zig rc` do both jobs behind those names.

The gnu target links WebView2's loader as a DLL, where the MSVC build gets a
static library, so `dist/windows/treesight.exe` travels with the
`WebView2Loader.dll` next to it. `treeserve.exe` stays a single file. To get a
one-file `treesight.exe` — or an installer — build on Windows with `build.bat`.

**Copy the result to a Windows drive before running it.** Started in place, from
`\\wsl.localhost\…\dist\windows\`, Windows sees an unsigned executable arriving
from a network location: it puts up *“can’t verify who created this file”* and
waits, Defender scans the whole image over WSL's 9P transport, and the image
pages in the same way. Measured here, that is 100 seconds the first time and
2.3 s once the scan is cached; the same binary under `C:\` answers in 0.19 s, and
`treesight.exe` opens its window in 0.13 s. Which folder you *serve* costs
nothing — serving the WSL tree from a binary on `C:` is as fast as anything else,
because the server reads it through the same 9P mount either way.

```sh
mkdir -p /mnt/c/WinApps && cp dist/windows/* /mnt/c/WinApps/
```

For a macOS universal binary, build both architectures and join them:

```sh
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create -output treeserve \
    target/aarch64-apple-darwin/release/treeserve \
    target/x86_64-apple-darwin/release/treeserve
```

## Usage

```
treeserve [OPTIONS] [ROOT]

  -b, --bind ADDR        address to bind (default: 127.0.0.1)
  -p, --port PORT        port to listen on (default: 8080)
  -t, --theme MODE       default theme: auto | light | dark (default: auto)
      --no-line-numbers  line numbers off by default
      --no-sidebar       file tree sidebar off by default
      --hidden           show dotfiles
      --title NAME       site title (default: root directory name)
      --threads N        worker threads (default: 8)
      --syntax-theme NAME        highlighting theme for both modes
      --syntax-theme-light NAME  highlighting theme for light mode (default: InspiredGitHub)
      --syntax-theme-dark NAME   highlighting theme for dark mode (default: OneHalfDark)
      --list-syntax-themes       list embedded highlighting themes and exit
```

All ~30 highlighting themes (Dracula, Nord, Solarized, Catppuccin, gruvbox, …)
are embedded in the binary and selectable at run time; names are matched
case- and punctuation-insensitively (`one-half-dark` == `OneHalfDark`).

Example: `treeserve -p 9000 ~/projects/notes`

## Features

- **Server-side rendering, zero JS.** Every page is plain HTML + CSS.
  Toggles (theme, line numbers, sidebar) are links that set a cookie via
  `/.ts/set` and redirect back.
- **File tree sidebar.** Rendered on the server; directories on the current
  path are expanded, everything else is a link, so no JS is needed for
  expansion. Toggle with “Tree” in the header.
- **Drawn icons, not typed ones.** Listing rows and header controls use inline
  SVG paths that inherit the surrounding colour, so a directory's icon is
  link-coloured and a file's is muted, and the theme flag shows the state it is
  in: a sun for light, a moon for dark, half of each for following the system.
  The characters you would reach for instead — folder, picture, film, note — are
  all in the emoji planes, and a font stack without them (any DejaVu-only Linux,
  for one) draws a missing-glyph box where the icon should be.
- **Glob filter.** Each directory listing has a filter form (`*.rs`,
  `[a-c]*.md`, …) with an optional recursive mode. Patterns containing `/`
  match relative paths.
- **Syntax highlighting** via syntect (Sublime Text grammars, ~190 languages
  through two-face), with a line-number gutter that can be toggled per user
  or disabled by default with `--no-line-numbers`.
- **Markdown** rendered server-side by comrak (GFM: tables, task lists,
  strikethrough, autolinks, tag filtering, plus GitHub's footnotes, alerts
  and heading anchors). Fenced code blocks go through the same syntect
  pipeline. A “Source” link shows the highlighted raw file.
- **Math.** `$x$` and `\(x\)` inline; `$$x$$`, `\[x\]` and ```` ```math ````
  blocks for display math — the union of what GitHub and KaTeX accept. The
  LaTeX-native `\(…\)`/`\[…\]` pairs are rewritten to the dollar forms before
  parsing (CommonMark would otherwise eat the backslashes as escapes); code
  spans and fenced blocks are left alone, and an opener with no partner in the
  same paragraph stays literal text. Formulas are typeset on the server:
  pulldown-latex converts
  the LaTeX to MathML, so formulas render as text in the browser with no
  JavaScript and no webfont download. Formulas that don't parse are shown in
  red with the parser message as a tooltip. Escape a literal dollar sign as
  `\$`. Rendering quality depends on the browser having a math font
  (Cambria Math on Windows, Latin Modern/STIX/DejaVu Math on Linux,
  built into Firefox); without one, the browser's default math layout is used.
- **Dark / light / auto themes.** Highlighting is emitted as CSS classes;
  one stylesheet per theme is generated at startup, so theme switching is
  pure CSS (`prefers-color-scheme` in auto mode, cookie override otherwise).
- **Media:** images, video, audio (with HTTP Range support for seeking) and
  PDF render inline; binaries get a download page.
- **curl-friendly.** Clients that don't ask for `text/html` get raw bytes
  for files and a plain-text listing for directories:
  `curl host:8080/path/file.tar > file.tar` just works.
- **Safety:** paths are sanitized and canonicalized; symlinks pointing
  outside the served root return 403.

## Query parameters

- `?raw=1` – raw bytes (correct Content-Type, Range supported)
- `?dl=1` – force download (Content-Disposition: attachment)
- `?src=1` – highlighted source view for Markdown files
- `?q=GLOB&r=1` – filter a directory listing (recursive with `r=1`)

## Desktop app — treesight

`app/` wraps the same server in a Tauri window, for people who would rather
double-click an icon than run a command:

```sh
cargo run -p treesight -- [FOLDER]     # dev run
./build.sh bundle                      # installers → target/release/bundle/
```

- **Started with a folder** (argument, shell verb, drag onto the exe) it serves
  it right away. **Started without one** — the normal double-click case, where
  the working directory is whatever the shell happened to pick — it opens a
  native folder picker instead of guessing; cancelling quits.
- The server binds `127.0.0.1:0` (an OS-assigned port) and the window is
  pointed at it, so cookies, redirects, Range requests and relative links work
  exactly as in a browser. Because any local process could otherwise reach that
  port, the server runs with a per-run token: the window's first navigation
  exchanges it for a cookie via `/.ts/auth`, and requests without it get 403.
- **The left pane is also the folder chooser**, so choosing one looks the same on
  every platform — which the native dialogs do not, GTK offering *Other
  Locations* where Windows offers *Quick access* and no tree at all. The tree
  has the pane to itself; scroll past it for **Places** (home, desktop,
  documents, downloads, and drive letters or `/`) and **Recent** (the last 8
  roots, kept in `recent.txt` in the app's config dir), which is where
  shortcuts reached for now and then belong. Places never feed Recent: that
  list is fixed, and a shortcut copying itself into the list below it would say
  nothing new. Re-rooting also happens by dropping a folder on the window, and a
  second launch re-roots this window rather than starting a second copy.
- **One line of chrome, not two.** No menu bar — those were rarely-used menus
  holding a permanent row — and no browser-style location bar either. Back
  shares the header with the path and the flags, and the shortcuts are
  `Alt+←` / `Alt+→` (back, forward), `Ctrl+R` (reload), `Ctrl+Home` (top of
  tree) and `Ctrl+O` (folder picker). On macOS a menu is also where `Cmd+Q` and
  the clipboard shortcuts come from, so that platform needs its own menu back
  before it is worth shipping there.
- **Nothing gets a row it does not earn.** Every control — Back, Source, Raw,
  Download, the flags — spells itself out while there is room and falls back to
  an icon when there is not, and the pane narrows and then goes as the window
  does. **Open Folder…** therefore lives in the status line rather than the pane,
  that being the one line which is always there.
- **One screen, three regions.** The shell lays itself out as an app rather than
  as a long document: the header and the status line stay where they are, and
  the sidebar and the listing scroll independently, so following a deep tree
  never scrolls the file you are reading off the top. A page in a browser keeps
  scrolling like a page, which is what a browser's own chrome expects.
- **Downloads** don't go through the webview's download stack, which is invisible
  on some platforms and missing on others. A `?dl=1` link is intercepted, the URL
  is resolved back to its file with the server's own traversal checks, and a
  native Save dialog copies it straight off disk — no second HTTP round trip.
  Downloads the webview starts by itself (a PDF WKWebView won't render, say)
  still get a destination in the OS download folder and a confirmation.
- Links pointing outside the served tree open in the system browser.

### Why none of that reaches the web

Every control above either re-roots the server or names a path outside it, so a
page served to anything but this window must not offer them. Two things keep
that true:

- `Config::app_ui` decides whether they are rendered at all. It defaults to
  `false`, `treesight` is the only thing that sets it, and there is no CLI flag
  to turn it on — a flag plus `--bind 0.0.0.0` would be a remote filesystem
  browser in someone else's filesystem.
- The controls do nothing on their own. They are ordinary links to `/.ts/open`,
  `/.ts/root`, `/.ts/place` and `/.ts/back`, and **none of those is a server
  route**: `treeserve` never re-roots itself over HTTP and answers 404 for all
  four. The desktop shell recognises them in its navigation handler and cancels
  the navigation, the same way it already intercepts `?dl=1` downloads. The
  capability lives in the one client allowed to have it.

A page served by the CLI therefore carries none of it — no back button, no
Places, no Recent, no picker, and still no JavaScript. What it does share is the
header flags, which now carry a symbol alongside their words for narrow windows.
The one-screen layout it does not: the shell turns that on with a class on
`<body>`, rather than imposing it on a page in a browser. The shell also injects
a small keydown handler for the shortcuts, since it has no menu to carry
accelerators.

### Windows Explorer integration

`app/windows/hooks.nsh` is an NSIS installer hook that registers a **“Browse
with treesight”** verb for folders, folder backgrounds and drives, pointing at
`"$INSTDIR\treesight.exe" "%V"`. It is written under `SHCTX`, so a per-user
install registers in `HKCU` and a per-machine install in `HKLM`, and the
uninstaller removes it. On Windows 11 the entry appears under **Show more
options** — surfacing it in the top-level menu would need an `IExplorerCommand`
shell extension, which this doesn't ship.

### Offline installers

The WebView2 runtime ships *inside* the Windows installer
(`webviewInstallMode: offlineInstaller`), so installing needs no network at all.
That costs about 127 MB of installer size; it is already present on Windows 11
and current Windows 10, so switch to `downloadBootstrapper` in
`app/tauri.conf.json` if you would rather have a small installer and let it
fetch the runtime on first install.

### Do you need the Tauri CLI?

Not for development: `cargo run -p treesight` builds and runs the app, and this
project has no JavaScript frontend, so nothing here needs Node or pnpm. The CLI
is only for packaging (`tauri build` → NSIS/MSI, deb/AppImage, dmg) and helpers
like `tauri icon`. Install it as pure Rust, no Node involved:

```sh
cargo install tauri-cli --version "^2" --locked   # provides `cargo tauri`
cargo binstall tauri-cli                          # or: prebuilt, much faster
```

The server CLI is unaffected by all of this: it never sets a token, and its HTTP
responses are byte-identical to the pre-workspace version.

## Direct dependencies

`tiny_http` (sync HTTP server), `syntect` + `two-face` (highlighting),
`comrak` (Markdown), `pulldown-latex` (LaTeX → MathML). Everything else —
argument parsing, URL handling, templating, glob matching — is hand-rolled
std-only Rust. The `treesight` crate adds `tauri` plus its dialog, opener and
single-instance plugins.
