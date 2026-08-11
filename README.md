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
| `treeserve.exe` | one file; needs the Visual C++ runtime unless you uncomment the `crt-static` line in `build.bat` |
| `treesight.exe` | one file; uses the WebView2 runtime that ships with Windows 11 and current Windows 10 |
| `treesight` (macOS) | one file; WKWebView is part of the OS. A bare executable runs, but only a `.app` bundle gets a dock icon and a proper app name |
| `treesight` (Linux) | links WebKitGTK dynamically, so build it on the distro you will run it on; there is no realistic static option |

The static build also swaps the highlighting regex engine from oniguruma (C) to
fancy-regex (Rust) with `--no-default-features --features pure`, which is what
removes the last C dependency. Output is byte-identical either way; highlighting
is roughly twice as slow and the binary about 1.4 MB larger. That same flag makes
cross-compiling `treeserve` painless, since a pure-Rust tree needs no C toolchain
for the target. `treesight` is best built natively per platform — it links that
platform's webview.

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
  native folder picker instead of guessing; cancelling quits. The chosen folder
  is remembered only to position the next picker.
- The server binds `127.0.0.1:0` (an OS-assigned port) and the window is
  pointed at it, so cookies, redirects, Range requests and relative links work
  exactly as in a browser. Because any local process could otherwise reach that
  port, the server runs with a per-run token: the window's first navigation
  exchanges it for a cookie via `/.ts/auth`, and requests without it get 403.
- **File ▸ Open Folder…** (`Ctrl/Cmd+O`) re-roots the running server, as does
  dropping a folder on the window. A second launch re-roots the existing window
  rather than starting a second copy. **View** has Back/Forward/Reload/Top of
  Tree, since a Tauri window has no browser chrome of its own.
- **Downloads** don't go through the webview's download stack, which is invisible
  on some platforms and missing on others. A `?dl=1` link is intercepted, the URL
  is resolved back to its file with the server's own traversal checks, and a
  native Save dialog copies it straight off disk — no second HTTP round trip.
  Downloads the webview starts by itself (a PDF WKWebView won't render, say)
  still get a destination in the OS download folder and a confirmation.
- Links pointing outside the served tree open in the system browser.

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
