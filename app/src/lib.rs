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

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use treeserve::Config;

const WINDOW: &str = "main";
const WORKER_THREADS: usize = 4;

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
                open_root(app, dir);
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
                Some(dir) => open_root(&handle, dir),
                // Started by double-click: the working directory is wherever
                // the shell happened to put us, which is never what the user
                // meant, so ask which folder to browse.
                None => ask_for_folder(handle, true),
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running treeserve");
}

/// First argument that names a readable directory.
fn first_dir_arg<I: Iterator<Item = String>>(args: I) -> Option<PathBuf> {
    args.filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .find_map(|p| p.canonicalize().ok().filter(|p| p.is_dir()))
}

/// Native folder picker. Non-blocking: the blocking variant would deadlock the
/// event loop when called from `setup` or a menu handler.
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    let mut dialog = app.dialog().file().set_title("Choose a folder to browse");
    if let Some(last) = last_root(&app) {
        dialog = dialog.set_directory(last);
    }
    dialog.pick_folder(move |picked| match picked {
        Some(path) => match path.into_path() {
            Ok(dir) => open_root(&app, dir),
            Err(e) => fail(&app, &format!("Cannot use that folder: {e}"), exit_if_cancelled),
        },
        None if exit_if_cancelled => app.exit(0),
        None => {}
    });
}

/// Serves `dir`: starts the server on first use, re-roots it afterwards.
fn open_root(app: &AppHandle, dir: PathBuf) {
    let dir = match dir.canonicalize() {
        Ok(d) if d.is_dir() => d,
        _ => {
            fail(app, &format!("Not a folder: {}", dir.display()), false);
            return;
        }
    };
    remember_root(app, &dir);

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
    .menu(menu(app).map_err(|e| e.to_string())?)
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
                return true;
            }
            // Links out of the served tree belong in the user's browser.
            let _ = app.opener().open_url(url.as_str(), None::<&str>);
            false
        }
    })
    .build()
    .map_err(|e| format!("Cannot create the window: {e}"))?;

    win.on_menu_event(|win, event| {
        let app = win.app_handle().clone();
        let js = match event.id().as_ref() {
            "open" => {
                ask_for_folder(app, false);
                return;
            }
            "reload" => "location.reload()",
            "back" => "history.back()",
            "forward" => "history.forward()",
            "home" => "location.assign('/')",
            _ => return,
        };
        if let Some(w) = app.get_webview_window(WINDOW) {
            let _ = w.eval(js);
        }
    });

    // Dropping a folder on the window re-roots; dropping a file opens its page.
    win.on_window_event({
        let app = app.clone();
        move |event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                && let Some(dir) = paths.iter().find(|p| p.is_dir())
            {
                open_root(&app, dir.clone());
            }
        }
    });

    Ok(())
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, "open", "Open Folder…", true, Some("CmdOrCtrl+O"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "back", "Back", true, Some("Alt+Left"))?,
            &MenuItem::with_id(app, "forward", "Forward", true, Some("Alt+Right"))?,
            &MenuItem::with_id(app, "home", "Top of Tree", true, Some("CmdOrCtrl+Home"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?,
        ],
    )?;
    Menu::with_items(app, &[&file, &view])
}

fn window_title(root: &Path) -> String {
    match root.file_name() {
        Some(name) => format!("{} — treeserve", name.to_string_lossy()),
        None => "treeserve".to_string(),
    }
}

/// Reports a problem in a native dialog, since a GUI build has nowhere to print.
fn fail(app: &AppHandle, msg: &str, fatal: bool) {
    let app = app.clone();
    let msg = msg.to_string();
    app.dialog()
        .message(msg)
        .kind(MessageDialogKind::Error)
        .title("treeserve")
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

fn last_root_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("last-root.txt"))
}

fn last_root(app: &AppHandle) -> Option<PathBuf> {
    let path = fs::read_to_string(last_root_file(app)?).ok()?;
    let path = PathBuf::from(path.trim());
    path.is_dir().then_some(path)
}

/// Remembered only to seed the next folder picker, not to reopen silently.
fn remember_root(app: &AppHandle, root: &Path) {
    let Some(file) = last_root_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(file, root.to_string_lossy().as_bytes());
}
