mod hl;
mod md;
mod page;
mod util;
mod view;

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use two_face::theme::EmbeddedThemeName;

use hl::Hl;
use page::{Prefs, ThemeMode};
use util::*;

const APP_CSS: &str = include_str!("app.css");

pub struct Config {
    pub root: PathBuf, // canonicalized
    pub title: String,
    pub bind: String,
    pub port: u16,
    pub theme: ThemeMode,
    pub ln: bool,
    pub sidebar: bool,
    pub show_hidden: bool,
    pub threads: usize,
    pub syn_light: EmbeddedThemeName,
    pub syn_dark: EmbeddedThemeName,
}

pub struct State {
    pub cfg: Config,
    pub hl: Hl,
}

fn print_help() {
    print!(
        "\
{name} {version} — serve a directory as a browsable, rendered website

USAGE:
    {name} [OPTIONS] [ROOT]

ARGS:
    ROOT                   directory to serve (default: .)

OPTIONS:
    -b, --bind ADDR        address to bind (default: 127.0.0.1)
    -p, --port PORT        port to listen on (default: 8080)
    -t, --theme MODE       default theme: auto | light | dark (default: auto)
        --no-line-numbers  line numbers off by default
        --no-sidebar       file tree sidebar off by default
        --hidden           show dotfiles
        --title NAME       site title (default: root directory name)
        --threads N        worker threads (default: 8)
        --syntax-theme NAME
                           highlighting theme for both light and dark mode
        --syntax-theme-light NAME
                           highlighting theme for light mode (default: InspiredGitHub)
        --syntax-theme-dark NAME
                           highlighting theme for dark mode (default: OneHalfDark)
        --list-syntax-themes
                           list embedded highlighting themes and exit
    -h, --help             print this help
    -V, --version          print version
",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
    );
}

fn parse_args() -> Config {
    let mut root: Option<PathBuf> = None;
    let mut bind = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut theme = ThemeMode::Auto;
    let mut ln = true;
    let mut sidebar = true;
    let mut show_hidden = false;
    let mut title: Option<String> = None;
    let mut threads: usize = 8;
    let mut syn_light = EmbeddedThemeName::InspiredGithub;
    let mut syn_dark = EmbeddedThemeName::OneHalfDark;

    let mut args = std::env::args().skip(1);
    let die = |msg: &str| -> ! {
        eprintln!("error: {}", msg);
        eprintln!("run with --help for usage");
        exit(2);
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "-V" | "--version" => {
                println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            "-b" | "--bind" => {
                bind = args.next().unwrap_or_else(|| die("--bind needs a value"));
            }
            "-p" | "--port" => {
                port = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| die("--port needs a number"));
            }
            "-t" | "--theme" => {
                let v = args.next().unwrap_or_else(|| die("--theme needs a value"));
                theme = ThemeMode::from_str(&v)
                    .unwrap_or_else(|| die("--theme must be auto, light or dark"));
            }
            "--no-line-numbers" => ln = false,
            "--no-sidebar" => sidebar = false,
            "--hidden" => show_hidden = true,
            "--title" => {
                title = Some(args.next().unwrap_or_else(|| die("--title needs a value")));
            }
            "--threads" => {
                threads = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&n| n > 0)
                    .unwrap_or_else(|| die("--threads needs a positive number"));
            }
            "--syntax-theme" | "--syntax-theme-light" | "--syntax-theme-dark" => {
                let v = args
                    .next()
                    .unwrap_or_else(|| die(&format!("{} needs a theme name", a)));
                let t = hl::find_theme(&v).unwrap_or_else(|| {
                    die(&format!(
                        "unknown theme {:?}; see --list-syntax-themes",
                        v
                    ))
                });
                if a != "--syntax-theme-dark" {
                    syn_light = t;
                }
                if a != "--syntax-theme-light" {
                    syn_dark = t;
                }
            }
            "--list-syntax-themes" => {
                for name in hl::theme_names() {
                    println!("{}", name);
                }
                exit(0);
            }
            _ if a.starts_with('-') => die(&format!("unknown option: {}", a)),
            _ => {
                if root.is_some() {
                    die("multiple ROOT arguments");
                }
                root = Some(PathBuf::from(a));
            }
        }
    }

    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or_else(|e| {
        eprintln!("error: cannot open root {}: {}", root.display(), e);
        exit(1);
    });
    if !root.is_dir() {
        eprintln!("error: root {} is not a directory", root.display());
        exit(1);
    }
    let title = title.unwrap_or_else(|| {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".to_string())
    });

    Config {
        root,
        title,
        bind,
        port,
        theme,
        ln,
        sidebar,
        show_hidden,
        threads,
        syn_light,
        syn_dark,
    }
}

fn main() {
    let cfg = parse_args();
    let addr = format!("{}:{}", cfg.bind, cfg.port);
    let server = Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("error: cannot bind {}: {}", addr, e);
        exit(1);
    });
    println!(
        "{} v{}: serving {} at http://{}/",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        cfg.root.display(),
        addr
    );

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
    for h in handles {
        let _ = h.join();
    }
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
                "dv_theme" => {
                    if let Some(t) = ThemeMode::from_str(&v) {
                        prefs.theme = t;
                    }
                }
                "dv_ln" => prefs.ln = v == "1",
                "dv_sidebar" => prefs.sidebar = v == "1",
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

    // Internal routes (assets + preference cookies).
    match path_raw.as_str() {
        "/.dv/app.css" => {
            let _ = rq.respond(css_resp(APP_CSS));
            return;
        }
        "/.dv/syntax-light.css" => {
            let _ = rq.respond(css_resp(&state.hl.css_light));
            return;
        }
        "/.dv/syntax-dark.css" => {
            let _ = rq.respond(css_resp(&state.hl.css_dark));
            return;
        }
        "/.dv/set" => {
            set_prefs(rq, &query);
            return;
        }
        _ => {}
    }

    // Decode and sanitize the filesystem path.
    let decoded = percent_decode(&path_raw);
    let rel: Vec<String> = decoded
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if decoded.contains('\0') || rel.iter().any(|s| s == "." || s == "..") {
        let _ = rq.respond(html_resp(
            400,
            page::error_page(state, prefs, &[], &url_now, 400, "bad path"),
        ));
        return;
    }

    let mut abs = state.cfg.root.clone();
    for seg in &rel {
        abs.push(seg);
    }
    // canonicalize resolves symlinks; the prefix check keeps everything
    // inside the served root.
    let canon = match abs.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            let _ = rq.respond(html_resp(
                404,
                page::error_page(state, prefs, &rel, &url_now, 404, "not found"),
            ));
            return;
        }
    };
    if !canon.starts_with(&state.cfg.root) {
        let _ = rq.respond(html_resp(
            403,
            page::error_page(state, prefs, &rel, &url_now, 403, "forbidden"),
        ));
        return;
    }

    if canon.is_dir() {
        // Directory URLs need a trailing slash so relative links resolve.
        if !path_raw.ends_with('/') {
            let loc = if query_raw.is_empty() {
                format!("{}/", path_raw)
            } else {
                format!("{}/?{}", path_raw, query_raw)
            };
            let _ =
                rq.respond(Response::empty(StatusCode(301)).with_header(h("Location", &loc)));
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

/// `/.dv/set?theme=dark&ln=1&sidebar=0&back=/some/where` — store prefs in
/// cookies and bounce back. Pure SSR option switching, no JS.
fn set_prefs(rq: Request, query: &[(String, String)]) {
    let mut resp = Response::empty(StatusCode(303));
    let cookie = |k: &str, v: &str| {
        h(
            "Set-Cookie",
            &format!("{}={}; Path=/; Max-Age=31536000; SameSite=Lax", k, v),
        )
    };
    for (k, v) in query {
        match k.as_str() {
            "theme" if ThemeMode::from_str(v).is_some() => {
                resp.add_header(cookie("dv_theme", v));
            }
            "ln" if v == "0" || v == "1" => {
                resp.add_header(cookie("dv_ln", v));
            }
            "sidebar" if v == "0" || v == "1" => {
                resp.add_header(cookie("dv_sidebar", v));
            }
            _ => {}
        }
    }
    let back = query_get(query, "back")
        .filter(|b| b.starts_with('/') && !b.starts_with("//"))
        .unwrap_or("/");
    resp.add_header(h("Location", back));
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

fn serve_raw(rq: Request, abs: &std::path::Path, name: &str, attachment: bool) {
    let Ok(mut file) = File::open(abs) else {
        let _ =
            rq.respond(Response::from_string("not found").with_status_code(StatusCode(404)));
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
                let _ = rq
                    .respond(Response::from_string("io error").with_status_code(StatusCode(500)));
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
