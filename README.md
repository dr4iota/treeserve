# treeserve

A single-binary file server in the spirit of `webfsd`, but with server-side
rendering: directory browsing with a file tree sidebar, syntax-highlighted
code, rendered Markdown, inline image/video/audio/PDF preview, and dark/light
themes. Plain HTML output, zero JavaScript, no runtime dependencies.

## Build

```sh
cargo build --release        # → target/release/treeserve (~3 MB static-ish binary)
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

## Direct dependencies

`tiny_http` (sync HTTP server), `syntect` + `two-face` (highlighting),
`comrak` (Markdown), `pulldown-latex` (LaTeX → MathML). Everything else —
argument parsing, URL handling, templating, glob matching — is hand-rolled
std-only Rust.
