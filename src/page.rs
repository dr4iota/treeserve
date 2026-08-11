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

fn icon_for(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "&#x1F4C1;"; // folder
    }
    let ext = ext_of(name);
    if IMAGE_EXTS.contains(&ext.as_str()) {
        "&#x1F5BC;&#xFE0F;" // picture
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        "&#x1F3B5;" // music note
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        "&#x1F3AC;" // clapper
    } else if MARKDOWN_EXTS.contains(&ext.as_str()) {
        "&#x1F4DD;" // memo
    } else {
        "&#x1F4C4;" // page
    }
}

fn set_href(key: &str, val: &str, back: &str) -> String {
    format!("/.ts/set?{}={}&back={}", key, val, percent_encode(back))
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
    let title = if rel.is_empty() {
        state.cfg.title.clone()
    } else {
        format!("{} — {}", rel.join("/"), state.cfg.title)
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
    let mut crumbs = format!(
        "<a href=\"/\">{}</a>",
        html_escape(&state.cfg.title)
    );
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
        controls.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            html_escape(&set_href("ln", val, url_now)),
            label
        ));
    }
    let (tree_label, tree_val) = if prefs.sidebar {
        ("Tree: on", "0")
    } else {
        ("Tree: off", "1")
    };
    controls.push_str(&format!(
        "<a href=\"{}\">{}</a>",
        html_escape(&set_href("sidebar", tree_val, url_now)),
        tree_label
    ));
    controls.push_str(&format!(
        "<a href=\"{}\" title=\"Cycle theme\">Theme: {}</a>",
        html_escape(&set_href("theme", prefs.theme.next().as_str(), url_now)),
        prefs.theme.as_str()
    ));

    let sidebar = if prefs.sidebar {
        tree_html(state, rel)
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
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'><text y='13' font-size='13'>&#x1F4C1;</text></svg>">
<link rel="stylesheet" href="/.ts/app.css">
<link rel="stylesheet" href="/.ts/math.css">
{syntax_css}
</head>
<body>
<header>
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
        crumbs = crumbs,
        controls = controls,
        sidebar = sidebar,
        content = content,
        footer = format!(
            "{} v{} &middot; {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            html_escape(&state.cfg.root.display().to_string())
        ),
    )
}

const TREE_MAX_PER_DIR: usize = 150;

/// Server-rendered directory tree. Directories on the path to `cur` are
/// expanded; everything else renders as a link (navigation re-renders the
/// tree with that branch expanded), so no JavaScript is needed.
fn tree_html(state: &State, cur: &[String]) -> String {
    let mut out = String::from("<nav class=\"tree\">");
    tree_dir(state, &state.cfg.root, &mut Vec::new(), cur, &mut out);
    out.push_str("</nav>");
    out
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
            icon_for(&e.name, e.is_dir),
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
            icon_for(&e.name, e.is_dir),
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
