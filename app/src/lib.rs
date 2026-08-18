//! Desktop shell around the treeserve HTTP server.
//!
//! The server is the same one the CLI runs, bound to an OS-assigned loopback
//! port, and the window is pointed at it. Everything the browser build does —
//! cookies for preferences, 303 redirects, Range requests, relative links —
//! therefore keeps working unchanged.
//!
//! Because a loopback port is reachable by every process on the machine, the
//! server is started with a per-run token; the window's first navigation
//! exchanges it for a cookie and every later request is checked against it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
// Both serve `picker_start_dir`, which is the desktop picker's alone.
#[cfg(desktop)]
use std::sync::mpsc;
use std::thread;
#[cfg(desktop)]
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use treeserve::{Config, RootStatus};

/// The one window's label — public so a downstream action ([`ShellExt`])
/// can find the same window the shell drives.
pub const WINDOW: &str = "main";
const WORKER_THREADS: usize = 4;
/// How many roots the Recent list keeps.
const RECENT_MAX: usize = 8;

/// Keyboard shortcuts. The window has no menu bar, and on Windows and Linux a
/// menu is what would otherwise carry accelerators, so the shell installs them
/// itself. Clicks need nothing of the sort: the page's controls are ordinary
/// links to `/.ts/…` that `on_navigation` intercepts, which is why this script
/// only ever navigates — the same thing a click does.
const SHORTCUTS: &str = r#"
addEventListener('keydown', function (e) {
  if (e.altKey && !e.ctrlKey && e.key === 'ArrowLeft') { history.back(); }
  else if (e.altKey && !e.ctrlKey && e.key === 'ArrowRight') { history.forward(); }
  else if (e.ctrlKey && (e.key === 'r' || e.key === 'R')) { location.reload(); }
  else if (e.key === 'F5' && !e.ctrlKey && !e.altKey) { location.reload(); }
  else if (e.ctrlKey && e.key === 'Home') { location.assign('/'); }
  else if (e.ctrlKey && (e.key === 'o' || e.key === 'O')) { location.assign('/.ts/open'); }
  else if (e.ctrlKey && (e.key === 'p' || e.key === 'P')) { location.assign('/.ts/print'); }
  else { return; }
  e.preventDefault();
});
"#;

/// The running server, kept in Tauri's managed state. Public so a downstream
/// action can reach the same server the shell drives —
/// `app.try_state::<Serving>()` — to re-root it (`Config::set_root_vfs`) and
/// to build URLs on its origin.
pub struct Serving {
    inner: treeserve::Serving,
    /// `http://127.0.0.1:<port>`, the only origin the window may navigate to.
    origin: String,
    /// The handshake URL the window opens with, kept so that every later
    /// navigation can start from it too. It sets the cookie and bounces to `/`,
    /// and doing it again costs one redirect — cheaper than any way of finding
    /// out whether the first one has landed yet.
    entry: String,
}

impl Serving {
    /// The server's shared state: the `Config` a root opener re-roots and
    /// feeds Places/Recent/status through.
    pub fn state(&self) -> &Arc<treeserve::State> {
        &self.inner.state
    }

    /// `http://127.0.0.1:<port>` — for building `/.ts/…` URLs to navigate to.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The token-handshake URL a fresh navigation should enter through; it
    /// sets the cookie and bounces to `/`.
    pub fn entry(&self) -> &str {
        &self.entry
    }
}

/// What a downstream app may hang on the shell. [`run`] is
/// `run_with(generate_context!(), ShellExt::default())`; a downstream build
/// supplies its own context — its own identifier, icons and windows — and its
/// extensions, and everything else here serves both apps from one source.
///
/// Unstable: this is the seam for downstream apps of this repo, not a public
/// API with compatibility promises.
#[derive(Default)]
pub struct ShellExt {
    /// Tried on every navigation before the built-in `/.ts/…` handling.
    /// Returning true claims the URL: the navigation is cancelled and the
    /// action has done whatever it does, exactly like the built-ins.
    pub actions: Vec<Box<dyn Fn(&AppHandle, &tauri::Url) -> bool + Send + Sync>>,
    /// Extra Places entries, appended after the platform's own —
    /// (label, RootId) pairs, like `Config::places`.
    pub extra_places: Vec<Box<dyn Fn(&AppHandle) -> Vec<(String, String)> + Send + Sync>>,
    /// Whole pane sections of the downstream app's own, drawn between Places
    /// and Recent. Computed once, when the server starts; for anything later —
    /// a config UI added a server — call `Config::set_sections` through
    /// [`Serving::state`] and the next page render has it.
    ///
    /// `extra_places` stays for what it is: an entry that belongs *inside* the
    /// platform's Places list rather than in a list of its own.
    #[allow(clippy::type_complexity)]
    pub extra_sections:
        Vec<Box<dyn Fn(&AppHandle) -> Vec<treeserve::PaneSection> + Send + Sync>>,
    /// Replaces the shell's keyboard-shortcut script wholesale. A downstream
    /// page can need the very keys the default script binds, so the guard
    /// belongs to whoever knows about that page.
    pub init_script: Option<String>,
    /// Origins the window may navigate to besides the local server. An entry
    /// ending in `:` with no `/` names a scheme (`telesight:`) — custom schemes
    /// have opaque origins, so the scheme is the whole identity. Anything
    /// else must equal the URL's serialized origin exactly
    /// (`http://telesight.localhost`, the form Windows and Android serve custom
    /// protocols on) — equality, not a prefix, so a lookalike host with a
    /// suffix cannot ride the allowlist. Everything not local and not listed
    /// still opens in the user's browser.
    pub allowed_origins: Vec<String>,
    /// One shot at the builder before the shell finishes it: plugins to
    /// register, mobile-specific setup.
    #[allow(clippy::type_complexity)]
    pub configure:
        Option<Box<dyn FnOnce(tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> + Send>>,
}

/// The extensions after defaults are resolved, in Tauri's managed state so
/// the navigation handler and every `start` can reach them.
struct Ext {
    actions: Vec<Box<dyn Fn(&AppHandle, &tauri::Url) -> bool + Send + Sync>>,
    extra_places: Vec<Box<dyn Fn(&AppHandle) -> Vec<(String, String)> + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    extra_sections:
        Vec<Box<dyn Fn(&AppHandle) -> Vec<treeserve::PaneSection> + Send + Sync>>,
    init_script: String,
    allowed_origins: Vec<String>,
}

struct SharedExt(Arc<Ext>);

pub fn run() {
    run_with(tauri::generate_context!(), ShellExt::default());
}

pub fn run_with(context: tauri::Context<tauri::Wry>, mut ext: ShellExt) {
    let configure = ext.configure.take();
    let ext = Arc::new(Ext {
        actions: ext.actions,
        extra_places: ext.extra_places,
        extra_sections: ext.extra_sections,
        init_script: ext.init_script.unwrap_or_else(|| SHORTCUTS.to_string()),
        allowed_origins: ext.allowed_origins,
    });
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // Must be registered first. A second launch re-roots the open window when it
    // names a directory (Explorer "open with", drag onto the exe). Desktop only:
    // a phone has no second process to fold in and no argv to read — the system
    // delivers a new intent to the activity that is already running.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(dir) = first_dir_arg(argv.into_iter().skip(1)) {
                open_root(app, dir, true);
            }
            if let Some(w) = app.get_webview_window(WINDOW) {
                let _ = w.set_focus();
            }
        }));
    }
    let mut builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    if let Some(f) = configure {
        builder = f(builder);
    }
    builder
        .setup(move |app| {
            app.manage(SharedExt(Arc::clone(&ext)));
            let handle = app.handle().clone();
            // The server and the window go up first, hidden, on a placeholder root
            // that is never seen. Creating the window is the one slow thing left in
            // a cold start — WebView2 spawning its processes, and on a first ever
            // run laying down its user-data folder — and it used to be spent after
            // the folder was settled, with nothing on screen to show for it. Now it
            // overlaps whatever settles the folder, which is either the user
            // reading a dialog or a drive making up its mind. Loading a real page
            // into it is deliberate too: the stylesheet and the layout are warm by
            // the time there is something to paint.
            if let Err(e) = start(&handle, placeholder_root(&handle)) {
                fail(&handle, &e, true);
                return Ok(());
            }
            #[cfg(desktop)]
            match first_dir_arg(std::env::args().skip(1)) {
                // Started from a shell with a path, or via a shell verb.
                Some(dir) => open_root(&handle, dir, true),
                // Started by double-click: the working directory is wherever the
                // shell happened to put us, which is never what the user meant, so
                // ask which folder to browse.
                None => ask_for_folder(handle, true),
            }
            // Nothing to ask on a phone: there is no argv, and the picker that
            // would stand in for it needs a per-directory grant this shell does
            // not request yet. The app's own storage is the one place that is
            // readable without asking anyone, so that is the root. It starts
            // empty, which is a true thing to show rather than a failure.
            #[cfg(mobile)]
            match app_storage_dir(&handle) {
                Some(dir) => open_root(&handle, dir, true),
                None => fail(&handle, "This device gave the app no storage.", true),
            }
            Ok(())
        })
        .run(context)
        .expect("error while running treesight");
}

/// First argument that names a readable directory.
#[cfg(desktop)]
fn first_dir_arg<I: Iterator<Item = String>>(args: I) -> Option<PathBuf> {
    args.filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .find_map(|p| p.canonicalize().ok().filter(|p| p.is_dir()))
}

/// Somewhere for the server to point while the picker is up, so the window can
/// be built before there is a folder to show in it. Never rendered visibly.
fn placeholder_root(app: &AppHandle) -> PathBuf {
    app.path()
        .home_dir()
        .ok()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(std::env::temp_dir)
}

/// Where to open the picker: the newest Recent that names a local path, if it
/// answers straight away.
///
/// A remembered folder can be on a drive that is not there, and handing the
/// picker one of those makes the picker do the waiting we just stopped doing.
/// Half a second on a thread we can walk away from, then the picker decides for
/// itself. The probe thread may sit there for another twenty seconds; it holds
/// nothing but a send end nobody is listening to.
///
/// A remote id is skipped rather than probed: it is not a path this picker —
/// the platform's own, browsing the local filesystem — could start in.
#[cfg(desktop)]
fn picker_start_dir(app: &AppHandle) -> Option<PathBuf> {
    let last = PathBuf::from(
        recent(app)
            .into_iter()
            .find(|id| treeserve::root_id_is_local(id))?,
    );
    let (tx, rx) = mpsc::channel();
    let probe = last.clone();
    thread::spawn(move || {
        let _ = tx.send(probe.is_dir());
    });
    match rx.recv_timeout(Duration::from_millis(500)) {
        Ok(true) => Some(last),
        _ => None,
    }
}

/// Native folder picker. Non-blocking: the blocking variant would deadlock the
/// event loop when called from `setup` or from a navigation handler.
#[cfg(desktop)]
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    let mut dialog = app.dialog().file().set_title("Choose a folder to browse");
    if let Some(last) = picker_start_dir(&app) {
        dialog = dialog.set_directory(last);
    }
    dialog.pick_folder(move |picked| match picked {
        Some(path) => match path.into_path() {
            Ok(dir) => open_root(&app, dir, true),
            Err(e) => fail(&app, &format!("Cannot use that folder: {e}"), exit_if_cancelled),
        },
        None if exit_if_cancelled => app.exit(0),
        None => {}
    });
}

/// There is no folder picker here. Choosing a directory on a phone means a
/// per-directory grant from the system picker, and this shell does not ask for
/// one yet — so say so, in the same place the desktop would have put a dialog.
///
/// `exit_if_cancelled` keeps the desktop signature so no caller has to know
/// which platform it is on; here it means "there is no page to say this over",
/// and the answer is the app's own storage rather than `exit(0)`. A window that
/// closes itself reads as a crash on a phone.
#[cfg(mobile)]
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    if !exit_if_cancelled {
        fail(&app, "Choosing a folder is not available on this device yet.", false);
        return;
    }
    match app_storage_dir(&app) {
        Some(dir) => open_root(&app, dir, true),
        None => fail(&app, "This device gave the app no storage.", true),
    }
}

/// The app's own directory: private, always there, and granted by nobody. No
/// permission prompt, no store review, and it survives everything except an
/// uninstall. Created on first use, because a root that does not exist cannot
/// be served and an empty one is the honest starting state.
#[cfg(mobile)]
fn app_storage_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Serves `dir`, resolving it off the UI thread.
///
/// `canonicalize` is a syscall with no time limit. A drive letter mapped to a host
/// that is switched off takes as long as the redirector takes to give up — twenty
/// seconds, measured — and this used to run inside `on_navigation`, which is the
/// main thread: the whole window froze, including the part of it that would have
/// said why. So the waiting happens on a thread, the window says what it is
/// opening while that goes on, and it comes back either way.
///
/// `remember` keeps it out of the Recent list, which is what the pane's Places
/// need: that list is fixed, and a Place that added itself to Recent would just
/// be duplicating a shortcut the pane already shows.
fn open_root(app: &AppHandle, dir: PathBuf, remember: bool) {
    let previous = show_opening(app, &dir);
    let app = app.clone();
    thread::spawn(move || {
        let resolved = match dir.canonicalize() {
            Ok(d) if d.is_dir() => Ok(d),
            // There, and not a folder: as good as gone for our purposes.
            Ok(_) => Err(RootStatus::Missing),
            Err(e) => Err(classify(&e)),
        };
        let back = app.clone();
        // Windows and server state are the main thread's to touch.
        let _ = app.run_on_main_thread(move || match resolved {
            Ok(dir) => serve_root(&back, dir, remember),
            Err(status) => {
                // The pane said what it knew when it was drawn; this is fresher,
                // so record it before going back to a page that will show it.
                if let Some(serving) = back.try_state::<Serving>() {
                    serving
                        .inner
                        .state
                        .cfg
                        .set_root_status(treeserve::util::display_path(&dir), status);
                }
                fail(&back, &cannot_open(&dir, status), false);
                open_failed(&back, previous);
            }
        });
    });
}

/// Why a folder did not open, in the words the pane uses for the same thing — so
/// that a drive which the list calls "not available" is not called "missing" here.
fn cannot_open(dir: &Path, status: RootStatus) -> String {
    let path = treeserve::util::display_path(dir);
    match status {
        RootStatus::Unreachable => {
            format!("{path} is not available.\n\nThe drive or share did not answer.")
        }
        _ => format!("{path} is no longer there."),
    }
}

/// Points the window at a folder that has been resolved. Main thread only.
fn serve_root(app: &AppHandle, dir: PathBuf, remember: bool) {
    if remember {
        remember_root_id(app, &treeserve::util::display_path(&dir));
    }

    if let Some(serving) = app.try_state::<Serving>() {
        serving.inner.state.cfg.set_root(dir.clone());
        // Back to the listing root through the handshake, not straight to `/`.
        // The window is built pointing at the handshake and this runs as soon as
        // a path argument has been canonicalized, which is sooner than a webview
        // starting under software rendering gets its first request out — so
        // navigating to `/` here replaced the handshake before it happened, and
        // the page it replaced it with was refused for want of the cookie the
        // handshake had not set yet. The picker hid it: a dialog is long enough
        // for the first navigation to land. The page-load hook retitles.
        if let Some(win) = app.get_webview_window(WINDOW) {
            if let Ok(url) = serving.entry.parse() {
                let _ = win.navigate(url);
            }
            // It may still be hidden: the window is built while the picker is up,
            // and this is the first moment there is a folder to put in it.
            // Idempotent for every later re-root.
            show(&win);
        }
        return;
    }

    match start(app, dir) {
        Ok(()) => {
            if let Some(win) = app.get_webview_window(WINDOW) {
                show(&win);
            }
        }
        Err(e) => fail(app, &e, true),
    }
}

/// Says what is being opened, for as long as opening it takes, and hands back the
/// page it replaced so a failed open can put it there again.
///
/// Only with a window already on screen. Before that there is nothing to put it
/// in, and a folder named on the command line is not something anybody is sitting
/// there watching fail.
fn show_opening(app: &AppHandle, dir: &Path) -> Option<tauri::Url> {
    let serving = app.try_state::<Serving>()?;
    let win = app.get_webview_window(WINDOW)?;
    if !win.is_visible().unwrap_or(false) {
        return None;
    }
    let previous = win.url().ok();
    let url = format!(
        "{}/.ts/wait?path={}",
        serving.origin,
        treeserve::util::percent_encode(&treeserve::util::display_path(dir))
    );
    if let Ok(url) = url.parse() {
        let _ = win.navigate(url);
    }
    previous
}

/// After an open that did not happen: back to the exact page that was on screen,
/// rather than to the served root — nothing was re-rooted, so nobody should lose
/// their place over it. With nothing on screen to go back to — a bad path on the
/// command line — ask for a folder instead of leaving a window that never appears.
fn open_failed(app: &AppHandle, previous: Option<tauri::Url>) {
    match (app.get_webview_window(WINDOW), previous) {
        (Some(win), Some(url)) => {
            let _ = win.navigate(url);
        }
        _ => ask_for_folder(app.clone(), true),
    }
}

fn show(win: &tauri::WebviewWindow) {
    let _ = win.show();
    let _ = win.set_focus();
}

fn start(app: &AppHandle, root: PathBuf) -> Result<(), String> {
    let ext = Arc::clone(&app.state::<SharedExt>().0);
    let token = new_token();
    let mut cfg = Config::new(root.clone());
    cfg.port = 0; // let the OS pick a free port
    cfg.threads = WORKER_THREADS;
    cfg.set_token(Some(token.clone()));
    // Turns on the page's own chooser: path bar, history buttons, Places,
    // Recent and the picker button. Only ever set here — a server reachable by
    // anything but this window has no business offering them.
    cfg.app_ui = true;
    cfg.places = places(app)
        .into_iter()
        .map(|(label, dir)| (label, treeserve::util::display_path(&dir)))
        .chain(ext.extra_places.iter().flat_map(|f| f(app)))
        .collect();
    cfg.set_recent(recent(app));
    cfg.set_sections(ext.extra_sections.iter().flat_map(|f| f(app)).collect());

    let inner = treeserve::spawn(cfg).map_err(|e| format!("Cannot start the local server: {e}"))?;
    let origin = format!("http://127.0.0.1:{}", inner.addr.port());
    // First stop is the token handshake, which sets the cookie and redirects.
    let entry = format!("{origin}/.ts/auth?t={token}&back=/");
    app.manage(Serving {
        inner,
        origin: origin.clone(),
        entry: entry.clone(),
    });

    let win = WebviewWindowBuilder::new(
        app,
        WINDOW,
        WebviewUrl::External(entry.parse().map_err(|e| format!("bad url: {e}"))?),
    )
    .title(window_title(&treeserve::util::display_path(&root)))
    .inner_size(1200.0, 850.0)
    .min_inner_size(480.0, 360.0)
    // Shown by whoever knows there is something worth showing — `open_root`,
    // once it has a folder. A window built before the picker has been answered
    // would otherwise flash a placeholder.
    .visible(false)
    .initialization_script(ext.init_script.as_str())
    // The title follows the served root, which "Open Folder…" can change, so
    // it is refreshed on every page load rather than only at window creation.
    .on_page_load({
        let app = app.clone();
        move |win, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && let Some(serving) = app.try_state::<Serving>()
            {
                let _ = win.set_title(&window_title(&serving.inner.state.cfg.root().id));
            }
        }
    })
    .on_navigation({
        let app = app.clone();
        let ext = Arc::clone(&ext);
        move |url| {
            // Downstream actions first: a URL an extension claims is handled
            // entirely by it, the way `/.ts/open` is handled below.
            if ext.actions.iter().any(|a| a(&app, url)) {
                return false;
            }
            // Origin equality, not a prefix: `http://127.0.0.1:45678` and
            // `http://127.0.0.1:4567@evil.com` both start with this origin's
            // text and are both someone else's server.
            if url.origin().ascii_serialization() == origin {
                // The page's own chrome. These paths are not routes: the server
                // never re-roots itself and would answer 404, so the whole
                // capability lives here, in the one client that may have it.
                if shell_action(&app, url) {
                    return false;
                }
                // A "Download" link: ask where to put it and copy from disk,
                // rather than leaving it to a webview download stack that is
                // invisible on some platforms and absent on others.
                if is_download_link(url) && save_as(&app, url) {
                    return false;
                }
                return true;
            }
            // Origins an extension serves itself — a plugin scheme page.
            if origin_allowed(&ext.allowed_origins, url) {
                return true;
            }
            // Links out of the served tree belong in the user's browser.
            let _ = app.opener().open_url(url.as_str(), None::<&str>);
            false
        }
    })
    // Backstop for downloads the webview starts by itself — a PDF WKWebView
    // declines to render, say. Without a destination those fail silently.
    .on_download({
        let app = app.clone();
        move |_webview, event| {
            match event {
                tauri::webview::DownloadEvent::Requested { url, destination } => {
                    if let Ok(dir) = app.path().download_dir() {
                        *destination = dir.join(download_name(&url));
                    }
                }
                tauri::webview::DownloadEvent::Finished { path, success, .. } => match (success, path)
                {
                    (true, Some(p)) => notify(&app, &format!("Saved to {}", p.display())),
                    (false, _) => fail(&app, "The download did not finish.", false),
                    _ => {}
                },
                _ => {}
            }
            true
        }
    })
    .build()
    .map_err(|e| format!("Cannot create the window: {e}"))?;

    check_roots(app);

    // Dropping a folder on the window re-roots; dropping a file opens its page.
    win.on_window_event({
        let app = app.clone();
        move |event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                && let Some(dir) = paths.iter().find(|p| p.is_dir())
            {
                open_root(&app, dir.clone(), true);
            }
        }
    });

    Ok(())
}

/// Finds out what the pane's shortcuts actually are, off the critical path.
///
/// The two lists go out unchecked — that is what makes them free — and this
/// catches up with them. One thread per path, because the entire problem is that
/// a path can take twenty seconds to answer and the other dozen should not be
/// queued behind it. Each answer is recorded as it lands, so any page rendered
/// after it greys that entry out and says why.
///
/// Recent then prunes itself: an entry that is not there is dropped from the
/// file, so the next launch does not carry it. Once, from here, when every check
/// is in — one writer, no lost updates — and only from the file. The list on
/// screen keeps it, greyed. An entry vanishing from under the pointer is worse
/// than one that says what is wrong with it.
fn check_roots(app: &AppHandle) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let state = Arc::clone(&serving.inner.state);
    let file = recent_file(app);
    let app = app.clone();
    thread::spawn(move || {
        let recent = state.cfg.recent();
        // Remote ids are skipped: probing one would mean a network handshake,
        // and this loop exists precisely because a probe can hang. Whatever
        // supplied a remote entry owns saying how it is doing.
        let ids: Vec<String> = state
            .cfg
            .places
            .iter()
            .map(|(_, id)| id.clone())
            .chain(recent.iter().cloned())
            .filter(|id| treeserve::root_id_is_local(id))
            .collect();

        let checks: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let status = match fs::metadata(Path::new(&id)) {
                        Ok(m) if m.is_dir() => RootStatus::Ok,
                        // Something is there, but not a folder any more.
                        Ok(_) => RootStatus::Missing,
                        Err(e) => classify(&e),
                    };
                    state.cfg.set_root_status(id.clone(), status);
                    (id, status)
                })
            })
            .collect();

        let answers: Vec<(String, RootStatus)> =
            checks.into_iter().filter_map(|h| h.join().ok()).collect();

        let gone: Vec<String> = answers
            .iter()
            .filter(|(id, status)| *status != RootStatus::Ok && recent.contains(id))
            .map(|(id, _)| id.clone())
            .collect();
        if let (Some(file), false) = (file, gone.is_empty()) {
            prune_recent(&file, &gone);
        }

        // The page on screen went out before these answers arrived and cannot show
        // them: it is static, and there is no script in it to change its mind. A
        // fast answer beats the first render anyway — an empty DVD drive says
        // "not ready" at once — but the twenty-second ones land long after, which
        // looked like the checks not happening at all. So the shell asks the window
        // to load itself again, which re-renders the pane; the page stays as static
        // as it was, and the one asking is the shell, as it is for the keyboard
        // shortcuts. Once, and only if an answer changed anything, since a reload
        // costs the reader their scroll position. Not while something has focus,
        // which would cost them what they had typed into it.
        if answers.iter().any(|(_, status)| *status != RootStatus::Ok) {
            let back = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(win) = back.get_webview_window(WINDOW) {
                    let _ = win.eval(
                        "var a = document.activeElement; \
                         if (!a || (a.tagName !== 'INPUT' && a.tagName !== 'TEXTAREA')) \
                         location.reload();",
                    );
                }
            });
        }
    });
}

/// What an error from looking at a path means for the pane, and for what we tell
/// anyone who clicked it.
///
/// `ErrorKind` alone gets this wrong, and did: a mapped drive whose host is off
/// answers `ERROR_BAD_NETPATH`, which std folds into `NotFound` — the same kind a
/// deleted folder gives — so `Z:` reported itself as *missing*, which says the
/// share is empty when what happened is that nobody answered. The codes that mean
/// "ask again later" are named here instead, and the kind is only the fallback.
fn classify(e: &io::Error) -> RootStatus {
    if unreachable_code(e) {
        return RootStatus::Unreachable;
    }
    match e.kind() {
        io::ErrorKind::NotFound => RootStatus::Missing,
        // A drive that is not ready, a folder we may not look into: something is
        // there, we just cannot see it. Not the same as gone.
        _ => RootStatus::Unreachable,
    }
}

/// ERROR_NOT_READY, ERROR_BAD_NETPATH, ERROR_DEV_NOT_EXIST, ERROR_UNEXP_NET_ERR,
/// ERROR_NETNAME_DELETED, ERROR_BAD_NET_NAME, ERROR_NO_NET_OR_BAD_PATH,
/// ERROR_NETWORK_UNREACHABLE.
#[cfg(windows)]
fn unreachable_code(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(21 | 53 | 55 | 59 | 64 | 67 | 1222 | 1231)
    )
}

/// ENODEV, ENETDOWN, ENETUNREACH, ETIMEDOUT, EHOSTDOWN, EHOSTUNREACH, ESTALE —
/// the same answer from an NFS or SMB mount whose server has gone.
#[cfg(not(windows))]
fn unreachable_code(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(19 | 100 | 101 | 110 | 112 | 113 | 116))
}

/// Drops paths from the Recent file, leaving everything else as it is — the file
/// may have been rewritten by `remember_root` while the checks were running.
fn prune_recent(file: &Path, gone: &[String]) {
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    // Lines written before the RootId change hold the verbatim form
    // (`\\?\C:\…`), so each line is normalized the same way the ids were
    // before comparing — otherwise a dead pre-upgrade entry never leaves.
    let kept: String = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            let norm = treeserve::util::display_path(Path::new(l));
            !l.is_empty() && !gone.iter().any(|g| *g == norm)
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let _ = fs::write(file, kept);
}

/// `?dl=1`, the query the server's "Download" links carry.
fn is_download_link(url: &tauri::Url) -> bool {
    url.query_pairs().any(|(k, v)| k == "dl" && v == "1")
}

/// Last path segment of a URL, for naming a saved file.
fn download_name(url: &tauri::Url) -> String {
    url.path_segments()
        .and_then(|mut s| s.next_back().filter(|s| !s.is_empty()))
        .map(percent_decode)
        .unwrap_or_else(|| "download".to_string())
}

/// Saves the file behind a download link through a native Save dialog.
///
/// The URL is resolved back to its path with the server's own checks, so the
/// bytes are copied straight from the served tree — no second HTTP round trip,
/// and nothing outside the root can be reached. Returns false when the link
/// does not name a file, leaving the navigation to proceed as before.
fn save_as(app: &AppHandle, url: &tauri::Url) -> bool {
    let Some(serving) = app.try_state::<Serving>() else {
        return false;
    };
    let root = serving.inner.state.cfg.root();
    let Ok(target) = treeserve::resolve_in_root(root.vfs.as_ref(), url.path()) else {
        return false;
    };
    if !root.vfs.metadata(&target.path).map(|m| m.is_file).unwrap_or(false) {
        return false;
    }

    let app = app.clone();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Save file")
        .set_file_name(target.rel.last().cloned().unwrap_or_default());
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let vfs = Arc::clone(&root.vfs);
    let src = target.path;
    dialog.save_file(move |dest| {
        let Some(dest) = dest.and_then(|d| d.into_path().ok()) else {
            return; // cancelled
        };
        // On a thread, because the copy is as slow as the backend is far away.
        // A local file arrives at disk speed and the dialog's own callback
        // could carry it; a backend reading over a network takes as long as
        // the file is big, and this callback runs where a click is answered —
        // the window would stop repainting for the length of the transfer,
        // including the part of it that would have said what was going on.
        thread::spawn(move || {
            // Streamed through the backend rather than `fs::copy`, which only a
            // local path could satisfy. `fs::copy` also carried the permission
            // bits, so a downloaded script stayed runnable — restore them from
            // the backend's metadata where it knows them.
            let mode = vfs.metadata(&src).ok().and_then(|m| m.mode);
            let copied = vfs.open(&src).and_then(|mut from| {
                fs::File::create(&dest).and_then(|mut to| io::copy(&mut from, &mut to))
            });
            match copied {
                Err(e) => {
                    let msg = format!("Could not save {}: {e}", dest.display());
                    let back = app.clone();
                    let _ = app.run_on_main_thread(move || fail(&back, &msg, false));
                }
                Ok(_) => {
                    #[cfg(unix)]
                    if let Some(mode) = mode {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(mode));
                    }
                    #[cfg(not(unix))]
                    let _ = mode;
                }
            }
        });
    });
    true
}

/// Minimal percent-decoding for a single URL path segment.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1).copied(), bytes.get(i + 2).copied()) {
            (b'%', Some(h), Some(l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push(hex(h).unwrap() * 16 + hex(l).unwrap());
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Handles the page's own controls, which are links rather than script: the
/// back button, the picker button, and the two lists that re-root. Returns
/// whether the navigation was one of ours and should therefore be cancelled.
fn shell_action(app: &AppHandle, url: &tauri::Url) -> bool {
    match url.path() {
        "/.ts/open" => ask_for_folder(app.clone(), false),
        "/.ts/back" => eval(app, "history.back()"),
        // The Refresh flag. A link to the page it is on would have done the same
        // work on the server and cost the reader their place in it: a navigation
        // starts at the top of the document and leaves the old page behind in the
        // history. A reload keeps the scroll and keeps Back meaning the folder
        // before this one.
        "/.ts/reload" => eval(app, "location.reload()"),
        // The Print flag, and Ctrl+P, which in this window is not the engine's to
        // answer: there is no menu here that would have carried it. The page's
        // print rules are what makes the result worth looking at — no header, no
        // status line, no pane, and the light palette whatever is on screen.
        "/.ts/print" => eval(app, "window.print()"),
        // Recent; a Place is the same but not worth remembering, since the pane
        // already lists it. Both carry a path we rendered ourselves, though
        // `open_root` still checks it — a remembered folder can go away.
        "/.ts/root" | "/.ts/place" => match url.query_pairs().find(|(k, _)| k == "path") {
            // A remote id reaching this arm means no extension action claimed
            // it. Coercing it into a PathBuf would "open" a folder named
            // `ssh:…`, fail, and grey a healthy entry with a status nothing
            // ever corrects — so say what actually happened instead.
            Some((_, path)) if !treeserve::root_id_is_local(path.trim()) => {
                fail(app, &format!("Nothing here can open {}.", path.trim()), false);
            }
            Some((_, path)) => {
                let dir = PathBuf::from(path.trim());
                open_root(app, dir, url.path() == "/.ts/root");
            }
            None => fail(app, "No folder in that link.", false),
        },
        _ => return false,
    }
    true
}

fn eval(app: &AppHandle, js: &str) {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.eval(js);
    }
}

/// Fixed shortcuts for the pane's Places list.
///
/// The shell resolves these because it is the side that knows the platform:
/// where the home directory is, and what stands in for "everything else" —
/// drive roots on Windows, `/` elsewhere. That last entry is the one the GTK
/// picker calls "Other Locations" and the Windows picker calls "This PC";
/// having our own means it is in the same place on both.
fn places(app: &AppHandle) -> Vec<(String, PathBuf)> {
    let p = app.path();
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    // First on a phone, because it is the only entry that is certainly readable:
    // everything below it resolves to a path the system may still refuse.
    #[cfg(mobile)]
    if let Some(dir) = app_storage_dir(app) {
        out.push(("App storage".to_string(), dir));
    }

    let mut named: Vec<(&str, Option<PathBuf>)> = vec![("Home", p.home_dir().ok())];
    // No desktop on a device with no desktop — `desktop_dir` is not among the
    // directories the mobile resolver answers for at all.
    #[cfg(desktop)]
    named.push(("Desktop", p.desktop_dir().ok()));
    named.push(("Documents", p.document_dir().ok()));
    named.push(("Downloads", p.download_dir().ok()));

    out.extend(named.into_iter().filter_map(|(label, dir)| {
        let dir = dir?;
        // Desktop drops what is not there; offering a folder that does not exist
        // helps nobody. Mobile keeps it: under scoped storage `document_dir()`
        // names a real place the app may not stat, so probing would hide exactly
        // the entries a grant is meant to open. Listed, and answered for when it
        // is clicked — the same trade the drive letters take below.
        #[cfg(desktop)]
        if !dir.is_dir() {
            return None;
        }
        Some((label.to_string(), dir))
    }));

    // Which letters exist, asked of the system rather than of the drives. The
    // obvious loop — `is_dir()` on A:\ through Z:\ — puts a `GetFileAttributesW`
    // on each of the 26, and a letter that exists but is not ready does not fail,
    // it *waits*: a mapped drive whose server is gone waits out the SMB
    // redirector timeout, and a spun-down external disk waits for the platters.
    // Measured on a machine with one disconnected mapping, `Z:` to a NAS that was
    // off: 21.1 seconds for the 26 probes. All of it fell between choosing a
    // folder and this window existing, because that is where `start` runs, and
    // only on the first open — every later one re-roots a window that is already
    // up. `GetLogicalDrives` is a bitmask out of the object namespace and touches
    // no device, so it cannot stall.
    //
    // The trade is that a letter which is present but unreachable is now listed.
    // That is the better half of it: the old probe paid for the discovery every
    // time and then hid the drive, where this pays nothing and answers when the
    // drive is actually asked for — which is also what Explorer does.
    #[cfg(windows)]
    {
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        for letter in b'A'..=b'Z' {
            if mask & (1 << (letter - b'A')) != 0 {
                let c = letter as char;
                out.push((format!("{c}:"), PathBuf::from(format!("{c}:\\"))));
            }
        }
    }
    // Not on a phone: `/` is readable enough to list and holds nothing a user
    // put there, so it offers a tour of the OS in place of their own files.
    #[cfg(all(not(windows), desktop))]
    out.push(("Filesystem".to_string(), PathBuf::from("/")));

    out
}

fn window_title(root_id: &str) -> String {
    match treeserve::leaf_of(root_id) {
        Some(name) => format!("{name} — treesight"),
        // A drive or a share: no last component, so say which one it is.
        None => format!("{root_id} — treesight"),
    }
}

/// Whether an extension declared this URL's origin. See
/// [`ShellExt::allowed_origins`] for the two entry shapes.
fn origin_allowed(allowed: &[String], url: &tauri::Url) -> bool {
    allowed.iter().any(|a| match a.strip_suffix(':') {
        Some(scheme) if !a.contains('/') => url.scheme() == scheme,
        _ => url.origin().ascii_serialization() == *a,
    })
}

/// Says something happened, for actions with no visible result of their own.
fn notify(app: &AppHandle, msg: &str) {
    app.dialog()
        .message(msg)
        .kind(MessageDialogKind::Info)
        .title("treesight")
        .show(|_| {});
}

/// Reports a problem in a native dialog, since a GUI build has nowhere to print.
fn fail(app: &AppHandle, msg: &str, fatal: bool) {
    let app = app.clone();
    let msg = msg.to_string();
    app.dialog()
        .message(msg)
        .kind(MessageDialogKind::Error)
        .title("treesight")
        .show(move |_| {
            if fatal {
                app.exit(1);
            }
        });
}

/// Per-run secret gating the loopback server.
fn new_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("system randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn recent_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("recent.txt"))
}

/// Roots served before, newest first, as the RootIds the server's lists and
/// status map speak.
///
/// Nothing here checks that they are still there. It used to, and that was a
/// blocking stat per entry on the way to the picker: one remembered folder on a
/// disconnected network drive and the dialog was twenty seconds late, every
/// launch. `check_roots` finds out afterwards instead, the pane greys out what is
/// gone, and the file loses it so the next launch never lists it.
fn recent(app: &AppHandle) -> Vec<String> {
    let Some(file) = recent_file(app) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };
    recent_ids(&text)
}

/// The file's lines as RootIds. A file written before the RootId change holds
/// the verbatim form (`\\?\C:\…`), which is a spelling of a local path and not
/// an id, so every line goes through the same normalization a fresh one gets —
/// otherwise the same folder is two entries and neither matches the status map.
/// A remote id passes through untouched: `display_path` only strips a prefix
/// nothing but Windows produces.
fn recent_ids(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| treeserve::util::display_path(Path::new(l)))
        .take(RECENT_MAX)
        .collect()
}

/// The Recent list with `id` at the front: an id already in it moves rather
/// than being added again, and the list keeps its length.
fn with_front(mut list: Vec<String>, id: &str) -> Vec<String> {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(RECENT_MAX);
    list
}

/// Moves a RootId to the front of Recent, on disk and in the running server so
/// the next page render shows it. Places are deliberately not fed through here:
/// that list stays fixed.
///
/// Public: a downstream app that re-roots onto its own backend records the root
/// the same way local opens are recorded. The id is whatever that backend calls
/// the root — for a local one, the display-form path.
pub fn remember_root_id(app: &AppHandle, id: &str) {
    let list = with_front(recent(app), id);

    if let Some(serving) = app.try_state::<Serving>() {
        serving.inner.state.cfg.set_recent(list.clone());
        // Whoever got this far has already resolved the root, so this one is
        // known good without anybody having to look again.
        serving.inner.state.cfg.set_root_status(id.to_string(), RootStatus::Ok);
    }
    let Some(file) = recent_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // One id per line, which is the same string the pane shows and the status
    // map is keyed by, so the three never disagree about which root is which.
    let text: String = list.iter().map(|id| format!("{id}\n")).collect();
    let _ = fs::write(file, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the pane makes, and the one `ErrorKind` cannot make on its
    /// own: a folder someone deleted is gone, and a share whose host is off is not
    /// gone, it is unreachable. Windows answers the second with `ERROR_BAD_NETPATH`,
    /// which std reports as `NotFound` — the same kind as the first.
    #[test]
    fn deleted_is_missing_and_a_dead_mount_is_not() {
        // ENOENT, and ERROR_FILE_NOT_FOUND, which share the number 2.
        assert_eq!(classify(&io::Error::from_raw_os_error(2)), RootStatus::Missing);

        // ERROR_BAD_NETPATH on Windows, ESTALE elsewhere: the mount is there and
        // the server is not.
        let dead = if cfg!(windows) { 53 } else { 116 };
        assert_eq!(
            classify(&io::Error::from_raw_os_error(dead)),
            RootStatus::Unreachable
        );
    }

    /// An allowlist that matched by prefix would wave
    /// `http://telesight.localhost.evil.com` through on the strength of
    /// `http://telesight.localhost`; equality on the serialized origin (or the
    /// whole scheme, for opaque-origin custom protocols) does not.
    #[test]
    fn lookalike_origins_stay_outside() {
        let allowed = vec!["telesight:".to_string(), "http://telesight.localhost".to_string()];
        let u = |s: &str| tauri::Url::parse(s).unwrap();
        assert!(origin_allowed(&allowed, &u("telesight://term")));
        assert!(origin_allowed(&allowed, &u("http://telesight.localhost/term.html")));
        assert!(!origin_allowed(&allowed, &u("http://telesight.localhost.evil.com/x")));
        assert!(!origin_allowed(&allowed, &u("http://evil.com/telesight.localhost")));
        assert!(!origin_allowed(&allowed, &u("https://telesight.localhost/x")));
    }

    /// What the Recent file holds and what the list speaks are now the same
    /// thing — RootIds — and the three cases that used to disagree: a reopened
    /// root moves to the front instead of doubling, a remote id comes back out
    /// spelled exactly as it went in, and a line written before ids existed
    /// holds a Windows verbatim path, which is the same root under a spelling
    /// nothing else in the app uses.
    #[test]
    fn recent_is_a_list_of_ids_and_a_reopened_one_moves() {
        let ids = recent_ids("C:\\Users\\x\r\n\\\\?\\C:\\work\n\n  ssh:prod:/var/www  \n");
        assert_eq!(ids, [r"C:\Users\x", r"C:\work", "ssh:prod:/var/www"]);

        assert_eq!(
            with_front(ids.clone(), "ssh:prod:/var/www"),
            ["ssh:prod:/var/www", r"C:\Users\x", r"C:\work"]
        );
        // The verbatim line and the id it normalizes to are one entry, not two.
        assert_eq!(with_front(ids, r"C:\work").len(), 3);

        let full: Vec<String> = (0..RECENT_MAX).map(|i| format!("/d{i}")).collect();
        assert_eq!(with_front(full, "/new").len(), RECENT_MAX);
    }

    /// The line between "probe it" and "leave it to whoever brought it" is
    /// treeserve's `root_id_is_local`; this pins the cases the shell cares
    /// about (probing, pruning) so a grammar change there fails loudly here.
    #[test]
    fn drive_letters_are_local_and_schemes_are_not() {
        assert!(treeserve::root_id_is_local("/home/x/mix"));
        assert!(treeserve::root_id_is_local(r"C:\Users\x"));
        assert!(treeserve::root_id_is_local(r"\\server\share"));
        assert!(treeserve::root_id_is_local("/odd:name/dir"));
        assert!(!treeserve::root_id_is_local("ssh:prod-web:/var/www"));
        assert!(!treeserve::root_id_is_local("s3:bucket:/data"));
    }
}
