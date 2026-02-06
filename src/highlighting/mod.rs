mod syntax;
pub mod theme;
mod tree_sitter_hl;

use syntax::SyntaxHighlighter;
use theme::HighlightTheme;
use tree_sitter_hl::TreeSitterHighlighter;

/// A highlighted line consisting of styled spans.
#[derive(Debug, Clone)]
pub struct HighlightedLine {
    pub spans: Vec<HighlightedSpan>,
}

/// A span of text with color.
#[derive(Debug, Clone)]
pub struct HighlightedSpan {
    pub text: String,
    pub color: String,
}

/// Unified highlighter: tree-sitter for supported languages, syntect as fallback.
pub struct Highlighter {
    tree_sitter: TreeSitterHighlighter,
    syntect: SyntaxHighlighter,
    current_theme: HighlightTheme,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            tree_sitter: TreeSitterHighlighter::new(),
            syntect: SyntaxHighlighter::new(),
            current_theme: theme::dark(),
        }
    }

    /// Update the color theme. `ui_theme` is the UI theme name
    /// (e.g. "dark", "light", "solarized-dark", "solarized-light").
    pub fn set_theme(&mut self, ui_theme: &str) {
        self.current_theme = theme::theme_for_ui(ui_theme);

        // Also update syntect theme for fallback
        let syntect_theme = match ui_theme {
            "light" => "InspiredGitHub",
            "solarized-light" => "Doom Solarized Light",
            "solarized-dark" => "Solarized (dark)",
            _ => "base16-ocean.dark",
        };
        self.syntect.set_theme(syntect_theme);
    }

    /// Highlight `code` for the file at `file_path`.
    /// Uses tree-sitter when available, syntect otherwise.
    pub fn highlight(&self, code: &str, file_path: &str) -> Vec<HighlightedLine> {
        let ext = file_path.rsplit('.').next().unwrap_or("");
        if self.tree_sitter.can_highlight(ext) {
            self.tree_sitter.highlight(code, ext, &self.current_theme)
        } else {
            self.syntect.highlight(code, file_path)
        }
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_sitter_used_for_rust() {
        let hl = Highlighter::new();
        let code = "fn main() {\n    let x: i32 = 42;\n}\n";
        let result = hl.highlight(code, "src/main.rs");

        assert_eq!(result.len(), 3);
        // Tree-sitter produces richer spans than syntect
        assert!(result[0].spans.len() >= 2);
    }

    #[test]
    fn test_syntect_fallback_for_unknown_ext() {
        let hl = Highlighter::new();
        let code = "some text\n";
        let result = hl.highlight(code, "file.rkt");

        assert!(!result.is_empty());
    }

    #[test]
    fn test_theme_switching() {
        let mut hl = Highlighter::new();
        let code = "fn test() {}\n";

        let dark_result = hl.highlight(code, "test.rs");
        hl.set_theme("light");
        let light_result = hl.highlight(code, "test.rs");

        // Both should produce output, but colors should differ
        assert!(!dark_result.is_empty());
        assert!(!light_result.is_empty());
        // At least one span color should differ between themes
        let dark_colors: Vec<&str> = dark_result[0].spans.iter().map(|s| s.color.as_str()).collect();
        let light_colors: Vec<&str> = light_result[0].spans.iter().map(|s| s.color.as_str()).collect();
        assert_ne!(dark_colors, light_colors, "dark and light themes should produce different colors");
    }
}
