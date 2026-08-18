# Architecture

What this repository is, how a request becomes a page, and where a
downstream app (the SSH product, a separate private repo) is allowed to
plug in. The README is the user-facing manual; this file is for people
changing the code.

**This repo stays local-only.** It never depends on russh, tokio, or any
SSH concept. Remote filesystems arrive as another [`Vfs`](../src/vfs.rs)
implementation supplied by the embedder.

---

## Two crates, two binaries

| crate | path | binary | role |
|---|---|---|---|
| `treeserve` | repo root | `treeserve` | HTTP server + CLI. Sync worker threads, `tiny_http`. |
| `treesight` | `app/` | `treesight` | Tauri window around the same server. `publish = false`. |

`cargo build` at the root builds only `treeserve`, so webview libraries are
needed only when you ask for the app (`cargo run -p treesight`, `./build.sh`).

The CLI (`src/main.rs`) is a thin argument parser over `treeserve::spawn`.
The app (`app/src/main.rs`) is seven lines that call `treesight::run()`.
`run()` is `run_with(generate_context!(), ShellExt::default())`; a second
app supplies its own Tauri context and a [`ShellExt`](#shellext).

```
                    ┌─────────────────────────────────────────┐
  treeserve CLI     │  tiny_http workers  →  HTML / raw bytes │
  treeserve::spawn  │         ▲                               │
                    │         │ Arc<dyn Vfs>  (LocalFs here)  │
                    └─────────┼───────────────────────────────┘
                              │
  treesight window ─ loopback ┘   127.0.0.1:<os-port>
       │                          ts_token cookie
       │  on_navigation intercepts /.ts/open, /.ts/root, …
       │  ShellExt.actions run first (downstream claims)
       └─ native dialogs, Recent file, Places, Save As
```

---

## Request path

`spawn` binds `cfg.bind:cfg.port` (port `0` lets the OS pick) and starts
`cfg.threads` worker threads. Each loop is `server.recv()` then `respond`.
A panic in one request is caught; the worker stays up.

`respond` (`src/lib.rs`) is GET-only and does, in order:

1. Snapshot `cfg.root()` once for the whole request, so a re-root mid-render
   cannot paint a listing from one tree and a pane from another.
2. If `cfg.token` is set: `/.ts/auth` mints the `ts_token` cookie and
   redirects; any other request without that cookie is 403. The CLI leaves
   `token` unset, so this is a no-op there.
3. Compiled-in assets: `/.ts/app.css`, `/.ts/math.css`,
   `/.ts/syntax-{light,dark}.css`. Preference cookies: `/.ts/set`.
4. `/.ts/wait` — parking page while the shell probes a folder (`app_ui` only).
5. `resolve_in_root` — percent-decode, reject `.` / `..` / NUL, then
   `Vfs::resolve` (symlink canonicalize + confinement). Missing → 404,
   outside the root → 403.
6. Directories 301 to a trailing slash, then `page::listing_page`.
   Files: `?raw=` / `?dl=` / non-HTML / subresource (`Sec-Fetch-Dest`) go
   through `serve_raw` (Range, ETag). Otherwise `view::file_page`.

HTML pages are `Cache-Control: no-store`. Raw bodies get an ETag from
`(mtime, len)` in `Meta`. Theme, line numbers and sidebar are cookies
(`ts_theme`, `ts_ln`, `ts_sidebar`), not query strings, so a shared URL
stays shareable.

There is **no JavaScript on served pages**. Toggles are links to `/.ts/set`.
The only script in the product is the shell's `initialization_script`
(keyboard shortcuts), injected by Tauri into the webview, not into HTML.

The one piece of page state that is neither a cookie nor a link is the
narrow-window drawer (≤50rem, where the pane leaves the layout): a checkbox
that only CSS reads, `#ts-drawer:checked ~ .shell nav.tree`. The sibling
combinator is a constraint on the markup — the input must stay a *preceding
sibling* of `.shell`, which is why `head_and_header` emits it as the first
child of `<body>` while `layout` emits the closing scrim inside `.shell`.
The state is per page, so the drawer closes on every navigation; that is the
intended behaviour, not a limitation being tolerated.

---

## Modules (`treeserve`)

| file | job |
|---|---|
| `lib.rs` | `Config`, `Root`, `State`, `PaneSection` / `PaneEntry`, `spawn` / `respond`, token, Range, ETag, `root_id_is_local`, `leaf_of` |
| `vfs.rs` | `Vfs`, `VfsPath`, `LocalFs`, `Meta`, `Entry`, `ResolveError` |
| `page.rs` | Listing, tree pane, Places/Recent/sections, layout, wait/error pages, SVG icons |
| `view.rs` | File page: media, PDF, markdown, mermaid files, highlighted source |
| `md.rs` | comrak + syntect fences; mermaid → dual light/dark SVG; LaTeX → MathML |
| `hl.rs` | syntect / two-face; class-prefixed CSS generated at startup |
| `util.rs` | HTML/percent/glob, `display_path` (strips `\\?\`), media extension lists, `MAX_HIGHLIGHT_BYTES` (2 MiB) |
| `app.css` / `math.css` | compiled in with `include_str!` |

`main.rs` is CLI-only and does not sit on the request path.

Caps that keep a worker from stalling on a huge file: 2 MiB for highlight /
README / markdown body; 64 KiB for a mermaid fence (`md.rs`). Anything
bigger is offered as Raw / Download, or left as source.

---

## VFS and RootId

Every byte the renderer needs comes from `Vfs`. `LocalFs` is `std::fs`
behind that trait. Paths inside a root are `VfsPath` — slash-separated
segments, never a host `PathBuf` — so a remote backend does not have to
pretend to be Windows or Unix.

**Confinement is `resolve` only.** It canonicalizes (follows symlinks) and
refuses a target outside the served root. `read` / `read_dir` / `metadata`
are then asked about that canonical path, or about children made by joining
names `read_dir` returned (README preview, tree walk, search). Those joins
follow symlinks the way `std::fs` always did: a URL cannot *navigate* out,
but a listing may still *summarize* a link. A stricter backend may confine
every method; the renderer only depends on `resolve` for 403.

**RootId** is a scheme-aware string naming a served root:

- Local: the bare **display-form** host path — the same string `recent.txt`
  and `/.ts/root?path=` have always carried. `display_path` strips Windows
  `\\?\` prefixes so the id matches what a person sees.
- Remote (downstream): `ssh:<bookmark>:/absolute/path`. A prefix of two or
  more characters that looks like a URI scheme marks a remote id;
  a single letter before `:` is a Windows drive. `root_id_is_local` is the
  only grammar — do not re-derive it.

`Root { id, vfs }` is swapped together (`set_root` / `set_root_vfs`). A
page never gets a title from one root and a tree from another.

`root_id_at(path)` is what the tree's “serve this folder as the root” link
puts in `?path=`. For `LocalFs` that is `display_path` of the host path.

---

## treesight

The window is a webview pointed at `http://127.0.0.1:<port>`. Cookies,
redirects, Range and relative links are therefore ordinary HTTP.

Because that port is reachable by every process on the machine, the app
sets a per-run token and opens on `/.ts/auth?t=…&back=/`. Every later
re-root navigates through that handshake URL again (`Serving::entry`):
going straight to `/` can beat the first request and land on 403.

### What the server must not do

Anything that re-roots or names a path outside the served tree is the
shell's capability:

- `Config::app_ui` (default false) decides whether Places, Recent, Back,
  Refresh, Open Folder, Print, wait page, extra sections are **rendered**. Only
  treesight sets it. There is no CLI flag — a flag plus `--bind 0.0.0.0`
  would be a remote-filesystem proxy.
- The controls are ordinary links (`/.ts/open`, `/.ts/root`, `/.ts/place`,
  `/.ts/back`, `/.ts/reload`, `/.ts/print`). **None of those is a server
  route**; the CLI 404s them. The shell's `on_navigation` cancels the
  navigation and does the work.

`?dl=1` is intercepted the same way: `resolve_in_root`, then a native Save
dialog (non-blocking, main thread), and the copy itself — `Vfs::open` →
`File::create` → `io::copy` — on a `std::thread`, so a backend reading over a
network cannot freeze the window for the length of the transfer. A failure
comes back through `run_on_main_thread`. Permission bits from `Meta.mode` are
restored on Unix.

### Re-root

`open_root` canonicalizes off the UI thread (a dead mapped drive can sit
for ~20 s). The window shows `/.ts/wait` meanwhile. Success: `set_root`,
`remember_root_id`, navigate to `entry`. Failure: `RootStatus` on the pane
(missing vs unreachable — Windows `ERROR_BAD_NETPATH` is not `NotFound`).

`check_roots` probes Places and Recent **local** ids only, one thread per
path, and prunes dead Recents from `recent.txt`. Remote ids are skipped;
whoever supplied them owns their status via `set_root_status`.

Places come from the platform (home, desktop, documents, downloads, drive
letters or `/`). Recent is `recent.txt` in the app config dir, newest
first, max 8, RootId strings. Opening a Place does not write Recent.

### Navigation allowlist

The window may only stay on:

1. The loopback origin (exact match on `url.origin()`, not a string prefix).
2. `ShellExt.allowed_origins` — a scheme (`telesight:`) or an exact origin
   (`http://telesight.localhost`, which is how Windows/Android serve custom
   protocols).

Anything else opens in the system browser.

---

## ShellExt

Unstable embedder API. `run()` is the public app; a downstream binary
calls `run_with(ctx, ext)`.

| field | purpose |
|---|---|
| `actions` | Tried **before** built-in `shell_action`. Return true to claim the URL. Remote `/.ts/root?path=ssh:…` must be claimed here or the built-in will refuse it. |
| `extra_places` | Extra rows **inside** Places. |
| `extra_sections` | Whole headed lists between Places and Recent (`PaneSection`). Evaluated at server start; later updates go through `Config::set_sections`. |
| `init_script` | Replaces `SHORTCUTS` wholesale (needed so a terminal page can keep Alt+arrows). |
| `allowed_origins` | Extra origins the webview may load (plugin scheme pages). |
| `configure` | One shot at the Tauri `Builder` (plugins). Runs after dialog/opener; single-instance stays first. |

Public helpers the embedder is meant to call:

- `Serving::state` / `origin` / `entry`, `WINDOW`
- `remember_root_id(app, id)` — Recent, disk, status Ok
- `Config::set_root_vfs`, `set_sections`, `set_root_status`

`RootOpener` is **not** in this repo yet. Downstream copies the
`open_root` / `serve_root` choreography for remote ids until that seam is
upstreamed.

`PaneEntry` links are always `{action}?path={percent_encode(id)}`. Aside
links on a row are raw hrefs (e.g. a terminal URL). A heading action is
`(href, title)` drawn as a plus.

---

## Security (local product)

- Default bind `127.0.0.1`. The CLI can bind elsewhere; `app_ui` cannot be
  turned on from the CLI.
- Loopback token in the app; no token in the CLI.
- Symlink escape → 403 for **navigation**.
- No secrets in `recent.txt`. No `?password=` links.
- Offline: no CDN, no webfonts, no telemetry. Syntax themes, mermaid,
  math, CSS are compiled in.
- Downloads resolve through the same `resolve_in_root` as pages.

---

## Tests and snapshots

`cargo test` (treeserve) and `cargo test -p treesight`. After treeserve
changes, `cargo check --no-default-features --features pure` must still
compile (fancy-regex instead of oniguruma; used by the static musl build).

`scripts/snap.sh` is the HTML/HTTP A/B harness: two servers over one
fixture (CLI-shaped and `app_ui`+token). Capture before and after a
change, `diff -r` the directories. Byte-identical refactors should diff
empty; markup changes should contain only the intended delta.
`examples/snapshot_server.rs` is the fixture server.

---

## What this repo is not

SSH browsing, a terminal, bookmarks, russh, and mobile SSH UI live in a
**private companion repo** (working name **telesight**). That repo vendors
this one and fills `Vfs` + `ShellExt`. Do not add russh here. If a seam
is missing, add a *neutral* capability in this repo (another `Vfs` method,
another `ShellExt` hook) rather than an SSH type.

The CLI will not grow `treeserve user@host:`.
