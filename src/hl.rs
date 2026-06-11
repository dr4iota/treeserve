use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

/// All generated span classes are prefixed so they cannot collide with page CSS.
/// The generated theme CSS targets the container class ".hl-code".
pub const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

pub struct Hl {
    pub ss: SyntaxSet,
    pub css_light: String,
    pub css_dark: String,
}

impl Hl {
    pub fn new() -> Hl {
        let ss = two_face::syntax::extra_newlines();
        let themes = two_face::theme::extra();
        let css_light =
            css_for_theme_with_class_style(themes.get(EmbeddedThemeName::InspiredGithub), CLASS_STYLE)
                .expect("light theme css");
        let css_dark =
            css_for_theme_with_class_style(themes.get(EmbeddedThemeName::OneHalfDark), CLASS_STYLE)
                .expect("dark theme css");
        Hl { ss, css_light, css_dark }
    }

    /// Pick a syntax for a file: extension, then full name (Makefile etc.),
    /// then shebang / first line, then plain text.
    pub fn syntax_for(&self, name: &str, ext: &str, first_line: &str) -> &SyntaxReference {
        self.ss
            .find_syntax_by_extension(ext)
            .or_else(|| self.ss.find_syntax_by_extension(name))
            .or_else(|| self.ss.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.ss.find_syntax_plain_text())
    }

    pub fn syntax_for_token(&self, token: &str) -> &SyntaxReference {
        self.ss
            .find_syntax_by_token(token)
            .unwrap_or_else(|| self.ss.find_syntax_plain_text())
    }

    /// Highlight text into class-annotated HTML spans (no surrounding <pre>).
    pub fn highlight(&self, syntax: &SyntaxReference, text: &str) -> String {
        let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, &self.ss, CLASS_STYLE);
        for line in LinesWithEndings::from(text) {
            if generator.parse_html_for_line_which_includes_newline(line).is_err() {
                // Highlighting failed mid-way; fall back to escaped plain text.
                return crate::util::html_escape(text);
            }
        }
        generator.finalize()
    }
}
