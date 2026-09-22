//! Persistence for per-file "viewed" state.
//!
//! Viewed state is keyed by diff target (branch, commit, PR number) and
//! stores a content hash per file. If the hash matches on load, the file
//! is considered still viewed. If the diff content changed, the hash
//! won't match and the file reverts to unviewed.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

/// Persisted viewed state.
///
/// `segments` is the live store: diff_target_key -> file_path -> the hashes of
/// the change segments the reviewer has marked. Whether a *file* is viewed is
/// derived from it (every segment marked), never stored, so the file checkbox
/// and the per-segment bars cannot disagree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewedState {
    /// Per-file content hashes written by versions before segment-level
    /// viewing. Kept so an existing state file still loads, and promoted to
    /// segment hashes by [`ViewedState::migrate_file`]. Never written.
    #[serde(default)]
    targets: HashMap<String, HashMap<String, u64>>,
    #[serde(default)]
    segments: HashMap<String, HashMap<String, HashSet<u64>>>,
}

impl ViewedState {
    /// Mark one change segment as viewed.
    pub fn set_segment_viewed(&mut self, target_key: &str, file_path: &str, hash: u64) {
        self.segments
            .entry(target_key.to_string())
            .or_default()
            .entry(file_path.to_string())
            .or_default()
            .insert(hash);
    }

    /// Drop the mark on one change segment.
    pub fn set_segment_unviewed(&mut self, target_key: &str, file_path: &str, hash: u64) {
        if let Some(marked) = self
            .segments
            .get_mut(target_key)
            .and_then(|files| files.get_mut(file_path))
        {
            marked.remove(&hash);
        }
    }

    pub fn is_segment_viewed(&self, target_key: &str, file_path: &str, hash: u64) -> bool {
        self.file_segments(target_key, file_path)
            .is_some_and(|marked| marked.contains(&hash))
    }

    /// Mark every segment of a file, which is what ticking the file checkbox
    /// means (D4).
    pub fn set_file_viewed(&mut self, target_key: &str, file_path: &str, hashes: &[u64]) {
        let marked = self
            .segments
            .entry(target_key.to_string())
            .or_default()
            .entry(file_path.to_string())
            .or_default();
        marked.clear();
        marked.extend(hashes.iter().copied());
    }

    /// Write a legacy per-file mark, as a version before segment-level viewing
    /// would have. Only the migration path reads this shape, so nothing but a
    /// test needs to produce it.
    #[cfg(test)]
    pub fn set_legacy_file_viewed_for_test(
        &mut self,
        target_key: &str,
        file_path: &str,
        file_hash: u64,
    ) {
        self.targets
            .entry(target_key.to_string())
            .or_default()
            .insert(file_path.to_string(), file_hash);
    }

    /// Promote a legacy per-file mark to segment marks, once, on first sight of
    /// the file's segments.
    ///
    /// A state file written before segment-level viewing records only that the
    /// whole file was read at content hash `file_hash`. That is exactly "every
    /// segment viewed", provided the file has not changed since. The legacy
    /// entry is consumed either way, so this runs once per file and a reviewer
    /// who has already marked something here is left alone.
    pub fn migrate_file(
        &mut self,
        target_key: &str,
        file_path: &str,
        file_hash: u64,
        hashes: &[u64],
    ) {
        let Some(legacy_hash) = self
            .targets
            .get_mut(target_key)
            .and_then(|files| files.remove(file_path))
        else {
            return;
        };
        if legacy_hash != file_hash {
            return;
        }
        if self.file_segments(target_key, file_path).is_some() {
            return;
        }
        self.set_file_viewed(target_key, file_path, hashes);
    }

    /// Drop every segment mark on a file, which is what clearing the file
    /// checkbox means (D4). Also clears the legacy entry, so a file that was
    /// only ever marked by an older version does not come back viewed.
    pub fn set_file_unviewed(&mut self, target_key: &str, file_path: &str) {
        if let Some(files) = self.segments.get_mut(target_key) {
            files.remove(file_path);
        }
        if let Some(files) = self.targets.get_mut(target_key) {
            files.remove(file_path);
        }
    }

    /// Whether a file's every segment is marked. A file with no segments at all
    /// is not viewed — there is nothing to have reviewed.
    pub fn is_file_viewed(&self, target_key: &str, file_path: &str, hashes: &[u64]) -> bool {
        if hashes.is_empty() {
            return false;
        }
        let Some(marked) = self.file_segments(target_key, file_path) else {
            return false;
        };
        hashes.iter().all(|h| marked.contains(h))
    }

    fn file_segments(&self, target_key: &str, file_path: &str) -> Option<&HashSet<u64>> {
        self.segments.get(target_key)?.get(file_path)
    }
}

impl ViewedState {
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
    fn a_file_is_viewed_only_when_every_segment_is() {
        let mut state = ViewedState::default();
        state.set_segment_viewed("ref:main", "src/app.rs", 1);

        assert!(!state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));

        state.set_segment_viewed("ref:main", "src/app.rs", 2);

        assert!(state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));
    }

    #[test]
    fn unviewing_one_segment_unchecks_a_fully_viewed_file() {
        let mut state = ViewedState::default();
        state.set_file_viewed("ref:main", "src/app.rs", &[1, 2]);
        assert!(state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));

        state.set_segment_unviewed("ref:main", "src/app.rs", 1);

        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 1));
        assert!(state.is_segment_viewed("ref:main", "src/app.rs", 2));
        assert!(!state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));
    }

    #[test]
    fn unchecking_a_file_drops_every_segment_mark() {
        let mut state = ViewedState::default();
        state.set_file_viewed("ref:main", "src/app.rs", &[1, 2, 3]);

        state.set_file_unviewed("ref:main", "src/app.rs");

        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 1));
        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 2));
        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 3));
        assert!(!state.is_file_viewed("ref:main", "src/app.rs", &[1, 2, 3]));
    }

    /// State written before segment-level viewing existed: one content hash per
    /// file, and no `segments` key at all.
    const LEGACY_STATE: &str = r#"{"targets":{"ref:main":{"src/app.rs":777}}}"#;

    #[test]
    fn a_state_file_from_before_segments_keeps_its_marks() {
        let mut state: ViewedState = serde_json::from_str(LEGACY_STATE).unwrap();

        // The file's content hash still matches, so everything in it was read.
        state.migrate_file("ref:main", "src/app.rs", 777, &[1, 2]);

        assert!(state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));
    }

    #[test]
    fn a_legacy_mark_does_not_survive_a_change_to_the_file() {
        let mut state: ViewedState = serde_json::from_str(LEGACY_STATE).unwrap();

        state.migrate_file("ref:main", "src/app.rs", 888, &[1, 2]);

        assert!(!state.is_file_viewed("ref:main", "src/app.rs", &[1, 2]));
    }

    #[test]
    fn migration_never_overwrites_marks_the_reviewer_already_made() {
        let mut state: ViewedState = serde_json::from_str(LEGACY_STATE).unwrap();
        state.set_segment_viewed("ref:main", "src/app.rs", 1);

        state.migrate_file("ref:main", "src/app.rs", 777, &[1, 2]);

        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 2));
    }

    #[test]
    fn segment_marks_survive_a_round_trip_through_disk() {
        let mut state = ViewedState::default();
        state.set_segment_viewed("pr:42", "README.md", 555);

        let json = serde_json::to_string(&state).unwrap();
        let loaded: ViewedState = serde_json::from_str(&json).unwrap();

        assert!(loaded.is_segment_viewed("pr:42", "README.md", 555));
    }

    #[test]
    fn a_mark_belongs_to_one_file_of_one_diff_target() {
        let mut state = ViewedState::default();
        state.set_segment_viewed("ref:main", "src/app.rs", 555);

        assert!(state.is_segment_viewed("ref:main", "src/app.rs", 555));
        // Not another segment, another file, or the same file on another diff.
        assert!(!state.is_segment_viewed("ref:main", "src/app.rs", 999));
        assert!(!state.is_segment_viewed("ref:main", "src/other.rs", 555));
        assert!(!state.is_segment_viewed("ref:dev", "src/app.rs", 555));
    }

    #[test]
    fn test_target_key_variants() {
        use crate::cli::DiffTarget;
        assert_eq!(target_key(&DiffTarget::DefaultBranch), "default-branch");
        assert_eq!(target_key(&DiffTarget::Ref("feature".into())), "ref:feature");
        assert_eq!(target_key(&DiffTarget::PullRequest(42)), "pr:42");
    }
}
