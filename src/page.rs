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
/// Refresh: the page as the disk has it now. Three quarters of a circle with the
/// gap and the arrowhead at the top right, so the drawing is the turn itself. The
/// head is the chevron `ICON_UP` and `ICON_BACK` use, rather than the square hook
/// most icon sets put on this one — an arrow is already spelled a particular way
/// here and a second spelling of it would only be a second spelling.
const ICON_REFRESH: &str =
    "<path d=\"M12.16 5.8A4.8 4.8 0 1 1 8 3.4\"/><path d=\"M6.5 2.1L8 3.4l-1.5 1.3\"/>";
/// The way out of a directory, on the `..` row.
const ICON_UP: &str = "<path d=\"M8 12.8V3.8\"/><path d=\"M4.3 7.5L8 3.8l3.7 3.7\"/>";
/// The shell's Back button. Drawn like the rest rather than typed as `←`, which
/// is a character with its own advance width and its own idea of the baseline,
/// and so never lined up with the icons beside it.
const ICON_BACK: &str = "<path d=\"M13 8H3\"/><path d=\"M7.2 3.8L3 8l4.2 4.2\"/>";
/// The theme flag says which setting is chosen rather than which one is next: a
/// sun for light, a moon for dark, and half of each for following the system.
/// Three settings, three drawings — resolving `auto` to the sun or the moon the
/// system happens to be on would draw it as an explicit choice, and lose the one
/// thing about that setting worth showing.
const ICON_SUN: &str = "<path d=\"M8 4.8a3.2 3.2 0 100 6.4 3.2 3.2 0 000-6.4z\"/>\
     <path d=\"M8 1.2v1.6M8 13.2v1.6M1.2 8h1.6M13.2 8h1.6M3.2 3.2l1.2 1.2\
     M11.6 11.6l1.2 1.2M12.8 3.2l-1.2 1.2M4.4 11.6l-1.2 1.2\"/>";
/// Centred on (8, 8) with a radius of 6, so it sits where the sun sits.
const ICON_MOON: &str = "<path d=\"M14 8.53A6 6 0 117.47 2 4.67 4.67 0 0014 8.53z\"/>";
/// Half filled, on the same circle. The fill runs out to the middle of the
/// stroke for the reason the pane's does: stopping at the inside of it leaves a
/// hairline seam. Unlike `Ln` and `Tree`, this fill is not a state — the flag has
/// three settings, so half-and-half means half of each, not "on".
const ICON_THEME_AUTO: &str = "<path d=\"M8 2a6 6 0 100 12A6 6 0 008 2z\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M8 2a6 6 0 000 12z\"/>";
// The two switches below are binary, so each says which way it is set in its own
// ink — more ink for on — since that is the whole of the state at the widths
// where the words are gone. How the ink arrives differs, and it follows the
// thing being switched rather than one house rule: the pane is there either way
// and fills in, while the line numbers are simply drawn or not.
//
// Neither draws anything on top of a fill, which is deliberate: an outline
// showing through solid ink would have to be painted in the colour behind the
// pill to read as a hole, and the ink here is `currentColor` — `--muted`, a
// mid-grey that goes *lighter* in the dark theme. So the knockout could not be
// white; it would have to be `--bg-subtle` and track the theme with it. Filling
// in place of the detail rather than over it avoids owning that problem, and at
// 14px a 1px hole in a 3px strip only reads as mud anyway.

/// The tree flag: a pane down the left of the window, which is what it hides.
fn icon_pane(on: bool) -> String {
    format!(
        "<path d=\"M2.2 3h11.6v10H2.2z\"/><path d=\"M6.4 3v10\"/>{}",
        if on {
            // Run the fill out to the middle of the frame and of the divider
            // rather than stopping at the inside of either stroke: butting two
            // shapes of the same colour edge to edge leaves a hairline of
            // half-covered pixels between them.
            "<path fill=\"currentColor\" stroke=\"none\" d=\"M2.2 3h4.2v10H2.2z\"/>"
        } else {
            "<path d=\"M3.6 5.8h1.4M3.6 8h1.4\"/>"
        }
    )
}

/// "Serve this folder as the root": an arrow going in through the side of a frame.
const ICON_AS_ROOT: &str = "<path d=\"M9.8 3.4h2.8v9.2H9.8\"/><path d=\"M3.4 8h5.8\"/>\
     <path d=\"M6.8 5.6L9.2 8l-2.4 2.4\"/>";

/// Line numbers: the lines they count, with the numbers beside them when they are
/// on. Presence, not fill, for this one — the marks in the gutter *are* the
/// numbers, so drawing them is what "on" means and anything else reads backwards.
/// The lines run the full width once the numbers are gone, which is what the
/// gutter's space does on the page too.
fn icon_lineno(on: bool) -> &'static str {
    if on {
        "<path d=\"M2.6 4.2h.8M2.6 8h.8M2.6 11.8h.8\"/>\
         <path d=\"M6 4.2h7.4M6 8h7.4M6 11.8h7.4\"/>"
    } else {
        "<path d=\"M2.6 4.2h10.8M2.6 8h10.8M2.6 11.8h10.8\"/>"
    }
}

/// Wraps icon paths in an `<svg>` that inherits colour and text size.
pub fn svg_icon(paths: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 16 16\" width=\"14\" height=\"14\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" \
         aria-hidden=\"true\">{paths}</svg>"
    )
}

/// The icon cell of a listing row.
///
/// Directories carry the class rather than being found by one: `tr:has(a.dir)`
/// asked the row about its link, which left the colour to whether the webview
/// knew `:has()`, and never applied to search results, whose links carry no
/// class at all.
fn entry_icon(name: &str, is_dir: bool) -> String {
    icon_cell(is_dir, icon_for(name, is_dir))
}

fn icon_cell(is_dir: bool, paths: &str) -> String {
    format!(
        "<span class=\"icon{}\">{}</span>",
        if is_dir { " dir" } else { "" },
        svg_icon(paths)
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
/// when there is not. `icon` is raw markup, and always a drawn one: every
/// character that would do instead brings its own metrics, and a row of pills is
/// only tidy when the thing inside each one measures the same.
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

/// Everything down to the end of the header: the head, and the one line that
/// says where you are and what can be done here. Both page shapes below start
/// with it and differ only in what they hang underneath.
#[allow(clippy::too_many_arguments)]
fn head_and_header(
    state: &State,
    prefs: Prefs,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    show_ln_toggle: bool,
    show_pane_flag: bool,
    extra_body_class: &str,
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
    // The syntax colours live in a stylesheet of their own, which is out of reach
    // of the print rules in ours: a dark theme would send its own pale code to
    // paper and the page would print with a gap where the listing was. So paper
    // is named in the light sheet's media and screens in the dark one's — the
    // same swap the palette makes for everything else, made where it has to be.
    let syntax_css = match prefs.theme {
        ThemeMode::Auto => concat!(
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\" media=\"print, (prefers-color-scheme: light)\">",
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\" media=\"screen and (prefers-color-scheme: dark)\">"
        )
        .to_string(),
        ThemeMode::Light => "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\">".to_string(),
        ThemeMode::Dark => concat!(
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\" media=\"screen\">",
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\" media=\"print\">"
        )
        .to_string(),
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

    // One control for both halves of the window, because there is only one
    // window: the pane and the listing come out of the same request, and the tree
    // re-reads its directories every time it is drawn. It goes ahead of the
    // page's own flags so that it sits in the same place on a listing, a file and
    // an error, instead of shifting along by however many controls that page
    // happens to bring.
    //
    // Shell only. There it is a link the shell turns into an actual reload, and
    // the window it is in has no other way to ask for one — the app has no
    // address bar and no reload button of its own. A browser has both, sitting
    // directly above ours and doing the same thing better, so on the web this is
    // a second button for something the reader already has.
    //
    // Each title spells out both the state and what a click does, because from
    // the width where the words go it is the only thing left that can.
    let mut controls = String::new();
    if state.cfg.app_ui {
        controls.push_str(&flag(
            "",
            "/.ts/reload",
            &svg_icon(ICON_REFRESH),
            "Refresh",
            "Reload this page (F5)",
        ));
    }
    controls.push_str(extra_controls);
    if show_ln_toggle {
        let (label, val) = if prefs.ln { ("Ln: on", "0") } else { ("Ln: off", "1") };
        controls.push_str(&flag(
            "",
            &set_href("ln", val, url_now),
            &svg_icon(&icon_lineno(prefs.ln)),
            label,
            if prefs.ln {
                "Line numbers on — click to hide"
            } else {
                "Line numbers off — click to show"
            },
        ));
    }
    // The switch is the pane, not the tree inside it: with the pane on, the tree
    // is what it is for and is always there, and with the pane off the listing
    // has the window. A switch for the tree alone would have left an empty
    // column behind, which is neither of the two things anyone wants.
    let (pane_label, pane_val, pane_title) = if prefs.sidebar {
        ("Pane: on", "0", "Side pane shown — click to hide")
    } else {
        ("Pane: off", "1", "Side pane hidden — click to show")
    };
    // `paneflag`: the stylesheet drops this one at the width where the pane it
    // switches is gone, rather than leaving a switch with nothing on the end.
    // The raw view drops it for the same reason at every width: there is no pane
    // on that page for it to be the switch of.
    if show_pane_flag {
        controls.push_str(&flag(
            "paneflag",
            &set_href("sidebar", pane_val, url_now),
            &svg_icon(&icon_pane(prefs.sidebar)),
            pane_label,
            pane_title,
        ));
    }
    let (theme_icon, theme_title) = match prefs.theme {
        ThemeMode::Auto => (
            ICON_THEME_AUTO,
            "Theme: following the system — click for light",
        ),
        ThemeMode::Light => (ICON_SUN, "Theme: light — click for dark"),
        ThemeMode::Dark => (ICON_MOON, "Theme: dark — click to follow the system"),
    };
    controls.push_str(&flag(
        "",
        &set_href("theme", prefs.theme.next().as_str(), url_now),
        &svg_icon(theme_icon),
        &format!("Theme: {}", prefs.theme.as_str()),
        theme_title,
    ));

    // The shell's own chrome, and all of it: one button on the line the path and
    // the flags already had, rather than a browser-style row of its own. Both
    // links are inert here — the shell intercepts them, and this server has no
    // route for either.
    let back = if state.cfg.app_ui {
        format!(
            "\n  {}",
            flag("back", "/.ts/back", &svg_icon(ICON_BACK), "Back", "Back (Alt+Left)")
        )
    } else {
        String::new()
    };
    let classes = match (state.cfg.app_ui, extra_body_class) {
        (false, "") => String::new(),
        (true, "") => " class=\"app\"".to_string(),
        (false, c) => format!(" class=\"{c}\""),
        (true, c) => format!(" class=\"app {c}\""),
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
<body{classes}>
<header>{back}
  <div class="crumbs">{crumbs}</div>
  <div class="controls">{controls}</div>
</header>"#,
        data_theme = data_theme,
        title = html_escape(&title),
        syntax_css = syntax_css,
        classes = classes,
        back = back,
        crumbs = crumbs,
        controls = controls,
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

    // One switch, one thing: the pane is either there with everything in it or
    // not there at all. In the shell that takes Places and Recent with it, which
    // is the honest trade for a full-width listing — the picker they are
    // shortcuts to is in the status line, and that line is always on screen.
    let sidebar = if prefs.sidebar {
        pane_html(state, rel)
    } else {
        String::new()
    };

    format!(
        r#"{chrome}
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
        chrome = head_and_header(
            state,
            prefs,
            rel,
            url_now,
            extra_controls,
            show_ln_toggle,
            true,
            ""
        ),
        sidebar = sidebar,
        content = content,
        // The served root gets the same head-and-leaf treatment as a Recent, and
        // for the same reason: what a plain ellipsis drops off the end of a path
        // is the folder you are actually in. The version goes first in the line
        // and last in importance, so it is what leaves when the line is short.
        footer = format!(
            "<span class=\"where\" title=\"{0}\"><span class=\"app\">{1} v{2} &middot;</span>{3}</span>\
             {4}",
            html_escape(&display_path(&state.cfg.root())),
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            path_label(&display_path(&state.cfg.root())),
            pick
        ),
    )
}

/// The raw view's page: the line that says which file this is, and under it the
/// file. Nothing else — no pane, no status line, not a pixel of padding of ours
/// — because what is under that line is not our document to lay out. Whatever
/// margins it has are the ones it brought, and the engine showing it is the one
/// that knows what they should be.
pub fn bare_layout(
    state: &State,
    prefs: Prefs,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    content: &str,
) -> String {
    format!(
        "{chrome}\n{content}\n</body>\n</html>\n",
        chrome = head_and_header(
            state,
            prefs,
            rel,
            url_now,
            extra_controls,
            false,
            false,
            "rawview"
        ),
        content = content,
    )
}

const TREE_MAX_PER_DIR: usize = 150;

/// The left pane: the directory tree, and in the desktop shell the Places and
/// Recent shortcuts below it plus the folder picker.
///
/// Only called when the pane is on, and then it always has its tree — the flag
/// in the header is the pane itself, all of it or none.
fn pane_html(state: &State, cur: &[String]) -> String {
    let mut out = String::from("<nav class=\"tree\">");
    // The tree first and foremost: it is what the pane is for, and it is what
    // grows, so it takes the height and the shortcuts below settle for what is
    // left.
    tree_dir(state, &state.cfg.root(), &mut Vec::new(), cur, &mut out);
    if state.cfg.app_ui {
        out.push_str("<div class=\"chooser\">");
        // Two paths on purpose: opening a Place is not something Recent should
        // collect, or the fixed list would keep copying itself into the other
        // one. Opening something from Recent does move it back to the top.
        root_list(
            &mut out,
            state,
            "places",
            "Places",
            "/.ts/place",
            state.cfg.places.iter().map(|(l, p)| (Some(l.clone()), p.clone())),
        );
        // No label, so each of these is drawn as its path: a Place is somewhere
        // with a name, a Recent is just a folder you were in, and which one it
        // was is a question about where it sits.
        root_list(
            &mut out,
            state,
            "recent",
            "Recent",
            "/.ts/root",
            state.cfg.recent().iter().map(|p| (None, p.clone())),
        );
        out.push_str("</div>");
    }
    out.push_str("</nav>");
    out
}

/// One pane section of "serve this folder instead" links. `action` is the path
/// the shell recognises, which is also how it tells a Place from a Recent. An
/// item with no label is drawn as its path, by `path_label`.
///
/// An entry whose check has come back badly is greyed and says what happened,
/// and stays a link: the check is a snapshot from whenever it ran, a drive that
/// was not ready can be ready now, and the only way to find out is to ask for
/// it. Clicking one costs whatever the wait costs, which is the same wait the
/// list used to charge everybody up front.
fn root_list<I: Iterator<Item = (Option<String>, PathBuf)>>(
    out: &mut String,
    state: &State,
    class: &str,
    heading: &str,
    action: &str,
    items: I,
) {
    let links: String = items
        .map(|(label, path)| {
            let full = display_path(&path);
            let note = state.cfg.root_status(&path).note();
            format!(
                "<li{}><a href=\"{}?path={}\" title=\"{}\">{}</a>{}</li>",
                if note.is_some() { " class=\"gone\"" } else { "" },
                action,
                percent_encode(&full),
                html_escape(&full),
                match &label {
                    Some(l) => html_escape(l),
                    None => path_label(&full),
                },
                match note {
                    Some(n) => format!("<span class=\"why\">{n}</span>"),
                    None => String::new(),
                }
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

/// A remembered root split into the path above it and the folder itself, so the
/// pane can show as many levels as it has room for and drop the middle of the
/// rest. The name is the part that identifies the entry, so it is the part that
/// never goes: the stylesheet ellipsises the head and leaves the leaf alone.
///
/// Splitting here rather than measuring anywhere is what keeps this a static
/// page. How much of `/home/hanhua/mix` survives is a question about the width
/// of a rendered box, and CSS is the only thing on either side of the wire that
/// knows that — the server would have to guess at a font it cannot see.
fn path_label(full: &str) -> String {
    match full.rfind(|c| c == '/' || c == '\\') {
        // A separator with something after it. The separator goes to the *leaf*,
        // not the end of the head: it is the one character that says the name is
        // a name under something, and on the head it would be the first thing an
        // ellipsis ate — leaving `/home/hanhua/w…project-alpha`, where the name
        // reads as the rest of the clipped word. On the leaf it cannot be lost:
        // `/home/hanhua/w…/project-alpha`. Nothing changes when the whole path
        // fits, since the two halves still spell it exactly.
        Some(i) if i + 1 < full.len() => format!(
            "{}<span class=\"leaf\">{}</span>",
            // Empty for an absolute path of one component — `/hanhua` is all
            // leaf, and a head of nothing is a box with an ellipsis in it.
            match &full[..i] {
                "" => String::new(),
                head => format!("<span class=\"head\">{}</span>", html_escape(head)),
            },
            html_escape(&full[i..])
        ),
        // A trailing separator, or none at all: a filesystem or drive root, or a
        // lone name. Either way there is no path above it to shorten.
        _ => format!("<span class=\"leaf\">{}</span>", html_escape(full)),
    }
}

/// The "serve this folder instead" button on a directory row in the tree.
///
/// Clicking the name walks into a directory; this re-roots to it, which is the
/// thing you cannot otherwise do without the picker and a path you have already
/// got on screen. It goes through `/.ts/root`, the same link a Recent uses, so the
/// shell remembers it in Recent exactly as it would any other opened root.
///
/// Only in the shell. Nothing else can act on it: the server has no such route,
/// and a page served over a network has no business offering one.
fn as_root_link(state: &State, abs: &Path) -> String {
    if !state.cfg.app_ui {
        return String::new();
    }
    let full = display_path(abs);
    format!(
        "<a class=\"asroot\" href=\"/.ts/root?path={}\" title=\"Serve {} as the root\">{}</a>",
        percent_encode(&full),
        html_escape(&full),
        svg_icon(ICON_AS_ROOT)
    )
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
            let child_abs = abs.join(&e.name);
            // The name and the button share a row of their own, so an expanded
            // directory's children hang below it rather than beside the button,
            // and a long name ellipsises against the button instead of pushing it
            // off the pane.
            out.push_str(&format!(
                "<li{}><span class=\"row\"><a class=\"dir\" href=\"{}/\">{} {}/</a>{}</span>",
                cls,
                html_escape(&href),
                arrow,
                html_escape(&e.name),
                as_root_link(state, &child_abs)
            ));
            if on_path {
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
            "<tr><td>{}<a href=\"{}/\">..</a></td><td class=\"size\"></td><td class=\"time\"></td></tr>",
            icon_cell(true, ICON_UP),
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
            "<tr><td>{}<a href=\"{}\"{}>{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            entry_icon(&e.name, e.is_dir),
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
            "<tr><td>{}<a href=\"{}\">{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            entry_icon(&e.name, e.is_dir),
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

/// Shown while the shell is finding out whether a folder can be opened.
///
/// Resolving a path is a syscall with no time limit — a drive letter mapped to a
/// host that is off takes as long as the network stack takes to give up — so the
/// shell does it on a thread and parks the window here meanwhile. Still the served
/// root's page furniture, because that is still what is being served: only the
/// middle of the window is waiting.
pub fn wait_page(state: &State, prefs: Prefs, url_now: &str, path: &str) -> String {
    let content = format!(
        "<div class=\"bigmsg\"><p>Opening {}&hellip;</p>\
         <p>If the folder is on a drive or a share that is not answering, this waits \
         for as long as that takes to find out.</p></div>",
        html_escape(path)
    );
    layout(state, prefs, &[], url_now, "", false, &content)
}

pub fn error_page(state: &State, prefs: Prefs, rel: &[String], url_now: &str, code: u32, msg: &str) -> String {
    let content = format!(
        "<div class=\"bigmsg\"><h2>{}</h2><p>{}</p><p><a href=\"/\">Back to root</a></p></div>",
        code,
        html_escape(msg)
    );
    layout(state, prefs, rel, url_now, "", false, &content)
}
