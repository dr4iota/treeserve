# Further work

Deferred work on this repo, each item with the **trigger** that unblocks it.
Nothing here is started early: every one of them is a seam whose shape is
supposed to be decided by a downstream app that has already hit the missing
edge, and guessing at that shape first is how a public API acquires something
nobody wanted.

"Downstream" throughout means the private companion repo (working name
**telesight**) that vendors this one and fills `Vfs` + `ShellExt`; see
[architecture.md](architecture.md), *What this repo is not*.

## House rules

- One item, one commit. Subjects are a single plain sentence saying what the
  change does for the reader of the code (`git log --oneline -15` for the
  register).
- `scripts/snap.sh` before and after, always: `scripts/snap.sh /tmp/snapwork
  base`, the change, `… new`, `diff -r`. Each item below says which outcome is
  correct — **IDENTICAL** (any diff is a bug) or **REVIEWED DIFF** (exactly the
  described change and nothing else).
- `cargo test`, `cargo test -p treesight`, and
  `cargo check --no-default-features --features pure` green; no new warnings.
- Served pages stay zero-JavaScript.
- `root_id_is_local` is the only RootId grammar. Never re-derive it.
- Stop and ask rather than improvise when a change seems to need JS on a served
  page, when a snapshot diff appears that you cannot fully explain, or when an
  item's description here disagrees with what the code actually looks like.

---

## Open from the last round

**The `save_as` copy has not been exercised by hand.** The copy moved off the
dialog thread (`f723d70`); the automated side is covered, but driving a native
GTK save dialog needs click automation this environment does not have. Someone
should run the app once, Download a file, and confirm both that the saved copy
lands (with its permission bits on Unix) and that an unwritable destination
still raises the error dialog.

---

## 1. Upstream `RootOpener`

**Trigger:** downstream's remote browsing works end to end.

Downstream's own action currently reimplements `open_root` / `serve_root` for
remote ids, because `treesight` only knows how to open a path. Once that
prototype is stable, fold the seam in here so the copy disappears:

```rust
pub trait RootOpener: Send + Sync {
    fn claims(&self, id: &str) -> bool;
    /// Off the UI thread. Returns the backend and the RootId it settled on.
    fn open(&self, app: &AppHandle, id: &str) -> Result<(Arc<dyn Vfs>, String), String>;
    /// MUST be non-connecting: cached status or live-session state only.
    /// `check_roots` calls this for every entry at launch.
    fn probe(&self, id: &str) -> RootStatus;
}
// ShellExt gains: pub openers: Vec<Arc<dyn RootOpener>>
```

`open_root` then consults `openers` for a non-local id — wait page, opener on a
thread, `set_root_vfs`, handshake navigation, which is the choreography the
prototype will have proved — and `check_roots` routes remote ids to `probe`
instead of skipping them.

Design against what the prototype actually needed, not against this sketch.
Snapshot: **IDENTICAL**.

## 2. `show_term` header flag

**Trigger:** downstream has a terminal.

`Config` gains `pub show_term: bool` (default false). Set together with
`app_ui`, `head_and_header` renders one more flag — icon plus "Terminal", href
`/.ts/term`, immediately after Refresh — through the existing `flag()` helper
and a new `ICON_TERM` (a `>_` prompt drawn as paths, like every other icon
here). The shell intercepts the link; this server never serves it.

Off by default, so snapshot: **IDENTICAL**; add a page test with the bit set.

## 3. Mobile-capable `run_with`

**Trigger:** downstream starts on mobile.

`treesight::run_with` assumes a desktop: the single-instance plugin, the folder
picker as the empty-argv path, drag-and-drop, and the accelerator comments are
all desktop concepts, and `tauri-plugin-single-instance` does not exist on
mobile at all.

- `#[cfg(desktop)]` the single-instance registration and the drag-drop hook; on
  mobile skip the picker fallback, since the downstream app supplies its own
  landing root or page through its extensions.
- Confirm `treesight` compiles as a lib for `aarch64-linux-android` and
  `aarch64-apple-ios` once gated (`cargo check --target …`, which needs the NDK
  / Xcode toolchains — coordinate on SDK setup).
- Whatever else the first real mobile build trips over belongs in this item.
  Keep the diff cfg-gated so desktop output is untouched. Snapshot:
  **IDENTICAL**.

## 4. Publish `treeserve` to crates.io

**Trigger:** downstream's browsing layer is done and `Vfs` survived it
unchanged.

Decide the version (0.2.0), re-read `Vfs` / `Meta` / `Entry` / `PaneSection`
for publish-worthiness — crates.io is forever — then `cargo publish -p
treeserve` after a dry run. Downstream may then swap its submodule for a
version dependency; that is its choice, not a requirement of this item.

## 5. Small cleanups

**Trigger:** convenience. Bundle each with a neighbouring change rather than
making a commit of its own.

- `Serving.entry` is stored as a `String` and re-parsed on every re-root, with
  the parse error swallowed. Store a `tauri::Url`.
- Windows verbatim-only names (a trailing dot or space) lose their meaning
  under display-form probing and in `recent.txt`. Document it in the README as
  a known limitation if anyone ever reports it; actually fixing it means
  carrying verbatim forms end to end, which the RootId design deliberately
  traded away.
- If `clippy` is ever introduced, do it repo-wide in one commit, not piecemeal.
