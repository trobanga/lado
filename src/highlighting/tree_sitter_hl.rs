//! Tree-sitter based syntax highlighting for common languages.

use std::collections::HashMap;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use super::theme::{HighlightTheme, HIGHLIGHT_NAMES};
use super::{HighlightedLine, HighlightedSpan};

/// Maximum spans per line to prevent UI slowdown on very long lines.
const MAX_SPANS_PER_LINE: usize = 50;

/// Highlights query for Slint (from slint-ui/slint Zed editor integration, MIT licensed).
const SLINT_HIGHLIGHTS_QUERY: &str = include_str!("queries/slint-highlights.scm");

struct LangConfig {
    config: HighlightConfiguration,
}

/// Tree-sitter-based syntax highlighter supporting ~15 languages.
pub struct TreeSitterHighlighter {
    /// Extension (without dot) -> index into `configs`
    ext_to_config: HashMap<String, usize>,
    configs: Vec<LangConfig>,
}

impl TreeSitterHighlighter {
    pub fn new() -> Self {
        let mut hl = Self {
            ext_to_config: HashMap::new(),
            configs: Vec::new(),
        };

        hl.register_lang(
            &["rs"],
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        );

        hl.register_lang(
            &["py", "pyi"],
            "python",
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["js", "mjs", "cjs", "jsx"],
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        );

        hl.register_lang(
            &["ts", "mts", "cts"],
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        );

        hl.register_lang(
            &["tsx"],
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        );

        hl.register_lang(
            &["go"],
            "go",
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["c", "h"],
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["sh", "bash"],
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["java"],
            "java",
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["rb"],
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        );

        hl.register_lang(
            &["css"],
            "css",
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["html", "htm"],
            "html",
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        );

        hl.register_lang(
            &["json"],
            "json",
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["toml"],
            "toml",
            tree_sitter_toml_ng::LANGUAGE.into(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["yml", "yaml"],
            "yaml",
            tree_sitter_yaml::LANGUAGE.into(),
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl.register_lang(
            &["slint"],
            "slint",
            tree_sitter_slint::LANGUAGE.into(),
            SLINT_HIGHLIGHTS_QUERY,
            "",
            "",
        );

        hl
    }

    fn register_lang(
        &mut self,
        extensions: &[&str],
        name: &str,
        language: tree_sitter::Language,
        highlights_query: &str,
        injections_query: &str,
        locals_query: &str,
    ) {
        let mut config =
            match HighlightConfiguration::new(language, name, highlights_query, injections_query, locals_query) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Warning: Failed to load tree-sitter grammar for {name}: {e}");
                    return;
                }
            };
        config.configure(HIGHLIGHT_NAMES);
        let idx = self.configs.len();
        self.configs.push(LangConfig { config });
        for ext in extensions {
            self.ext_to_config.insert(ext.to_string(), idx);
        }
    }

    /// Whether tree-sitter can highlight files with this extension.
    pub fn can_highlight(&self, ext: &str) -> bool {
        self.ext_to_config.contains_key(ext)
    }

    /// Highlight `code` using the grammar matching `ext`, colored by `theme`.
    pub fn highlight(&self, code: &str, ext: &str, theme: &HighlightTheme) -> Vec<HighlightedLine> {
        let config_idx = match self.ext_to_config.get(ext) {
            Some(&idx) => idx,
            None => return fallback_plain(code, theme),
        };

        let mut highlighter = Highlighter::new();
        let config = &self.configs[config_idx].config;

        let events = match highlighter.highlight(config, code.as_bytes(), None, |_| None) {
            Ok(events) => events,
            Err(_) => return fallback_plain(code, theme),
        };

        let mut lines: Vec<HighlightedLine> = Vec::new();
        let mut current_spans: Vec<HighlightedSpan> = Vec::new();
        let mut style_stack: Vec<usize> = Vec::new(); // stack of highlight indices

        for event in events {
            match event {
                Ok(HighlightEvent::Source { start, end }) => {
                    let text = &code[start..end];
                    let color = style_stack
                        .last()
                        .map(|&idx| theme.color_hex(idx))
                        .unwrap_or_else(|| {
                            let c = theme.default_fg;
                            format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
                        });

                    // Split by newlines to produce one HighlightedLine per source line
                    let mut parts = text.split('\n');
                    if let Some(first) = parts.next() {
                        if !first.is_empty() && current_spans.len() < MAX_SPANS_PER_LINE {
                            current_spans.push(HighlightedSpan {
                                text: first.to_string(),
                                color: color.clone(),
                            });
                        }
                    }
                    for part in parts {
                        // End current line
                        lines.push(HighlightedLine {
                            spans: std::mem::take(&mut current_spans),
                        });
                        if !part.is_empty() && current_spans.len() < MAX_SPANS_PER_LINE {
                            current_spans.push(HighlightedSpan {
                                text: part.to_string(),
                                color: color.clone(),
                            });
                        }
                    }
                }
                Ok(HighlightEvent::HighlightStart(highlight)) => {
                    style_stack.push(highlight.0);
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    style_stack.pop();
                }
                Err(_) => {
                    // On error, bail and return what we have so far
                    break;
                }
            }
        }

        // Flush last line
        if !current_spans.is_empty() {
            lines.push(HighlightedLine {
                spans: current_spans,
            });
        }

        lines
    }
}

/// Plain fallback: one span per line with default fg color.
fn fallback_plain(code: &str, theme: &HighlightTheme) -> Vec<HighlightedLine> {
    let color = {
        let c = theme.default_fg;
        format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
    };
    code.lines()
        .map(|line| HighlightedLine {
            spans: vec![HighlightedSpan {
                text: line.to_string(),
                color: color.clone(),
            }],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlighting::theme;

    #[test]
    fn test_can_highlight_known_extensions() {
        let hl = TreeSitterHighlighter::new();
        assert!(hl.can_highlight("rs"));
        assert!(hl.can_highlight("py"));
        assert!(hl.can_highlight("js"));
        assert!(hl.can_highlight("ts"));
        assert!(hl.can_highlight("tsx"));
        assert!(hl.can_highlight("go"));
        assert!(hl.can_highlight("c"));
        assert!(hl.can_highlight("cpp"));
        assert!(hl.can_highlight("java"));
        assert!(hl.can_highlight("rb"));
        assert!(hl.can_highlight("css"));
        assert!(hl.can_highlight("html"));
        assert!(hl.can_highlight("json"));
        assert!(hl.can_highlight("toml"));
        assert!(hl.can_highlight("yml"));
        assert!(hl.can_highlight("yaml"));
        assert!(hl.can_highlight("sh"));
        assert!(hl.can_highlight("slint"));
    }

    #[test]
    fn test_cannot_highlight_unknown_extension() {
        let hl = TreeSitterHighlighter::new();
        assert!(!hl.can_highlight("xyz"));
        assert!(!hl.can_highlight("md"));
        assert!(!hl.can_highlight(""));
    }

    #[test]
    fn test_highlight_rust_produces_spans() {
        let hl = TreeSitterHighlighter::new();
        let t = theme::dark();
        let code = "fn main() {\n    println!(\"Hello\");\n}\n";
        let result = hl.highlight(code, "rs", &t);

        assert_eq!(result.len(), 3, "expected 3 lines from 3-line Rust snippet");
        // First line should have at least 'fn' and 'main' as separate spans
        assert!(
            result[0].spans.len() >= 2,
            "expected multiple spans on first line, got {}",
            result[0].spans.len()
        );
    }

    #[test]
    fn test_highlight_produces_colored_spans() {
        let hl = TreeSitterHighlighter::new();
        let t = theme::dark();
        let code = "let x = 42;\n";
        let result = hl.highlight(code, "rs", &t);

        assert!(!result.is_empty());
        // Every span should have a valid hex color
        for line in &result {
            for span in &line.spans {
                assert!(span.color.starts_with('#'), "color should be hex: {}", span.color);
                assert_eq!(span.color.len(), 7, "color should be #rrggbb: {}", span.color);
            }
        }
    }
}
