use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::options::Plugins;
use comrak::{markdown_to_html_with_plugins, Options};

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

pub fn render_markdown(hl: &Hl, src: &str) -> String {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.header_id_prefix = Some(String::new());
    // Local files viewed by their owner: allow embedded raw HTML.
    options.render.r#unsafe = true;

    let adapter = CodeAdapter { hl };
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    markdown_to_html_with_plugins(src, &options, &plugins)
}
