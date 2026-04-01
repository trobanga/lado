//! Persistence for per-file "viewed" state.
//!
//! Viewed state is keyed by diff target (branch, commit, PR number) and
//! stores a content hash per file. If the hash matches on load, the file
//! is considered still viewed. If the diff content changed, the hash
//! won't match and the file reverts to unviewed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

/// Persisted viewed state: diff_target_key -> (file_path -> content_hash)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewedState {
    targets: HashMap<String, HashMap<String, u64>>,
}

impl ViewedState {
    /// Check if a file is viewed and its content hash still matches.
    pub fn is_viewed(&self, target_key: &str, file_path: &str, current_hash: u64) -> bool {
        self.targets
            .get(target_key)
            .and_then(|files| files.get(file_path))
            .map(|&stored_hash| stored_hash == current_hash)
            .unwrap_or(false)
    }

    /// Mark a file as viewed with its current content hash.
    pub fn set_viewed(&mut self, target_key: &str, file_path: &str, content_hash: u64) {
        self.targets
            .entry(target_key.to_string())
            .or_default()
            .insert(file_path.to_string(), content_hash);
    }

    /// Remove the viewed mark for a file.
    pub fn set_unviewed(&mut self, target_key: &str, file_path: &str) {
        if let Some(files) = self.targets.get_mut(target_key) {
            files.remove(file_path);
        }
    }

    /// Load from disk. Returns default if missing or invalid.
    pub fn load() -> Self {
        let Some(path) = state_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = state_path() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, contents)
    }
}

/// Compute a content hash for a file's diff lines.
/// Uses the concatenated line content so any change invalidates the viewed state.
pub fn hash_diff_content(hunks: &[crate::git::DiffHunk]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for hunk in hunks {
        hunk.header.hash(&mut hasher);
        for line in &hunk.lines {
            line.content.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Derive a stable key from the DiffTarget for persistence.
pub fn target_key(target: &crate::cli::DiffTarget) -> String {
    match target {
        crate::cli::DiffTarget::DefaultBranch => "default-branch".to_string(),
        crate::cli::DiffTarget::Ref(r) => format!("ref:{r}"),
        crate::cli::DiffTarget::PullRequest(n) => format!("pr:{n}"),
    }
}

fn state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("lado").join("viewed_state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewed_state_roundtrip() {
        let mut state = ViewedState::default();
        state.set_viewed("ref:main", "src/app.rs", 12345);

        assert!(state.is_viewed("ref:main", "src/app.rs", 12345));
        assert!(!state.is_viewed("ref:main", "src/app.rs", 99999));
        assert!(!state.is_viewed("ref:main", "src/other.rs", 12345));
        assert!(!state.is_viewed("ref:dev", "src/app.rs", 12345));
    }

    #[test]
    fn test_unview_file() {
        let mut state = ViewedState::default();
        state.set_viewed("ref:main", "src/app.rs", 12345);
        assert!(state.is_viewed("ref:main", "src/app.rs", 12345));

        state.set_unviewed("ref:main", "src/app.rs");
        assert!(!state.is_viewed("ref:main", "src/app.rs", 12345));
    }

    #[test]
    fn test_hash_invalidates_on_content_change() {
        let mut state = ViewedState::default();
        state.set_viewed("ref:main", "src/app.rs", 100);
        assert!(!state.is_viewed("ref:main", "src/app.rs", 200));
    }

    #[test]
    fn test_serialization() {
        let mut state = ViewedState::default();
        state.set_viewed("pr:42", "README.md", 555);

        let json = serde_json::to_string(&state).unwrap();
        let loaded: ViewedState = serde_json::from_str(&json).unwrap();
        assert!(loaded.is_viewed("pr:42", "README.md", 555));
    }

    #[test]
    fn test_target_key_variants() {
        use crate::cli::DiffTarget;
        assert_eq!(target_key(&DiffTarget::DefaultBranch), "default-branch");
        assert_eq!(target_key(&DiffTarget::Ref("feature".into())), "ref:feature");
        assert_eq!(target_key(&DiffTarget::PullRequest(42)), "pr:42");
    }
}
