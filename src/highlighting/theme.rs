//! Color mappings from tree-sitter highlight names to Slint colors per UI theme.

use slint::Color;

/// Standard highlight capture names used across all tree-sitter grammars.
/// The order here determines the index used in `HighlightTheme::colors`.
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "function.macro",
    "keyword",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Colors indexed parallel to `HIGHLIGHT_NAMES`.
pub struct HighlightTheme {
    pub colors: Vec<Color>,
    /// Default foreground when no highlight applies
    pub default_fg: Color,
}

impl HighlightTheme {
    pub fn color_for(&self, highlight_index: usize) -> Color {
        self.colors
            .get(highlight_index)
            .copied()
            .unwrap_or(self.default_fg)
    }

    pub fn color_hex(&self, highlight_index: usize) -> String {
        let c = self.color_for(highlight_index);
        format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb_u8(r, g, b)
}

/// Dark theme — base16-ocean-inspired palette for dark backgrounds.
pub fn dark() -> HighlightTheme {
    HighlightTheme {
        default_fg: rgb(0xe4, 0xe4, 0xe7),
        colors: vec![
            rgb(0xd0, 0x8c, 0x60), // attribute — warm brown
            rgb(0x6b, 0x6b, 0x73), // comment — muted gray
            rgb(0xd1, 0x9a, 0x66), // constant — orange
            rgb(0xd1, 0x9a, 0x66), // constant.builtin — orange
            rgb(0xe5, 0xc0, 0x7b), // constructor — gold
            rgb(0xe4, 0xe4, 0xe7), // embedded — default fg
            rgb(0x61, 0xaf, 0xef), // function — blue
            rgb(0x56, 0xb6, 0xc2), // function.builtin — cyan
            rgb(0xc6, 0x78, 0xdd), // function.macro — purple
            rgb(0xc6, 0x78, 0xdd), // keyword — purple
            rgb(0xe5, 0xc0, 0x7b), // label — gold
            rgb(0xe5, 0xc0, 0x7b), // module — gold
            rgb(0xd1, 0x9a, 0x66), // number — orange
            rgb(0x56, 0xb6, 0xc2), // operator — cyan
            rgb(0xe0, 0x6c, 0x75), // property — red
            rgb(0xa0, 0xa0, 0xa6), // punctuation — secondary text
            rgb(0xa0, 0xa0, 0xa6), // punctuation.bracket — secondary text
            rgb(0xa0, 0xa0, 0xa6), // punctuation.delimiter — secondary text
            rgb(0x56, 0xb6, 0xc2), // punctuation.special — cyan
            rgb(0x98, 0xc3, 0x79), // string — green
            rgb(0xd1, 0x9a, 0x66), // string.escape — orange
            rgb(0x56, 0xb6, 0xc2), // string.special — cyan
            rgb(0xe0, 0x6c, 0x75), // tag — red
            rgb(0xe5, 0xc0, 0x7b), // type — gold
            rgb(0xe5, 0xc0, 0x7b), // type.builtin — gold
            rgb(0xe4, 0xe4, 0xe7), // variable — default fg
            rgb(0xe0, 0x6c, 0x75), // variable.builtin — red
            rgb(0xe4, 0xe4, 0xe7), // variable.parameter — default fg
        ],
    }
}

/// Light theme — GitHub-inspired palette for light backgrounds.
pub fn light() -> HighlightTheme {
    HighlightTheme {
        default_fg: rgb(0x1f, 0x23, 0x28),
        colors: vec![
            rgb(0x95, 0x3b, 0x00), // attribute — dark orange
            rgb(0x8c, 0x95, 0x9f), // comment — muted gray
            rgb(0x09, 0x50, 0xae), // constant — blue
            rgb(0x09, 0x50, 0xae), // constant.builtin — blue
            rgb(0x8c, 0x6c, 0x00), // constructor — dark yellow
            rgb(0x1f, 0x23, 0x28), // embedded — default fg
            rgb(0x6f, 0x42, 0xc1), // function — purple
            rgb(0x6f, 0x42, 0xc1), // function.builtin — purple
            rgb(0x6f, 0x42, 0xc1), // function.macro — purple
            rgb(0xcf, 0x22, 0x2e), // keyword — red
            rgb(0x09, 0x50, 0xae), // label — blue
            rgb(0x95, 0x3b, 0x00), // module — dark orange
            rgb(0x09, 0x50, 0xae), // number — blue
            rgb(0x1f, 0x23, 0x28), // operator — fg
            rgb(0x09, 0x69, 0xda), // property — bright blue
            rgb(0x57, 0x60, 0x6a), // punctuation — secondary
            rgb(0x57, 0x60, 0x6a), // punctuation.bracket — secondary
            rgb(0x57, 0x60, 0x6a), // punctuation.delimiter — secondary
            rgb(0xcf, 0x22, 0x2e), // punctuation.special — red
            rgb(0x0a, 0x30, 0x69), // string — dark blue
            rgb(0x09, 0x50, 0xae), // string.escape — blue
            rgb(0x09, 0x50, 0xae), // string.special — blue
            rgb(0x1a, 0x7f, 0x37), // tag — green
            rgb(0x8c, 0x6c, 0x00), // type — dark yellow
            rgb(0x8c, 0x6c, 0x00), // type.builtin — dark yellow
            rgb(0x1f, 0x23, 0x28), // variable — default fg
            rgb(0x09, 0x50, 0xae), // variable.builtin — blue
            rgb(0x1f, 0x23, 0x28), // variable.parameter — default fg
        ],
    }
}

/// Solarized dark — classic Solarized accent colors on base03 background.
pub fn solarized_dark() -> HighlightTheme {
    // Solarized accents:
    // yellow=#b58900, orange=#cb4b16, red=#dc322f, magenta=#d33682,
    // violet=#6c71c4, blue=#268bd2, cyan=#2aa198, green=#859900
    HighlightTheme {
        default_fg: rgb(0x83, 0x94, 0x96), // base0
        colors: vec![
            rgb(0xcb, 0x4b, 0x16), // attribute — orange
            rgb(0x58, 0x6e, 0x75), // comment — base01
            rgb(0x2a, 0xa1, 0x98), // constant — cyan
            rgb(0x2a, 0xa1, 0x98), // constant.builtin — cyan
            rgb(0xb5, 0x89, 0x00), // constructor — yellow
            rgb(0x83, 0x94, 0x96), // embedded — base0
            rgb(0x26, 0x8b, 0xd2), // function — blue
            rgb(0x26, 0x8b, 0xd2), // function.builtin — blue
            rgb(0xd3, 0x36, 0x82), // function.macro — magenta
            rgb(0x85, 0x99, 0x00), // keyword — green
            rgb(0xb5, 0x89, 0x00), // label — yellow
            rgb(0xb5, 0x89, 0x00), // module — yellow
            rgb(0xd3, 0x36, 0x82), // number — magenta
            rgb(0x83, 0x94, 0x96), // operator — base0
            rgb(0x26, 0x8b, 0xd2), // property — blue
            rgb(0x65, 0x7b, 0x83), // punctuation — base00
            rgb(0x65, 0x7b, 0x83), // punctuation.bracket — base00
            rgb(0x65, 0x7b, 0x83), // punctuation.delimiter — base00
            rgb(0xcb, 0x4b, 0x16), // punctuation.special — orange
            rgb(0x2a, 0xa1, 0x98), // string — cyan
            rgb(0xcb, 0x4b, 0x16), // string.escape — orange
            rgb(0xcb, 0x4b, 0x16), // string.special — orange
            rgb(0xdc, 0x32, 0x2f), // tag — red
            rgb(0xb5, 0x89, 0x00), // type — yellow
            rgb(0xb5, 0x89, 0x00), // type.builtin — yellow
            rgb(0x83, 0x94, 0x96), // variable — base0
            rgb(0xcb, 0x4b, 0x16), // variable.builtin — orange
            rgb(0x83, 0x94, 0x96), // variable.parameter — base0
        ],
    }
}

/// Solarized light — doom-solarized-light-inspired accent colors on base3 background.
pub fn solarized_light() -> HighlightTheme {
    // Same Solarized accents, light mode uses base3/base2 for backgrounds
    // and base00/base01 for body text.
    HighlightTheme {
        default_fg: rgb(0x55, 0x6b, 0x72), // fg from doom-solarized-light
        colors: vec![
            rgb(0xcb, 0x4b, 0x16), // attribute — orange
            rgb(0x96, 0xa7, 0xa9), // comment — base6 (muted)
            rgb(0x2a, 0xa1, 0x98), // constant — cyan
            rgb(0x2a, 0xa1, 0x98), // constant.builtin — cyan
            rgb(0xb5, 0x89, 0x00), // constructor — yellow
            rgb(0x55, 0x6b, 0x72), // embedded — fg
            rgb(0x26, 0x8b, 0xd2), // function — blue
            rgb(0x26, 0x8b, 0xd2), // function.builtin — blue
            rgb(0xd3, 0x36, 0x82), // function.macro — magenta
            rgb(0x85, 0x99, 0x00), // keyword — green
            rgb(0xb5, 0x89, 0x00), // label — yellow
            rgb(0xb5, 0x89, 0x00), // module — yellow
            rgb(0xd3, 0x36, 0x82), // number — magenta
            rgb(0x55, 0x6b, 0x72), // operator — fg
            rgb(0x26, 0x8b, 0xd2), // property — blue
            rgb(0x78, 0x84, 0x84), // punctuation — base7
            rgb(0x78, 0x84, 0x84), // punctuation.bracket — base7
            rgb(0x78, 0x84, 0x84), // punctuation.delimiter — base7
            rgb(0xcb, 0x4b, 0x16), // punctuation.special — orange
            rgb(0x2a, 0xa1, 0x98), // string — cyan
            rgb(0xcb, 0x4b, 0x16), // string.escape — orange
            rgb(0xcb, 0x4b, 0x16), // string.special — orange
            rgb(0xdc, 0x32, 0x2f), // tag — red
            rgb(0xb5, 0x89, 0x00), // type — yellow
            rgb(0xb5, 0x89, 0x00), // type.builtin — yellow
            rgb(0x55, 0x6b, 0x72), // variable — fg
            rgb(0xcb, 0x4b, 0x16), // variable.builtin — orange
            rgb(0x55, 0x6b, 0x72), // variable.parameter — fg
        ],
    }
}

/// Resolve a UI theme name to the corresponding `HighlightTheme`.
pub fn theme_for_ui(ui_theme: &str) -> HighlightTheme {
    match ui_theme {
        "light" => light(),
        "solarized-dark" => solarized_dark(),
        "solarized-light" => solarized_light(),
        _ => dark(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_colors_match_highlight_names_count() {
        for (name, theme) in [
            ("dark", dark()),
            ("light", light()),
            ("solarized_dark", solarized_dark()),
            ("solarized_light", solarized_light()),
        ] {
            assert_eq!(
                theme.colors.len(),
                HIGHLIGHT_NAMES.len(),
                "theme '{}' has {} colors but HIGHLIGHT_NAMES has {} entries",
                name,
                theme.colors.len(),
                HIGHLIGHT_NAMES.len(),
            );
        }
    }
}
