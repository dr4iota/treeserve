use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::nodes::{Node, NodeValue};
use comrak::options::Plugins;
use comrak::{format_html_with_plugins, parse_document, Arena, Options};
use pulldown_latex::config::DisplayMode;
use pulldown_latex::{push_mathml, Parser as LatexParser, RenderConfig, Storage};

use crate::hl::Hl;
use crate::util::html_escape;

/// Routes fenced code blocks through the same syntect pipeline used for
/// standalone files, so markdown code gets identical class-based highlighting.
struct CodeAdapter<'a> {
    hl: &'a Hl,
}

fn write_tag(
    out: &mut dyn fmt::Write,
    tag: &str,
    attrs: HashMap<&'static str, Cow<'_, str>>,
    extra_class: Option<&str>,
) -> fmt::Result {
    write!(out, "<{}", tag)?;
    let mut class_written = false;
    for (k, v) in &attrs {
        if *k == "class" {
            class_written = true;
            match extra_class {
                Some(c) => write!(out, " class=\"{} {}\"", c, html_escape(v))?,
                None => write!(out, " class=\"{}\"", html_escape(v))?,
            }
        } else {
            write!(out, " {}=\"{}\"", k, html_escape(v))?;
        }
    }
    if !class_written {
        if let Some(c) = extra_class {
            write!(out, " class=\"{}\"", c)?;
        }
    }
    write!(out, ">")
}

impl SyntaxHighlighterAdapter for CodeAdapter<'_> {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let syntax = match lang {
            Some(l) if !l.is_empty() => self.hl.syntax_for_token(l),
            _ => self.hl.ss.find_syntax_plain_text(),
        };
        output.write_str(&self.hl.highlight(syntax, code))
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        write_tag(output, "pre", attributes, Some("hl-code"))
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        write_tag(output, "code", attributes, None)
    }
}

/// LaTeX → MathML, server-side.
///
/// Broken math is not fatal: the source is shown in red with the parser
/// message as its tooltip, which keeps the rest of the document readable
/// (pulldown-latex's own `<merror>` output inlines the full multi-line
/// diagnostic, which swamps the page).
fn latex_to_mathml(latex: &str, display: bool) -> String {
    let storage = Storage::new();
    if let Some(err) = latex_error(latex, &storage) {
        return format!(
            "<code class=\"math-error\" title=\"{}\">{}</code>",
            html_escape(&err),
            html_escape(latex.trim())
        );
    }

    let parser = LatexParser::new(latex, &storage);
    let config = RenderConfig {
        display_mode: if display {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        // Keep the source in the output so copy/paste and assistive tech get
        // the original TeX.
        annotation: Some(latex),
        // Mid red: the renderer bakes this into the markup, so it has to be
        // legible against both the light and the dark background.
        error_color: (229, 83, 75),
        ..RenderConfig::default()
    };
    let mut out = String::new();
    match push_mathml(&mut out, parser, config) {
        Ok(()) => out,
        Err(e) => format!(
            "<code class=\"math-error\" title=\"{}\">{}</code>",
            html_escape(&e.to_string()),
            html_escape(latex.trim())
        ),
    }
}

/// First line of the first parse error, if the formula does not parse.
fn latex_error(latex: &str, storage: &Storage) -> Option<String> {
    LatexParser::new(latex, storage)
        .find_map(|ev| ev.err())
        .map(|e| {
            let msg = e.to_string();
            msg.lines().next().unwrap_or("invalid LaTeX").to_string()
        })
}

/// Replaces math nodes with pre-rendered MathML.
///
/// Comrak's math extensions only mark math up as `data-math-style` spans (they
/// assume a client-side typesetter), so we swap each one for a `Raw` node —
/// verbatim output, independent of the `unsafe` render option.
fn render_math_nodes<'a>(root: Node<'a>) {
    // Collect first: the tree is rewritten below.
    let mut targets: Vec<(Node<'a>, String, bool)> = Vec::new();
    for node in root.descendants() {
        match &node.data().value {
            NodeValue::Math(m) => targets.push((node, m.literal.clone(), m.display_math)),
            // ```math fences stay code blocks in the AST; comrak decides by
            // info string at render time.
            NodeValue::CodeBlock(cb) if info_lang(&cb.info) == "math" => {
                targets.push((node, cb.literal.trim_end_matches('\n').to_string(), true))
            }
            _ => {}
        }
    }

    for (node, latex, display) in targets {
        let mut target = node;
        // Display math standing alone gets block treatment. A paragraph holding
        // nothing but `$$...$$` is only a wrapper, so the formula replaces it
        // rather than being nested inside a <p>; display math mixed into a
        // paragraph's text has to stay inline to keep the HTML valid.
        let mut block = display;
        match node.parent() {
            Some(p) if matches!(p.data().value, NodeValue::Paragraph) => {
                if display && p.children().count() == 1 {
                    node.detach();
                    target = p;
                } else {
                    block = false;
                }
            }
            _ => {}
        }

        let mathml = latex_to_mathml(&latex, display);
        let html = if block {
            // The wrapper centers the formula and anchors equation numbers.
            format!("<div class=\"math-block\">{}</div>", mathml)
        } else {
            mathml
        };
        target.data_mut().value = NodeValue::Raw(html);
    }
}

/// First token of a code fence info string, as comrak splits it.
fn info_lang(info: &str) -> &str {
    info.split_whitespace().next().unwrap_or("")
}

/// Rewrites the LaTeX-native delimiters — `\(x\)` and `\[x\]`, KaTeX's own
/// defaults — into the dollar forms comrak understands.
///
/// This has to happen before parsing: to CommonMark, `\(` is just an escaped
/// parenthesis, so the backslashes are gone by the time there is an AST.
/// Content inside code spans and fenced code blocks is left untouched, and a
/// delimiter is only rewritten when its partner is found, so stray `\[`
/// escapes in prose keep their current meaning. Indentation-only code blocks
/// are not tracked: a `\(` in one is rewritten to `$` and shows as such. That
/// is the one case this trades away, because telling four-space code from an
/// indented paragraph inside a list needs the block structure we don't have yet.
fn expand_tex_delimiters(src: &str) -> Cow<'_, str> {
    if !src.contains("\\(") && !src.contains("\\[") {
        return Cow::Borrowed(src);
    }

    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len() + 32);
    let mut i = 0;
    let mut at_line_start = true;
    // Open fence, as (fence char, length).
    let mut fence: Option<(u8, usize)> = None;

    while i < b.len() {
        // Fenced code blocks are line-oriented; copy them through verbatim,
        // opening and closing fence lines included.
        if at_line_start {
            let marker = fence_marker(b, i);
            let in_code = match (fence, marker) {
                (Some((open_c, open_len)), Some((c, len, after))) => {
                    if c == open_c && len >= open_len && rest_of_line_blank(b, after) {
                        fence = None;
                    }
                    true
                }
                (Some(_), None) => true,
                (None, Some((c, len, _))) => {
                    fence = Some((c, len));
                    true
                }
                (None, None) => false,
            };
            if in_code {
                let end = line_end(b, i);
                out.extend_from_slice(&b[i..end]);
                i = end;
                continue; // still at a line start
            }
        }

        at_line_start = false;
        match b[i] {
            b'\n' => {
                out.push(b'\n');
                i += 1;
                at_line_start = true;
            }
            // Code span: copy through the matching backtick run.
            b'`' => {
                let n = run_len(b, i, b'`');
                let end = match find_run(b, i + n, b'`', n) {
                    Some(close) => close + n,
                    None => i + n,
                };
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            // Existing display math: copy through the closing `$$`.
            b'$' if b.get(i + 1) == Some(&b'$') => {
                let end = match find_bytes(b, i + 2, b"$$") {
                    Some(close) => close + 2,
                    None => i + 2,
                };
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'\\' => match b.get(i + 1) {
                // `\(x\)` is inline math and stays within one paragraph.
                Some(b'(') => match tex_span(src, i + 2, "\\)") {
                    Some((inner, end)) => {
                        out.push(b'$');
                        out.extend_from_slice(inner.trim().as_bytes());
                        out.push(b'$');
                        i = end;
                    }
                    None => {
                        out.extend_from_slice(&b[i..i + 2]);
                        i += 2;
                    }
                },
                // `\[x\]` is display math and may span lines.
                Some(b'[') => match tex_span(src, i + 2, "\\]") {
                    Some((inner, end)) => {
                        out.extend_from_slice(b"$$");
                        out.extend_from_slice(inner.as_bytes());
                        out.extend_from_slice(b"$$");
                        i = end;
                    }
                    None => {
                        out.extend_from_slice(&b[i..i + 2]);
                        i += 2;
                    }
                },
                // Any other escape, `\\` included, passes through as a unit so
                // its second byte is never mistaken for a delimiter.
                Some(_) => {
                    out.extend_from_slice(&b[i..i + 2]);
                    i += 2;
                }
                None => {
                    out.push(b'\\');
                    i += 1;
                }
            },
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }

    // Only whole characters and ASCII delimiters were copied, so this holds.
    match String::from_utf8(out) {
        Ok(s) => Cow::Owned(s),
        Err(_) => Cow::Borrowed(src),
    }
}

/// Content of a math span starting at `from`, plus the index just past its
/// closing delimiter.
///
/// The search stops at a blank line: display math may run over several lines,
/// but neither form crosses a paragraph, and without that limit an opener with
/// no partner would swallow text up to some unrelated closer later in the file.
/// A span containing `$` is rejected too, since rewriting it would produce
/// delimiters comrak would then mis-pair.
fn tex_span<'a>(src: &'a str, from: usize, close: &str) -> Option<(&'a str, usize)> {
    let b = src.as_bytes();
    let mut i = from;
    while i < b.len() {
        if src[i..].starts_with(close) {
            let inner = &src[from..i];
            if inner.trim().is_empty() || inner.contains('$') {
                return None;
            }
            return Some((inner, i + close.len()));
        }
        if src[i..].starts_with("\n\n") {
            return None;
        }
        // Skip escapes so `\\` before the closing bracket doesn't confuse us.
        i += if b[i] == b'\\' && i + 1 < b.len() { 2 } else { 1 };
    }
    None
}

/// Length of the run of `c` starting at `i`.
fn run_len(b: &[u8], i: usize, c: u8) -> usize {
    b[i..].iter().take_while(|&&x| x == c).count()
}

/// Start of the next run of exactly `n` occurrences of `c`, at or after `from`.
fn find_run(b: &[u8], from: usize, c: u8, n: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i] == c {
            let len = run_len(b, i, c);
            if len == n {
                return Some(i);
            }
            i += len;
        } else {
            i += 1;
        }
    }
    None
}

fn find_bytes(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..b.len().saturating_sub(needle.len() - 1)).find(|&i| &b[i..i + needle.len()] == needle)
}

fn line_end(b: &[u8], from: usize) -> usize {
    match b[from..].iter().position(|&c| c == b'\n') {
        Some(p) => from + p + 1,
        None => b.len(),
    }
}

/// Code fence marker on the line beginning at `i`, as (char, length, index
/// just past the run). `None` when the line neither opens nor closes a fence.
/// Up to three leading spaces are allowed, as in CommonMark.
fn fence_marker(b: &[u8], i: usize) -> Option<(u8, usize, usize)> {
    let mut p = i;
    while p < b.len() && p - i < 3 && b[p] == b' ' {
        p += 1;
    }
    if p < b.len() && (b[p] == b'`' || b[p] == b'~') {
        let len = run_len(b, p, b[p]);
        if len >= 3 {
            return Some((b[p], len, p + len));
        }
    }
    None
}

fn rest_of_line_blank(b: &[u8], from: usize) -> bool {
    b[from..]
        .iter()
        .take_while(|&&c| c != b'\n')
        .all(|&c| c == b' ' || c == b'\t')
}

fn md_options() -> Options<'static> {
    let mut options = Options::default();
    // GitHub Flavored Markdown.
    options.extension.strikethrough = true;
    options.extension.tagfilter = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.gfm_quirks = true;
    options.render.tasklist_classes = true;
    // GitHub extras beyond the GFM spec.
    options.extension.footnotes = true;
    options.extension.alerts = true;
    options.extension.header_id_prefix = Some(String::new());
    // Math: `$x$` / `$$x$$` and `$`x`$` / ```math fences.
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    // Local files viewed by their owner: allow embedded raw HTML, minus the
    // handful of tags GFM's tagfilter neutralizes (script, iframe, style, …).
    options.render.r#unsafe = true;
    options
}

pub fn render_markdown(hl: &Hl, src: &str) -> String {
    let options = md_options();

    let adapter = CodeAdapter { hl };
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    let src = expand_tex_delimiters(src);

    let arena = Arena::new();
    let root = parse_document(&arena, &src, &options);
    render_math_nodes(root);

    let mut out = String::with_capacity(src.len() * 3 / 2);
    match format_html_with_plugins(root, &options, &mut out, &plugins) {
        Ok(()) => out,
        Err(_) => format!("<pre>{}</pre>", html_escape(&src)),
    }
}
