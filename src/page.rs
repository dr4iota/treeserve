use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::util::*;
use crate::State;

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn from_str(s: &str) -> Option<ThemeMode> {
        match s {
            "auto" => Some(ThemeMode::Auto),
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }
    pub fn next(self) -> ThemeMode {
        match self {
            ThemeMode::Auto => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Auto,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Prefs {
    pub theme: ThemeMode,
    pub ln: bool,
    pub sidebar: bool,
}

pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
}

pub fn read_dir_sorted(state: &State, abs: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(abs) else {
        return out;
    };
    for de in rd.flatten() {
        let name = de.file_name().to_string_lossy().into_owned();
        if !state.cfg.show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = de.metadata().ok();
        let is_dir = de.path().is_dir(); // follows symlinks
        out.push(Entry {
            name,
            is_dir,
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            mtime: meta.and_then(|m| m.modified().ok()),
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

// Icons are drawn, not typed. The obvious characters for these — folder,
// picture, film, note — all live in the emoji planes, and a font stack without
// them (any DejaVu-only Linux, for one) draws a missing-glyph box in their
// place. Paths cannot miss, they take the surrounding colour, and they stay
// legible in both themes. Each constant is the inside of an `<svg>`; `svg_icon`
// supplies the rest.
pub const ICON_FOLDER: &str = "<path d=\"M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 \
     .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z\"/>";
const ICON_IMAGE: &str = "<path d=\"M2 3.6h12v8.8H2z\"/><path d=\"M2.9 11.6l3.5-3.4 1.9 2 2.3-2.7 \
     3 3.4\"/><path d=\"M5.6 5.6a1.1 1.1 0 100 2.2 1.1 1.1 0 000-2.2z\"/>";
const ICON_AUDIO: &str = "<path d=\"M6.6 11.4V4l6.4-1.4v2.2L6.6 6.2\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M4.9 9.6a2 2 0 100 4 2 2 0 000-4z\"/>";
const ICON_VIDEO: &str = "<path d=\"M1.9 3.9h12.2v8.2H1.9z\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M6.6 6.3l4 1.7-4 1.7z\"/>";
const ICON_DOC: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>\
     <path d=\"M5.6 8.4h5M5.6 10.6h5\"/>";
const ICON_FILE: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>";
/// The file exactly as it is on disk, so: leaving for it.
pub const ICON_RAW: &str =
    "<path d=\"M9.6 3.4h3v3\"/><path d=\"M12.6 3.4L8.2 7.8\"/><path d=\"M12 9.4v3.2H3.4V4h3.2\"/>";
pub const ICON_DOWNLOAD: &str =
    "<path d=\"M8 2.9v6.7\"/><path d=\"M5.2 7l2.8 2.8L10.8 7\"/><path d=\"M3.2 13.1h9.6\"/>";
pub const ICON_SOURCE: &str = "<path d=\"M6.2 4.4L2.7 8l3.5 3.6\"/><path d=\"M9.8 4.4L13.3 8l-3.5 3.6\"/>";
pub const ICON_RENDERED: &str =
    "<path d=\"M3.4 3h9.2v10H3.4z\"/><path d=\"M5.4 6h5.2M5.4 8.4h5.2M5.4 10.8h3.4\"/>";

/// Wraps icon paths in an `<svg>` that inherits colour and text size.
pub fn svg_icon(paths: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 16 16\" width=\"14\" height=\"14\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" \
         aria-hidden=\"true\">{paths}</svg>"
    )
}

fn icon_for(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return ICON_FOLDER;
    }
    let ext = ext_of(name);
    if IMAGE_EXTS.contains(&ext.as_str()) {
        ICON_IMAGE
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        ICON_AUDIO
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        ICON_VIDEO
    } else if MARKDOWN_EXTS.contains(&ext.as_str()) {
        ICON_DOC
    } else {
        ICON_FILE
    }
}

fn set_href(key: &str, val: &str, back: &str) -> String {
    format!("/.ts/set?{}={}&back={}", key, val, percent_encode(back))
}

/// A control that spells itself out when there is room and shrinks to a symbol
/// when there is not. `icon` is raw markup: a character reference, or the folder
/// drawn below, since no font can be relied on for that one.
pub(crate) fn flag(class: &str, href: &str, icon: &str, label: &str, title: &str) -> String {
    let class = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    format!(
        "<a{} href=\"{}\" title=\"{}\"><span class=\"ico\">{}</span><span class=\"lbl\">{}</span></a>",
        class,
        html_escape(href),
        html_escape(title),
        icon,
        html_escape(label)
    )
}

/// Full page shell. `rel` is the current path segments, `url_now` the raw
/// (still percent-encoded) path+query of this request, used for toggles.
pub fn layout(
    state: &State,
    prefs: Prefs,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    show_ln_toggle: bool,
    content: &str,
) -> String {
    let site_title = state.cfg.title();
    let title = if rel.is_empty() {
        site_title.clone()
    } else {
        format!("{} — {}", rel.join("/"), site_title)
    };

    let data_theme = match prefs.theme {
        ThemeMode::Auto => String::new(),
        m => format!(" data-theme=\"{}\"", m.as_str()),
    };
    let syntax_css = match prefs.theme {
        ThemeMode::Auto => concat!(
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\" media=\"(prefers-color-scheme: light)\">",
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\" media=\"(prefers-color-scheme: dark)\">"
        )
        .to_string(),
        ThemeMode::Light => "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\">".to_string(),
        ThemeMode::Dark => "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\">".to_string(),
    };

    // Breadcrumbs
    let mut crumbs = format!("<a href=\"/\">{}</a>", html_escape(&site_title));
    let mut acc: Vec<String> = Vec::new();
    for (i, seg) in rel.iter().enumerate() {
        acc.push(seg.clone());
        crumbs.push_str("<span class=\"sep\">/</span>");
        if i + 1 == rel.len() {
            crumbs.push_str(&html_escape(seg));
        } else {
            crumbs.push_str(&format!(
                "<a href=\"{}/\">{}</a>",
                html_escape(&href_path(&acc)),
                html_escape(seg)
            ));
        }
    }

    let mut controls = String::from(extra_controls);
    if show_ln_toggle {
        let (label, val) = if prefs.ln { ("Ln: on", "0") } else { ("Ln: off", "1") };
        controls.push_str(&flag("", &set_href("ln", val, url_now), "#", label, "Line numbers"));
    }
    let (tree_label, tree_val) = if prefs.sidebar {
        ("Tree: on", "0")
    } else {
        ("Tree: off", "1")
    };
    controls.push_str(&flag(
        "",
        &set_href("sidebar", tree_val, url_now),
        "&#x2630;", // trigram for heaven, i.e. the usual list glyph
        tree_label,
        // Says what the symbol is for, since at narrow widths it is all there
        // is to go on.
        "File tree",
    ));
    controls.push_str(&flag(
        "",
        &set_href("theme", prefs.theme.next().as_str(), url_now),
        "&#x25D0;", // half-filled circle
        &format!("Theme: {}", prefs.theme.as_str()),
        "Cycle theme",
    ));

    // The shell's own chrome, and all of it: one button on the line the path and
    // the flags already had, rather than a browser-style row of its own. Both
    // links are inert here — the shell intercepts them, and this server has no
    // route for either.
    let back = if state.cfg.app_ui {
        format!(
            "\n  {}",
            flag("back", "/.ts/back", "&#x2190;", "Back", "Back (Alt+Left)")
        )
    } else {
        String::new()
    };
    // The picker lives in the status line, which is always on screen — the pane
    // that used to hold it is the first thing to go when the window narrows.
    let pick = if state.cfg.app_ui {
        flag(
            "pick",
            "/.ts/open",
            &svg_icon(ICON_FOLDER),
            "Open Folder…",
            "Open Folder… (Ctrl+O)",
        )
    } else {
        String::new()
    };
    let body_class = if state.cfg.app_ui { " class=\"app\"" } else { "" };

    // In the shell the pane also holds Places, Recent and the picker button, so
    // it stays even with the tree off; served on its own it is the tree or
    // nothing, exactly as before.
    let sidebar = if prefs.sidebar || state.cfg.app_ui {
        pane_html(state, prefs, rel)
    } else {
        String::new()
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en"{data_theme}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='%234c8dff' stroke-width='1.4' stroke-linejoin='round'><path d='M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z'/></svg>">
<link rel="stylesheet" href="/.ts/app.css">
<link rel="stylesheet" href="/.ts/math.css">
{syntax_css}
</head>
<body{body_class}>
<header>{back}
  <div class="crumbs">{crumbs}</div>
  <div class="controls">{controls}</div>
</header>
<div class="shell">
{sidebar}
<main>
{content}
</main>
</div>
<footer>{footer}</footer>
</body>
</html>
"#,
        data_theme = data_theme,
        title = html_escape(&title),
        syntax_css = syntax_css,
        body_class = body_class,
        back = back,
        crumbs = crumbs,
        controls = controls,
        sidebar = sidebar,
        content = content,
        footer = format!(
            "<span class=\"where\">{} v{} &middot; {}</span>{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            html_escape(&display_path(&state.cfg.root())),
            pick
        ),
    )
}

const TREE_MAX_PER_DIR: usize = 150;

/// The left pane: the directory tree, and in the desktop shell the Places and
/// Recent shortcuts above it plus the folder picker pinned below.
///
/// Keeps the `nav.tree` element even when it holds more than the tree, so one
/// stylesheet rule still hides the whole pane on narrow screens.
fn pane_html(state: &State, prefs: Prefs, cur: &[String]) -> String {
    let mut out = String::from("<nav class=\"tree\">");
    // The tree first and foremost: it is what the pane is for, and it is what
    // grows, so it takes the height and the shortcuts below settle for what is
    // left.
    if prefs.sidebar {
        tree_dir(state, &state.cfg.root(), &mut Vec::new(), cur, &mut out);
    }
    if state.cfg.app_ui {
        out.push_str("<div class=\"chooser\">");
        // Two paths on purpose: opening a Place is not something Recent should
        // collect, or the fixed list would keep copying itself into the other
        // one. Opening something from Recent does move it back to the top.
        root_list(
            &mut out,
            "places",
            "Places",
            "/.ts/place",
            state.cfg.places.iter().cloned(),
        );
        root_list(
            &mut out,
            "recent",
            "Recent",
            "/.ts/root",
            state.cfg.recent().iter().map(|p| (root_label(p), p.clone())),
        );
        out.push_str("</div>");
    }
    out.push_str("</nav>");
    out
}

/// One pane section of "serve this folder instead" links. `action` is the path
/// the shell recognises, which is also how it tells a Place from a Recent.
fn root_list<I: Iterator<Item = (String, PathBuf)>>(
    out: &mut String,
    class: &str,
    heading: &str,
    action: &str,
    items: I,
) {
    let links: String = items
        .map(|(label, path)| {
            let full = display_path(&path);
            format!(
                "<li><a href=\"{}?path={}\" title=\"{}\">{}</a></li>",
                action,
                percent_encode(&full),
                html_escape(&full),
                html_escape(&label)
            )
        })
        .collect();
    if links.is_empty() {
        return;
    }
    out.push_str(&format!(
        "<section class=\"{}\"><h2>{}</h2><ul>{}</ul></section>",
        class, heading, links
    ));
}

/// Short name for a remembered root: its own name, or the whole path for a
/// filesystem or drive root, which has no file name to show.
fn root_label(p: &Path) -> String {
    match p.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => display_path(p),
    }
}

fn tree_dir(state: &State, abs: &Path, rel: &mut Vec<String>, cur: &[String], out: &mut String) {
    let entries = read_dir_sorted(state, abs);
    let total = entries.len();
    out.push_str("<ul>");
    for e in entries.into_iter().take(TREE_MAX_PER_DIR) {
        rel.push(e.name.clone());
        let is_cur = rel.as_slice() == cur;
        let on_path = cur.len() >= rel.len() && cur[..rel.len()] == rel[..];
        let href = href_path(rel);
        let cls = if is_cur { " class=\"cur\"" } else { "" };
        if e.is_dir {
            let arrow = if on_path { "&#x25BE;" } else { "&#x25B8;" };
            out.push_str(&format!(
                "<li{}><a class=\"dir\" href=\"{}/\">{} {}/</a>",
                cls,
                html_escape(&href),
                arrow,
                html_escape(&e.name)
            ));
            if on_path {
                let child_abs = abs.join(&e.name);
                tree_dir(state, &child_abs, rel, cur, out);
            }
            out.push_str("</li>");
        } else {
            out.push_str(&format!(
                "<li{}><a href=\"{}\">{}</a></li>",
                cls,
                html_escape(&href),
                html_escape(&e.name)
            ));
        }
        rel.pop();
    }
    if total > TREE_MAX_PER_DIR {
        out.push_str(&format!(
            "<li class=\"more\">&hellip; {} more</li>",
            total - TREE_MAX_PER_DIR
        ));
    }
    out.push_str("</ul>");
}

const SEARCH_MAX_RESULTS: usize = 2000;
const SEARCH_MAX_SCANNED: usize = 50_000;
const SEARCH_MAX_DEPTH: usize = 12;

pub fn listing_page(
    state: &State,
    prefs: Prefs,
    rel: &[String],
    abs: &Path,
    query: &[(String, String)],
    url_now: &str,
) -> String {
    let q = query_get(query, "q").unwrap_or("");
    let recursive = query_get(query, "r") == Some("1");

    let mut content = format!(
        r#"<form class="filter" method="get">
<input type="text" name="q" value="{}" placeholder="glob, e.g. *.rs" aria-label="filter pattern">
<label><input type="checkbox" name="r" value="1"{}> recursive</label>
<button>Filter</button>{}
</form>
"#,
        html_escape(q),
        if recursive { " checked" } else { "" },
        if q.is_empty() {
            String::new()
        } else {
            format!(
                " <a href=\"{}\">clear</a>",
                html_escape(&href_path(rel).trim_end_matches('/').to_string() /* dir href */) + "/"
            )
        }
    );

    if q.is_empty() {
        content.push_str(&entries_table(state, rel, abs));
    } else {
        content.push_str(&search_results(state, rel, abs, q, recursive));
    }

    layout(state, prefs, rel, url_now, "", false, &content)
}

fn entries_table(state: &State, rel: &[String], abs: &Path) -> String {
    let entries = read_dir_sorted(state, abs);
    let mut rows = String::new();
    if !rel.is_empty() {
        let parent = &rel[..rel.len() - 1];
        rows.push_str(&format!(
            "<tr><td><span class=\"icon\">&#x2B06;&#xFE0F;</span><a href=\"{}/\">..</a></td><td class=\"size\"></td><td class=\"time\"></td></tr>",
            html_escape(href_path(parent).trim_end_matches('/'))
        ));
    }
    for e in &entries {
        let mut href = {
            let mut r = rel.to_vec();
            r.push(e.name.clone());
            href_path(&r)
        };
        if e.is_dir {
            href.push('/');
        }
        rows.push_str(&format!(
            "<tr><td><span class=\"icon\">{}</span><a href=\"{}\"{}>{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            svg_icon(icon_for(&e.name, e.is_dir)),
            html_escape(&href),
            if e.is_dir { " class=\"dir\"" } else { "" },
            html_escape(&e.name),
            if e.is_dir {
                "&mdash;".to_string()
            } else {
                human_size(e.size)
            },
            e.mtime.map(fmt_time).unwrap_or_default(),
        ));
    }
    if entries.is_empty() {
        rows.push_str("<tr><td colspan=\"3\"><em>empty directory</em></td></tr>");
    }
    format!(
        "<table class=\"listing\"><tr><th>Name</th><th class=\"size\">Size</th><th class=\"time\">Modified</th></tr>{}</table>",
        rows
    )
}

fn search_results(state: &State, rel: &[String], abs: &Path, pat: &str, recursive: bool) -> String {
    // Pattern containing '/' matches the path relative to this directory;
    // otherwise it matches the file name only.
    let match_path = pat.contains('/');
    let mut results: Vec<(Vec<String>, Entry)> = Vec::new();
    let mut scanned = 0usize;
    let mut truncated = false;

    // DFS; non-recursive mode just doesn't descend.
    let mut stack: Vec<(PathBuf, Vec<String>)> = vec![(abs.to_path_buf(), Vec::new())];
    while let Some((dir, drel)) = stack.pop() {
        for e in read_dir_sorted(state, &dir) {
            scanned += 1;
            if scanned > SEARCH_MAX_SCANNED || results.len() >= SEARCH_MAX_RESULTS {
                truncated = true;
                break;
            }
            let mut erel = drel.clone();
            erel.push(e.name.clone());
            let hay = if match_path {
                erel.join("/")
            } else {
                e.name.clone()
            };
            if fnmatch(pat, &hay) {
                results.push((erel.clone(), e));
            } else if e.is_dir && recursive && erel.len() < SEARCH_MAX_DEPTH {
                stack.push((dir.join(erel.last().unwrap()), erel));
            } else if e.is_dir && recursive {
                truncated = true;
            }
        }
        if truncated {
            break;
        }
    }

    let mut out = format!(
        "<p class=\"matchnote\">{} match{}{}{}</p>",
        results.len(),
        if results.len() == 1 { "" } else { "es" },
        if recursive { " (recursive)" } else { "" },
        if truncated { ", truncated" } else { "" }
    );
    let mut rows = String::new();
    for (erel, e) in &results {
        let mut full = rel.to_vec();
        full.extend(erel.iter().cloned());
        let mut href = href_path(&full);
        if e.is_dir {
            href.push('/');
        }
        rows.push_str(&format!(
            "<tr><td><span class=\"icon\">{}</span><a href=\"{}\">{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            svg_icon(icon_for(&e.name, e.is_dir)),
            html_escape(&href),
            html_escape(&erel.join("/")),
            if e.is_dir {
                "&mdash;".to_string()
            } else {
                human_size(e.size)
            },
            e.mtime.map(fmt_time).unwrap_or_default(),
        ));
    }
    if !results.is_empty() {
        out.push_str(&format!(
            "<table class=\"listing\"><tr><th>Path</th><th class=\"size\">Size</th><th class=\"time\">Modified</th></tr>{}</table>",
            rows
        ));
    }
    out
}

/// Plain-text listing for non-browser clients (curl, scripts).
pub fn listing_text(state: &State, abs: &Path) -> String {
    let mut out = String::new();
    for e in read_dir_sorted(state, abs) {
        out.push_str(&e.name);
        if e.is_dir {
            out.push('/');
        }
        out.push('\n');
    }
    out
}

pub fn error_page(state: &State, prefs: Prefs, rel: &[String], url_now: &str, code: u32, msg: &str) -> String {
    let content = format!(
        "<div class=\"bigmsg\"><h2>{}</h2><p>{}</p><p><a href=\"/\">Back to root</a></p></div>",
        code,
        html_escape(msg)
    );
    layout(state, prefs, rel, url_now, "", false, &content)
}
