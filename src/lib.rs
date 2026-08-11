//! Server-rendered file/code tree browser.
//!
//! The binary in `main.rs` is a thin CLI over this library; embedders (the
//! Tauri app in `app/`) call [`spawn`] and drive the server themselves.

pub mod hl;
pub mod md;
pub mod page;
pub mod util;
pub mod view;

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use two_face::theme::EmbeddedThemeName;

use hl::Hl;
use page::{Prefs, ThemeMode};
use util::*;

const APP_CSS: &str = include_str!("app.css");
const MATH_CSS: &str = include_str!("math.css");

pub struct Config {
    /// Canonicalized served root. Behind a lock so an embedder can re-root a
    /// running server; the CLI never changes it.
    root: RwLock<Arc<PathBuf>>,
    /// Site title. `None` means "name of the served directory", which follows
    /// the root when it changes.
    title: Option<String>,
    pub bind: String,
    pub port: u16,
    pub theme: ThemeMode,
    pub ln: bool,
    pub sidebar: bool,
    pub show_hidden: bool,
    pub threads: usize,
    pub syn_light: EmbeddedThemeName,
    pub syn_dark: EmbeddedThemeName,
    /// When set, requests must carry a matching `ts_token` cookie, obtained by
    /// visiting `/.ts/auth?t=<token>&back=<path>`. Used by the desktop app,
    /// where the loopback port would otherwise be open to any local process.
    /// `None` — the CLI default — disables the check and the auth route.
    token: Option<String>,
}

impl Config {
    /// Config with library defaults, matching the CLI's own defaults.
    pub fn new(root: PathBuf) -> Config {
        Config {
            root: RwLock::new(Arc::new(root)),
            title: None,
            bind: "127.0.0.1".to_string(),
            port: 8080,
            theme: ThemeMode::Auto,
            ln: true,
            sidebar: true,
            show_hidden: false,
            threads: 8,
            syn_light: EmbeddedThemeName::InspiredGithub,
            syn_dark: EmbeddedThemeName::OneHalfDark,
            token: None,
        }
    }

    pub fn root(&self) -> Arc<PathBuf> {
        Arc::clone(&self.root.read().expect("root lock"))
    }

    /// Re-roots a running server. The path should already be canonicalized.
    pub fn set_root(&self, root: PathBuf) {
        *self.root.write().expect("root lock") = Arc::new(root);
    }

    pub fn title(&self) -> String {
        match &self.title {
            Some(t) => t.clone(),
            None => self
                .root()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".to_string()),
        }
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }
}

pub struct State {
    pub cfg: Config,
    pub hl: Hl,
}

/// Where a URL path points inside the served root.
pub struct Resolved {
    /// Percent-decoded path segments, for breadcrumbs and file names.
    pub rel: Vec<String>,
    /// Canonical filesystem path.
    pub abs: PathBuf,
}

/// Why a URL path does not name something servable. The segments are carried
/// along where they are safe to echo back in a page.
pub enum PathError {
    /// Malformed or traversing path; nothing safe to show.
    Bad,
    Missing(Vec<String>),
    Outside(Vec<String>),
}

/// Resolves a URL path against the served root.
///
/// Shared by the request handler and by embedders that need the file behind a
/// link (the desktop app's download handler), so the traversal and
/// symlink-escape checks live in exactly one place.
pub fn resolve_in_root(root: &Path, url_path: &str) -> Result<Resolved, PathError> {
    let decoded = percent_decode(url_path);
    let rel: Vec<String> = decoded
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if decoded.contains('\0') || rel.iter().any(|s| s == "." || s == "..") {
        return Err(PathError::Bad);
    }

    let mut abs = root.to_path_buf();
    for seg in &rel {
        abs.push(seg);
    }
    // canonicalize resolves symlinks; the prefix check keeps everything
    // inside the served root.
    let Ok(abs) = abs.canonicalize() else {
        return Err(PathError::Missing(rel));
    };
    if !abs.starts_with(root) {
        return Err(PathError::Outside(rel));
    }
    Ok(Resolved { rel, abs })
}

/// A running server: worker threads plus the address they are serving.
pub struct Serving {
    pub addr: SocketAddr,
    pub state: Arc<State>,
    handles: Vec<JoinHandle<()>>,
}

impl Serving {
    /// Blocks until every worker thread exits (i.e. forever, in practice).
    pub fn join(self) {
        for h in self.handles {
            let _ = h.join();
        }
    }
}

/// Binds the configured address and starts the worker pool.
///
/// Pass port 0 to let the OS pick one; the assigned address is in
/// [`Serving::addr`].
pub fn spawn(cfg: Config) -> Result<Serving, Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(format!("{}:{}", cfg.bind, cfg.port))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or("server is not listening on an IP address")?;

    let threads = cfg.threads;
    let hl = Hl::new(cfg.syn_light, cfg.syn_dark);
    let state = Arc::new(State { cfg, hl });
    let server = Arc::new(server);

    let mut handles = Vec::new();
    for _ in 0..threads {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        handles.push(thread::spawn(move || loop {
            let rq = match server.recv() {
                Ok(rq) => rq,
                Err(e) => {
                    eprintln!("recv error: {}", e);
                    break;
                }
            };
            let r = panic::catch_unwind(AssertUnwindSafe(|| respond(&state, rq)));
            if let Err(e) = r {
                eprintln!("handler panicked: {:?}", e);
            }
        }));
    }

    Ok(Serving {
        addr,
        state,
        handles,
    })
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn html_resp(status: u16, body: String) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(h("Content-Type", "text/html; charset=utf-8"))
        .with_header(h("X-Content-Type-Options", "nosniff"))
}

fn css_resp(body: &str) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(h("Content-Type", "text/css; charset=utf-8"))
        .with_header(h("Cache-Control", "max-age=300"))
}

fn header_value<'a>(rq: &'a Request, name: &'static str) -> Option<&'a str> {
    rq.headers()
        .iter()
        .find(|hd| hd.field.equiv(name))
        .map(|hd| hd.value.as_str())
}

fn prefs_from(state: &State, rq: &Request) -> Prefs {
    let mut prefs = Prefs {
        theme: state.cfg.theme,
        ln: state.cfg.ln,
        sidebar: state.cfg.sidebar,
    };
    for hd in rq.headers().iter().filter(|hd| hd.field.equiv("Cookie")) {
        for (k, v) in parse_cookies(hd.value.as_str()) {
            match k.as_str() {
                "ts_theme" => {
                    if let Some(t) = ThemeMode::from_str(&v) {
                        prefs.theme = t;
                    }
                }
                "ts_ln" => prefs.ln = v == "1",
                "ts_sidebar" => prefs.sidebar = v == "1",
                _ => {}
            }
        }
    }
    prefs
}

fn wants_html(rq: &Request) -> bool {
    header_value(rq, "Accept")
        .map(|a| a.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

/// Subresource loads (e.g. <img src="relative.png"> inside rendered
/// markdown) must get raw bytes even without ?raw=1.
fn is_subresource(rq: &Request) -> bool {
    matches!(
        header_value(rq, "Sec-Fetch-Dest"),
        Some("image" | "video" | "audio" | "embed" | "object" | "font")
    )
}

/// True when a token is configured and the request does not present it.
fn unauthorized(state: &State, rq: &Request) -> bool {
    let Some(token) = &state.cfg.token else {
        return false;
    };
    let presented = rq
        .headers()
        .iter()
        .filter(|hd| hd.field.equiv("Cookie"))
        .flat_map(|hd| parse_cookies(hd.value.as_str()))
        .any(|(k, v)| k == "ts_token" && &v == token);
    !presented
}

fn respond(state: &State, rq: Request) {
    if *rq.method() != Method::Get {
        let _ = rq.respond(
            Response::from_string("method not allowed").with_status_code(StatusCode(405)),
        );
        return;
    }

    let url_now = rq.url().to_string();
    let (path_raw, query_raw) = match url_now.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url_now.clone(), String::new()),
    };
    let query = parse_query(&query_raw);
    let prefs = prefs_from(state, &rq);

    // Token handshake, desktop builds only: hand out the cookie, then bounce
    // to the requested page. Registered only when a token is configured.
    if state.cfg.token.is_some() && path_raw == "/.ts/auth" {
        set_token(state, rq, &query);
        return;
    }
    if unauthorized(state, &rq) {
        let _ = rq.respond(Response::from_string("forbidden").with_status_code(StatusCode(403)));
        return;
    }

    // Internal routes (assets + preference cookies).
    match path_raw.as_str() {
        "/.ts/app.css" => {
            let _ = rq.respond(css_resp(APP_CSS));
            return;
        }
        "/.ts/math.css" => {
            let _ = rq.respond(css_resp(MATH_CSS));
            return;
        }
        "/.ts/syntax-light.css" => {
            let _ = rq.respond(css_resp(&state.hl.css_light));
            return;
        }
        "/.ts/syntax-dark.css" => {
            let _ = rq.respond(css_resp(&state.hl.css_dark));
            return;
        }
        "/.ts/set" => {
            set_prefs(rq, &query);
            return;
        }
        _ => {}
    }

    // Decode and sanitize the filesystem path.
    let (rel, canon) = match resolve_in_root(&state.cfg.root(), &path_raw) {
        Ok(r) => (r.rel, r.abs),
        Err(PathError::Bad) => {
            let _ = rq.respond(html_resp(
                400,
                page::error_page(state, prefs, &[], &url_now, 400, "bad path"),
            ));
            return;
        }
        Err(PathError::Missing(rel)) => {
            let _ = rq.respond(html_resp(
                404,
                page::error_page(state, prefs, &rel, &url_now, 404, "not found"),
            ));
            return;
        }
        Err(PathError::Outside(rel)) => {
            let _ = rq.respond(html_resp(
                403,
                page::error_page(state, prefs, &rel, &url_now, 403, "forbidden"),
            ));
            return;
        }
    };

    if canon.is_dir() {
        // Directory URLs need a trailing slash so relative links resolve.
        if !path_raw.ends_with('/') {
            let loc = if query_raw.is_empty() {
                format!("{}/", path_raw)
            } else {
                format!("{}/?{}", path_raw, query_raw)
            };
            let _ = rq.respond(Response::empty(StatusCode(301)).with_header(h("Location", &loc)));
            return;
        }
        if wants_html(&rq) {
            let body = page::listing_page(state, prefs, &rel, &canon, &query, &url_now);
            let _ = rq.respond(html_resp(200, body));
        } else {
            let _ = rq.respond(
                Response::from_string(page::listing_text(state, &canon))
                    .with_header(h("Content-Type", "text/plain; charset=utf-8")),
            );
        }
        return;
    }

    let name = rel.last().cloned().unwrap_or_default();
    let want_raw = query_get(&query, "raw") == Some("1");
    let want_dl = query_get(&query, "dl") == Some("1");
    if want_raw || want_dl || !wants_html(&rq) || is_subresource(&rq) {
        serve_raw(rq, &canon, &name, want_dl);
        return;
    }

    let body = view::file_page(state, prefs, &rel, &canon, &query, &url_now);
    let _ = rq.respond(html_resp(200, body));
}

fn cookie_header(k: &str, v: &str) -> Header {
    h(
        "Set-Cookie",
        &format!("{}={}; Path=/; Max-Age=31536000; SameSite=Lax", k, v),
    )
}

/// Local redirect target from `?back=`, defaulting to the root.
fn back_target(query: &[(String, String)]) -> String {
    query_get(query, "back")
        .filter(|b| b.starts_with('/') && !b.starts_with("//"))
        .unwrap_or("/")
        .to_string()
}

/// `/.ts/set?theme=dark&ln=1&sidebar=0&back=/some/where` — store prefs in
/// cookies and bounce back. Pure SSR option switching, no JS.
fn set_prefs(rq: Request, query: &[(String, String)]) {
    let mut resp = Response::empty(StatusCode(303));
    for (k, v) in query {
        match k.as_str() {
            "theme" if ThemeMode::from_str(v).is_some() => {
                resp.add_header(cookie_header("ts_theme", v));
            }
            "ln" if v == "0" || v == "1" => {
                resp.add_header(cookie_header("ts_ln", v));
            }
            "sidebar" if v == "0" || v == "1" => {
                resp.add_header(cookie_header("ts_sidebar", v));
            }
            _ => {}
        }
    }
    resp.add_header(h("Location", &back_target(query)));
    let _ = rq.respond(resp);
}

/// `/.ts/auth?t=<token>&back=/` — the desktop app's opening navigation. The
/// cookie is session-scoped: a token is only valid for the run that minted it.
fn set_token(state: &State, rq: Request, query: &[(String, String)]) {
    let ok = matches!(
        (query_get(query, "t"), state.cfg.token.as_deref()),
        (Some(given), Some(want)) if given == want
    );
    if !ok {
        let _ = rq.respond(Response::from_string("forbidden").with_status_code(StatusCode(403)));
        return;
    }
    let token = state.cfg.token.as_deref().unwrap_or_default();
    let mut resp = Response::empty(StatusCode(303));
    resp.add_header(h(
        "Set-Cookie",
        &format!("ts_token={}; Path=/; SameSite=Lax", token),
    ));
    resp.add_header(h("Location", &back_target(query)));
    let _ = rq.respond(resp);
}

/// Parse "bytes=a-b" | "bytes=a-" | "bytes=-n". Multi-range is ignored
/// (the whole file is served instead, which is allowed by RFC 9110).
fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?.trim();
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let end_default = size - 1;
    match (a.is_empty(), b.is_empty()) {
        (false, true) => Some((a.parse().ok()?, end_default)),
        (false, false) => {
            let s: u64 = a.parse().ok()?;
            let e: u64 = b.parse().ok()?;
            Some((s, e.min(end_default)))
        }
        (true, false) => {
            let n: u64 = b.parse().ok()?;
            Some((size.saturating_sub(n), end_default))
        }
        (true, true) => None,
    }
}

fn serve_raw(rq: Request, abs: &Path, name: &str, attachment: bool) {
    let Ok(mut file) = File::open(abs) else {
        let _ = rq.respond(Response::from_string("not found").with_status_code(StatusCode(404)));
        return;
    };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mime = mime_for_ext(&ext_of(name));

    let mut headers = vec![
        h("Content-Type", mime),
        h("Accept-Ranges", "bytes"),
        h("X-Content-Type-Options", "nosniff"),
    ];
    if attachment {
        let safe: String = name.chars().filter(|c| *c != '"' && *c != '\\').collect();
        headers.push(h(
            "Content-Disposition",
            &format!("attachment; filename=\"{}\"", safe),
        ));
    }

    let range = header_value(&rq, "Range").and_then(|v| parse_range(v, size));
    match range {
        Some((start, end)) if start < size && start <= end => {
            if file.seek(SeekFrom::Start(start)).is_err() {
                let _ =
                    rq.respond(Response::from_string("io error").with_status_code(StatusCode(500)));
                return;
            }
            let len = end - start + 1;
            headers.push(h(
                "Content-Range",
                &format!("bytes {}-{}/{}", start, end, size),
            ));
            let resp = Response::new(
                StatusCode(206),
                headers,
                file.take(len),
                Some(len as usize),
                None,
            );
            let _ = rq.respond(resp);
        }
        Some(_) => {
            let resp = Response::from_string("range not satisfiable")
                .with_status_code(StatusCode(416))
                .with_header(h("Content-Range", &format!("bytes */{}", size)));
            let _ = rq.respond(resp);
        }
        None => {
            let resp = Response::new(StatusCode(200), headers, file, Some(size as usize), None);
            let _ = rq.respond(resp);
        }
    }
}
