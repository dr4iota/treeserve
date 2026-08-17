use std::fs;
use std::path::Path;

use crate::md::render_markdown;
use crate::page::{flag, layout, svg_icon, Prefs, ICON_DOWNLOAD, ICON_RAW, ICON_RENDERED, ICON_SOURCE};
use crate::util::*;
use crate::State;

const MAX_HIGHLIGHT_BYTES: u64 = 2 * 1024 * 1024;

fn raw_href(rel: &[String]) -> String {
    format!("{}?raw=1", href_path(rel))
}

fn std_controls(rel: &[String]) -> String {
    let base = href_path(rel);
    format!(
        "{}{}",
        flag(
            "",
            &format!("{base}?raw=1"),
            &svg_icon(ICON_RAW),
            "Raw",
            "The file as it is on disk"
        ),
        flag(
            "",
            &format!("{base}?dl=1"),
            &svg_icon(ICON_DOWNLOAD),
            "Download",
            "Save a copy"
        )
    )
}

fn meta_line(size: u64, mtime: Option<std::time::SystemTime>, extra: &str) -> String {
    format!(
        "<p class=\"filemeta\">{}{} &middot; {}</p>",
        human_size(size),
        mtime
            .map(|t| format!(" &middot; {}", fmt_time(t)))
            .unwrap_or_default(),
        extra
    )
}

pub fn file_page(
    state: &State,
    prefs: Prefs,
    rel: &[String],
    abs: &Path,
    query: &[(String, String)],
    url_now: &str,
) -> String {
    let name = rel.last().map(String::as_str).unwrap_or("");
    let ext = ext_of(name);
    let meta = fs::metadata(abs).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta.and_then(|m| m.modified().ok());

    if IMAGE_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><a href=\"{1}\"><img class=\"preview-img\" src=\"{1}\" alt=\"{2}\"></a></div>",
            meta_line(size, mtime, "image"),
            html_escape(&raw_href(rel)),
            html_escape(name)
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }
    if VIDEO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><video class=\"preview\" controls src=\"{}\"></video></div>",
            meta_line(size, mtime, "video"),
            html_escape(&raw_href(rel))
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }
    if AUDIO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<audio class=\"preview\" controls src=\"{}\"></audio>",
            meta_line(size, mtime, "audio"),
            html_escape(&raw_href(rel))
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }
    if ext == "pdf" {
        let content = format!(
            "{}<div class=\"fit\"><embed class=\"pdf\" src=\"{}\" type=\"application/pdf\"></div>",
            meta_line(size, mtime, "pdf"),
            html_escape(&raw_href(rel))
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }

    // Text-ish content from here on.
    if size > MAX_HIGHLIGHT_BYTES {
        let content = format!(
            "{}<div class=\"bigmsg\"><p>File is too large to render ({}).</p>\
             <p><a href=\"{}?raw=1\">View raw</a> or <a href=\"{2}?dl=1\">download</a>.</p></div>",
            meta_line(size, mtime, "large file"),
            human_size(size),
            html_escape(&href_path(rel))
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }

    let Ok(bytes) = fs::read(abs) else {
        let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
        return layout(state, prefs, rel, url_now, "", false, &content);
    };

    if looks_binary(&bytes[..bytes.len().min(8192)]) {
        let content = format!(
            "{}<div class=\"bigmsg\"><p>Binary file.</p>\
             <p><a href=\"{}?dl=1\">Download</a></p></div>",
            meta_line(size, mtime, "binary"),
            html_escape(&href_path(rel))
        );
        return layout(state, prefs, rel, url_now, &std_controls(rel), false, &content);
    }

    let text = String::from_utf8_lossy(&bytes);
    let want_source = query_get(query, "src") == Some("1");

    if MARKDOWN_EXTS.contains(&ext.as_str()) && !want_source {
        let body = render_markdown(&state.hl, &text);
        let controls = format!(
            "{}{}",
            flag(
                "",
                &format!("{}?src=1", href_path(rel)),
                &svg_icon(ICON_SOURCE),
                "Source",
                "Highlighted source instead"
            ),
            std_controls(rel)
        );
        let content = format!("<div class=\"md\">{}</div>", body);
        return layout(state, prefs, rel, url_now, &controls, false, &content);
    }

    // Highlighted source view.
    let first_line = text.lines().next().unwrap_or("");
    let syntax = state.hl.syntax_for(name, &ext, first_line);
    let html = state.hl.highlight(syntax, &text);
    let line_count = text.lines().count().max(1);

    let gutter = if prefs.ln {
        let mut nums = String::with_capacity(line_count * 4);
        for i in 1..=line_count {
            nums.push_str(&i.to_string());
            nums.push('\n');
        }
        format!("<pre class=\"gutter\" aria-hidden=\"true\">{}</pre>", nums)
    } else {
        String::new()
    };

    let mut controls = String::new();
    if MARKDOWN_EXTS.contains(&ext.as_str()) {
        controls.push_str(&flag(
            "",
            &href_path(rel),
            &svg_icon(ICON_RENDERED),
            "Rendered",
            "Rendered view instead",
        ));
    }
    controls.push_str(&std_controls(rel));

    let content = format!(
        "{}<div class=\"codewrap\">{}<pre class=\"hl-code\"><code>{}</code></pre></div>",
        meta_line(
            size,
            mtime,
            &format!(
                "{} &middot; {} line{}",
                html_escape(&syntax.name),
                line_count,
                if line_count == 1 { "" } else { "s" }
            )
        ),
        gutter,
        html
    );
    layout(state, prefs, rel, url_now, &controls, true, &content)
}
