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
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use treeserve::Config;

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
            match first_dir_arg(std::env::args().skip(1)) {
                // Started from a shell with a path, or via a shell verb.
                Some(dir) => open_root(&handle, dir, true),
                // Started by double-click: the working directory is wherever
                // the shell happened to put us, which is never what the user
                // meant, so ask which folder to browse.
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

/// Native folder picker. Non-blocking: the blocking variant would deadlock the
/// event loop when called from `setup` or from a navigation handler.
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    let mut dialog = app.dialog().file().set_title("Choose a folder to browse");
    if let Some(last) = recent(&app).into_iter().next() {
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

/// Serves `dir`: starts the server on first use, re-roots it afterwards.
///
/// `remember` keeps it out of the Recent list, which is what the pane's Places
/// need: that list is fixed, and a Place that added itself to Recent would just
/// be duplicating a shortcut the pane already shows.
fn open_root(app: &AppHandle, dir: PathBuf, remember: bool) {
    let dir = match dir.canonicalize() {
        Ok(d) if d.is_dir() => d,
        _ => {
            fail(app, &format!("Not a folder: {}", dir.display()), false);
            return;
        }
    };
    if remember {
        remember_root(app, &dir);
    }

    if let Some(serving) = app.try_state::<Serving>() {
        serving.inner.state.cfg.set_root(dir.clone());
        // Back to the listing root; the token cookie is already set, and the
        // page-load hook retitles the window.
        if let Some(win) = app.get_webview_window(WINDOW)
            && let Ok(url) = format!("{}/", serving.origin).parse()
        {
            let _ = win.navigate(url);
        }
        return;
    }

    if let Err(e) = start(app, dir) {
        fail(app, &e, true);
    }
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
/// picker button, the history buttons, and everything that re-roots (Places,
/// Recent, the path bar). Returns whether the navigation was one of ours and
/// should therefore be cancelled.
fn shell_action(app: &AppHandle, url: &tauri::Url) -> bool {
    match url.path() {
        "/.ts/open" => ask_for_folder(app.clone(), false),
        "/.ts/back" => eval(app, "history.back()"),
        "/.ts/forward" => eval(app, "history.forward()"),
        // Recent and the path bar; a Place is the same but not worth
        // remembering, since the pane already lists it.
        "/.ts/root" | "/.ts/place" => match url.query_pairs().find(|(k, _)| k == "path") {
            // A typed path may start with a `~` no shell has expanded, and may
            // name something that is not there at all, hence the fixing up and
            // the check inside `open_root`.
            Some((_, path)) => {
                let dir = expand_home(app, path.trim());
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

/// `~` and `~/…` mean the home directory, as everywhere else.
fn expand_home(app: &AppHandle, path: &str) -> PathBuf {
    let Ok(home) = app.path().home_dir() else {
        return PathBuf::from(path);
    };
    match path.strip_prefix('~') {
        Some("") => home,
        Some(rest) if rest.starts_with('/') || rest.starts_with('\\') => home.join(&rest[1..]),
        _ => PathBuf::from(path),
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

    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if drive.is_dir() {
            out.push((format!("{}:", letter as char), drive));
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

/// Roots served before, newest first. Folders that have since gone away are
/// dropped, so a stale list cannot offer something that no longer opens.
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
        .filter(|p| p.is_dir())
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
    }
    let Some(file) = recent_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = fs::write(file, text);
}
