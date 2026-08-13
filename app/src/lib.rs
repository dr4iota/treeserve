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
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use treeserve::{Config, RootStatus};

const WINDOW: &str = "main";
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
  else { return; }
  e.preventDefault();
});
"#;

/// The running server, kept in Tauri's managed state.
struct Serving {
    inner: treeserve::Serving,
    /// `http://127.0.0.1:<port>`, the only origin the window may navigate to.
    origin: String,
}

pub fn run() {
    tauri::Builder::default()
        // Must be registered first. A second launch re-roots the open window
        // when it names a directory (Explorer "open with", drag onto the exe).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(dir) = first_dir_arg(argv.into_iter().skip(1)) {
                open_root(app, dir, true);
            }
            if let Some(w) = app.get_webview_window(WINDOW) {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
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
            match first_dir_arg(std::env::args().skip(1)) {
                // Started from a shell with a path, or via a shell verb.
                Some(dir) => open_root(&handle, dir, true),
                // Started by double-click: the working directory is wherever the
                // shell happened to put us, which is never what the user meant, so
                // ask which folder to browse.
                None => ask_for_folder(handle, true),
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running treesight");
}

/// First argument that names a readable directory.
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

/// Where to open the picker: the newest Recent, if it answers straight away.
///
/// A remembered folder can be on a drive that is not there, and handing the
/// picker one of those makes the picker do the waiting we just stopped doing.
/// Half a second on a thread we can walk away from, then the picker decides for
/// itself. The probe thread may sit there for another twenty seconds; it holds
/// nothing but a send end nobody is listening to.
fn picker_start_dir(app: &AppHandle) -> Option<PathBuf> {
    let last = recent(app).into_iter().next()?;
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
                    serving.inner.state.cfg.set_root_status(dir.clone(), status);
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
        remember_root(app, &dir);
    }

    if let Some(serving) = app.try_state::<Serving>() {
        serving.inner.state.cfg.set_root(dir.clone());
        // Back to the listing root; the token cookie is already set, and the
        // page-load hook retitles the window.
        if let Some(win) = app.get_webview_window(WINDOW) {
            if let Ok(url) = format!("{}/", serving.origin).parse() {
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
    let token = new_token();
    let mut cfg = Config::new(root.clone());
    cfg.port = 0; // let the OS pick a free port
    cfg.threads = WORKER_THREADS;
    cfg.set_token(Some(token.clone()));
    // Turns on the page's own chooser: path bar, history buttons, Places,
    // Recent and the picker button. Only ever set here — a server reachable by
    // anything but this window has no business offering them.
    cfg.app_ui = true;
    cfg.places = places(app);
    cfg.set_recent(recent(app));

    let inner = treeserve::spawn(cfg).map_err(|e| format!("Cannot start the local server: {e}"))?;
    let origin = format!("http://127.0.0.1:{}", inner.addr.port());
    // First stop is the token handshake, which sets the cookie and redirects.
    let entry = format!("{origin}/.ts/auth?t={token}&back=/");
    app.manage(Serving {
        inner,
        origin: origin.clone(),
    });

    let win = WebviewWindowBuilder::new(
        app,
        WINDOW,
        WebviewUrl::External(entry.parse().map_err(|e| format!("bad url: {e}"))?),
    )
    .title(window_title(&root))
    .inner_size(1200.0, 850.0)
    .min_inner_size(480.0, 360.0)
    // Shown by whoever knows there is something worth showing — `open_root`,
    // once it has a folder. A window built before the picker has been answered
    // would otherwise flash a placeholder.
    .visible(false)
    .initialization_script(SHORTCUTS)
    // The title follows the served root, which "Open Folder…" can change, so
    // it is refreshed on every page load rather than only at window creation.
    .on_page_load({
        let app = app.clone();
        move |win, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && let Some(serving) = app.try_state::<Serving>()
            {
                let _ = win.set_title(&window_title(&serving.inner.state.cfg.root()));
            }
        }
    })
    .on_navigation({
        let app = app.clone();
        move |url| {
            if url.as_str().starts_with(&origin) {
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
        let paths: Vec<PathBuf> = state
            .cfg
            .places
            .iter()
            .map(|(_, p)| p.clone())
            .chain(recent.iter().cloned())
            .collect();

        let checks: Vec<_> = paths
            .into_iter()
            .map(|path| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let status = match fs::metadata(&path) {
                        Ok(m) if m.is_dir() => RootStatus::Ok,
                        // Something is there, but not a folder any more.
                        Ok(_) => RootStatus::Missing,
                        Err(e) => classify(&e),
                    };
                    state.cfg.set_root_status(path.clone(), status);
                    (path, status)
                })
            })
            .collect();

        let answers: Vec<(PathBuf, RootStatus)> =
            checks.into_iter().filter_map(|h| h.join().ok()).collect();

        let gone: Vec<PathBuf> = answers
            .iter()
            .filter(|(path, status)| *status != RootStatus::Ok && recent.contains(path))
            .map(|(path, _)| path.clone())
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
fn prune_recent(file: &Path, gone: &[PathBuf]) {
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    let kept: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !gone.iter().any(|g| g == Path::new(l)))
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
    let Ok(target) = treeserve::resolve_in_root(&serving.inner.state.cfg.root(), url.path()) else {
        return false;
    };
    if !target.abs.is_file() {
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
    let src = target.abs;
    dialog.save_file(move |dest| {
        let Some(dest) = dest.and_then(|d| d.into_path().ok()) else {
            return; // cancelled
        };
        if let Err(e) = fs::copy(&src, &dest) {
            fail(&app, &format!("Could not save {}: {e}", dest.display()), false);
        }
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
        // Recent; a Place is the same but not worth remembering, since the pane
        // already lists it. Both carry a path we rendered ourselves, though
        // `open_root` still checks it — a remembered folder can go away.
        "/.ts/root" | "/.ts/place" => match url.query_pairs().find(|(k, _)| k == "path") {
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
    let mut out: Vec<(String, PathBuf)> = [
        ("Home", p.home_dir()),
        ("Desktop", p.desktop_dir()),
        ("Documents", p.document_dir()),
        ("Downloads", p.download_dir()),
    ]
    .into_iter()
    .filter_map(|(label, dir)| dir.ok().filter(|d| d.is_dir()).map(|d| (label.to_string(), d)))
    .collect();

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
    #[cfg(not(windows))]
    out.push(("Filesystem".to_string(), PathBuf::from("/")));

    out
}

fn window_title(root: &Path) -> String {
    match root.file_name() {
        Some(name) => format!("{} — treesight", name.to_string_lossy()),
        // A drive or a share: no last component, so say which one it is.
        None => format!("{} — treesight", treeserve::util::display_path(root)),
    }
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

/// Roots served before, newest first, exactly as recorded.
///
/// Nothing here checks that they are still there. It used to, and that was a
/// blocking stat per entry on the way to the picker: one remembered folder on a
/// disconnected network drive and the dialog was twenty seconds late, every
/// launch. `check_roots` finds out afterwards instead, the pane greys out what is
/// gone, and the file loses it so the next launch never lists it.
fn recent(app: &AppHandle) -> Vec<PathBuf> {
    let Some(file) = recent_file(app) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .take(RECENT_MAX)
        .collect()
}

/// Moves `root` to the front of the Recent list, on disk and in the running
/// server so the next page render shows it. Places are deliberately not fed
/// through here: that list stays fixed.
fn remember_root(app: &AppHandle, root: &Path) {
    let mut list = recent(app);
    list.retain(|p| p != root);
    list.insert(0, root.to_path_buf());
    list.truncate(RECENT_MAX);

    if let Some(serving) = app.try_state::<Serving>() {
        serving.inner.state.cfg.set_recent(list.clone());
        // We got here through `canonicalize`, so this one is known good without
        // anybody having to look again.
        serving
            .inner
            .state
            .cfg
            .set_root_status(root.to_path_buf(), RootStatus::Ok);
    }
    let Some(file) = recent_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
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
}
