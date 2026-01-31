use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Syntax highlighter using syntect
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Highlight a code snippet and return styled spans
    pub fn highlight(&self, code: &str, file_path: &str) -> Vec<HighlightedLine> {
        let extension = file_path
            .rsplit('.')
            .next()
            .unwrap_or("");

        let syntax = self
            .syntax_set
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        let mut result = Vec::new();

        for line in LinesWithEndings::from(code) {
            let ranges: Vec<(Style, &str)> = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_default();

            let spans: Vec<HighlightedSpan> = ranges
                .into_iter()
                .map(|(style, text)| HighlightedSpan {
                    text: text.to_string(),
                    color: format!(
                        "#{:02x}{:02x}{:02x}",
                        style.foreground.r, style.foreground.g, style.foreground.b
                    ),
                })
                .collect();

            result.push(HighlightedLine { spans });
        }

        result
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// A highlighted line consisting of styled spans
#[derive(Debug, Clone)]
pub struct HighlightedLine {
    pub spans: Vec<HighlightedSpan>,
}

/// A span of text with color
#[derive(Debug, Clone)]
pub struct HighlightedSpan {
    pub text: String,
    pub color: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust() {
        let highlighter = SyntaxHighlighter::new();
        let code = "fn main() {\n    println!(\"Hello\");\n}\n";
        let result = highlighter.highlight(code, "test.rs");

        assert_eq!(result.len(), 3);
        assert!(!result[0].spans.is_empty());
    }

    #[test]
    fn test_highlight_unknown_extension() {
        let highlighter = SyntaxHighlighter::new();
        let code = "some random text\n";
        let result = highlighter.highlight(code, "file.xyz");

        assert_eq!(result.len(), 1);
    }
}
