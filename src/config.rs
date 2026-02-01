//! Configuration persistence for lado settings.
//!
//! Settings are stored in `~/.config/lado/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration struct mirroring Slint's AppSettings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub ui_theme: String,
    pub syntax_theme: String,
    pub font_size: i32,
    pub tab_width: i32,
    pub line_wrap: bool,
    // Keybindings
    pub key_unified: String,
    pub key_side_by_side: String,
    pub key_scroll_down: String,
    pub key_scroll_up: String,
    pub key_file_next: String,
    pub key_file_prev: String,
    pub key_prev_commit: String,
    pub key_next_commit: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui_theme: "dark".to_string(),
            syntax_theme: "base16-ocean.dark".to_string(),
            font_size: 14,
            tab_width: 4,
            line_wrap: false,
            key_unified: "u".to_string(),
            key_side_by_side: "s".to_string(),
            key_scroll_down: "j".to_string(),
            key_scroll_up: "k".to_string(),
            key_file_next: "J".to_string(),
            key_file_prev: "K".to_string(),
            key_prev_commit: "[".to_string(),
            key_next_commit: "]".to_string(),
        }
    }
}

/// Returns the path to the config file: `~/.config/lado/config.toml`
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("lado").join("config.toml"))
}

/// Load configuration from disk. Returns default if file is missing or invalid.
pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };

    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save configuration to disk. Creates the config directory if needed.
pub fn save(config: &Config) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine config directory",
        ));
    };

    // Create directory if it doesn't exist
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.ui_theme, "dark");
        assert_eq!(config.font_size, 14);
        assert_eq!(config.tab_width, 4);
        assert!(!config.line_wrap);
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = Config {
            ui_theme: "light".to_string(),
            syntax_theme: "InspiredGitHub".to_string(),
            font_size: 16,
            tab_width: 2,
            line_wrap: true,
            key_unified: "u".to_string(),
            key_side_by_side: "s".to_string(),
            key_scroll_down: "j".to_string(),
            key_scroll_up: "k".to_string(),
            key_file_next: "J".to_string(),
            key_file_prev: "K".to_string(),
            key_prev_commit: "[".to_string(),
            key_next_commit: "]".to_string(),
        };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(config, parsed);
    }

    #[test]
    fn test_missing_fields_use_defaults() {
        // Partial config with only some fields
        let partial = r#"
            ui_theme = "solarized-dark"
        "#;

        let config: Config = toml::from_str(partial).unwrap();
        assert_eq!(config.ui_theme, "solarized-dark");
        // Other fields should be defaults
        assert_eq!(config.font_size, 14);
        assert_eq!(config.tab_width, 4);
        assert!(!config.line_wrap);
    }

    #[test]
    fn test_invalid_toml_returns_default() {
        let invalid = "this is not valid toml {{{{";
        let config: Config = toml::from_str(invalid).unwrap_or_default();
        assert_eq!(config, Config::default());
    }
}
