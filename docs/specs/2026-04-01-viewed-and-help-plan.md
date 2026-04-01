# Viewed Checkbox & Help Popup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-file "viewed" checkbox with persistent state and J/K skip behavior, plus a centered help overlay triggered by `?`.

**Architecture:** Both features thread through the existing Slint ↔ Rust callback pattern. The viewed state adds a `viewed` field to the FileEntry pipeline (structs.slint → FlatFileEntry → FileEntryModel → FileEntry) and persists via a new `viewed_state` module using JSON with content hashing for invalidation. The help overlay is a self-contained Slint component with no Rust-side state.

**Tech Stack:** Slint 1.14, Rust (serde_json for persistence, std hash for content hashing)

---

### Task 1: Add `viewed` field to data pipeline

**Files:**
- Modify: `ui/structs.slint:7-15`
- Modify: `src/git/file_tree.rs:183-192` (FlatFileEntry struct)
- Modify: `src/git/file_tree.rs:148-180` (flatten_tree_with_state)
- Modify: `src/models/file_tree_model.rs:1-41`

- [ ] **Step 1: Add `viewed` to Slint FileEntry struct**

In `ui/structs.slint`, add `viewed: bool` to the `FileEntry` struct:

```slint
export struct FileEntry {
    name: string,
    path: string,
    depth: int,
    is-folder: bool,
    is-expanded: bool,
    status: string,
    comment-count: int,
    viewed: bool,
}
```

- [ ] **Step 2: Add `viewed` to FlatFileEntry**

In `src/git/file_tree.rs`, add the field to the struct:

```rust
#[derive(Debug, Clone)]
pub struct FlatFileEntry {
    pub name: String,
    pub path: String,
    pub depth: i32,
    pub is_folder: bool,
    pub is_expanded: bool,
    pub status: String,
    pub comment_count: i32,
    pub viewed: bool,
}
```

And update `flatten_tree_with_state` to initialize it to `false`:

```rust
result.push(FlatFileEntry {
    name: node.name.clone(),
    path: node.path.clone(),
    depth,
    is_folder: node.is_folder,
    is_expanded,
    status: node.status.clone().unwrap_or_else(|| "modified".to_string()),
    comment_count: 0,
    viewed: false,
});
```

- [ ] **Step 3: Add `viewed` to FileEntryModel and conversions**

In `src/models/file_tree_model.rs`:

```rust
pub struct FileEntryModel {
    pub name: String,
    pub path: String,
    pub depth: i32,
    pub is_folder: bool,
    pub is_expanded: bool,
    pub status: String,
    pub comment_count: i32,
    pub viewed: bool,
}

impl From<&FlatFileEntry> for FileEntryModel {
    fn from(entry: &FlatFileEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone(),
            depth: entry.depth,
            is_folder: entry.is_folder,
            is_expanded: entry.is_expanded,
            status: entry.status.clone(),
            comment_count: entry.comment_count,
            viewed: entry.viewed,
        }
    }
}

impl From<FileEntryModel> for FileEntry {
    fn from(model: FileEntryModel) -> Self {
        Self {
            name: model.name.into(),
            path: model.path.into(),
            depth: model.depth,
            is_folder: model.is_folder,
            is_expanded: model.is_expanded,
            status: model.status.into(),
            comment_count: model.comment_count,
            viewed: model.viewed,
        }
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles successfully (the new field defaults to `false` everywhere)

- [ ] **Step 5: Run existing tests to confirm no regression**

Run: `cargo test 2>&1 | tail -20`
Expected: All existing tests pass

- [ ] **Step 6: Commit**

```bash
git add ui/structs.slint src/git/file_tree.rs src/models/file_tree_model.rs
git commit -m "feat(lado): add viewed field to file entry data pipeline"
```

---

### Task 2: Viewed state persistence module

**Files:**
- Create: `src/viewed_state.rs`
- Modify: `src/main.rs:1-8` (add module declaration)

- [ ] **Step 1: Write test for ViewedState**

Create `src/viewed_state.rs` with tests first:

```rust
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
        assert!(!state.is_viewed("ref:main", "src/app.rs", 99999)); // different hash
        assert!(!state.is_viewed("ref:main", "src/other.rs", 12345)); // different file
        assert!(!state.is_viewed("ref:dev", "src/app.rs", 12345)); // different target
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
        // Simulates file content changing — new hash won't match
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
```

- [ ] **Step 2: Add module declaration**

In `src/main.rs`, add `mod viewed_state;` after `mod ui;`:

```rust
mod app;
mod cli;
mod config;
mod git;
mod github;
mod highlighting;
mod models;
mod ui;
mod viewed_state;
```

- [ ] **Step 3: Run tests**

Run: `cargo test viewed_state 2>&1`
Expected: All 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/viewed_state.rs src/main.rs
git commit -m "feat(lado): add viewed state persistence module with content hashing"
```

---

### Task 3: Integrate viewed state into App

**Files:**
- Modify: `src/app.rs`

This task wires up the viewed state: load on startup, apply to file entries, add toggle callback, modify find_next_file to skip viewed files, persist on exit.

- [ ] **Step 1: Add viewed state fields to App struct**

In `src/app.rs`, add to the `App` struct:

```rust
use crate::viewed_state::{self, ViewedState};

pub struct App {
    window: MainWindow,
    repo: Rc<Repository>,
    target: DiffTarget,
    diff_data: Rc<RefCell<Option<DiffData>>>,
    pr_comments: Rc<RefCell<Option<FileComments>>>,
    pr_commits: Rc<RefCell<Vec<PrCommit>>>,
    all_pr_comments: Rc<RefCell<Vec<github::PrComment>>>,
    pr_base_ref: Rc<RefCell<Option<String>>>,
    pr_head_ref: Rc<RefCell<Option<String>>>,
    highlighter: Rc<RefCell<Highlighter>>,
    file_tree: Rc<RefCell<Vec<FileTreeNode>>>,
    expanded_state: Rc<RefCell<HashMap<String, bool>>>,
    viewed_state: Rc<RefCell<ViewedState>>,
    target_key: String,
}
```

- [ ] **Step 2: Initialize viewed state in App::new**

In `App::new`, after creating the highlighter, load the viewed state:

```rust
let viewed_state = Rc::new(RefCell::new(ViewedState::load()));
let target_key = viewed_state::target_key(&target);
```

And add them to the `App` struct initialization:

```rust
let app = Self {
    window,
    repo,
    target,
    diff_data: Rc::new(RefCell::new(None)),
    pr_comments: Rc::new(RefCell::new(None)),
    pr_commits: Rc::new(RefCell::new(Vec::new())),
    all_pr_comments: Rc::new(RefCell::new(Vec::new())),
    pr_base_ref: Rc::new(RefCell::new(None)),
    pr_head_ref: Rc::new(RefCell::new(None)),
    highlighter: Rc::new(RefCell::new(highlighter)),
    file_tree: Rc::new(RefCell::new(Vec::new())),
    expanded_state: Rc::new(RefCell::new(HashMap::new())),
    viewed_state,
    target_key,
};
```

- [ ] **Step 3: Update `build_file_entries` to apply viewed state**

Modify the `build_file_entries` function to accept viewed state and diff data for hashing:

```rust
fn build_file_entries(
    flat_entries: &[crate::git::FlatFileEntry],
    pr_comments: Option<&FileComments>,
    diff_data: Option<&DiffData>,
    viewed_state: Option<(&ViewedState, &str)>,
) -> Vec<FileEntry> {
    flat_entries
        .iter()
        .map(|f| {
            let mut model = FileEntryModel::from(f);
            if let (Some(comments), Some(data)) = (pr_comments, diff_data) {
                if let Some(file_comments) = comments.get(&f.path) {
                    let hunks = data.file_hunks.get(&f.path);
                    model.comment_count = match hunks {
                        Some(h) => count_matching_comments(h, file_comments),
                        None => 0,
                    };
                }
            }
            // Apply viewed state from persistence
            if let Some((vs, target_key)) = viewed_state {
                if !f.is_folder {
                    if let Some(data) = diff_data {
                        let hash = data.file_hunks.get(&f.path)
                            .map(|h| viewed_state::hash_diff_content(h))
                            .unwrap_or(0);
                        model.viewed = vs.is_viewed(target_key, &f.path, hash);
                    }
                }
            }
            model.into()
        })
        .collect()
}
```

- [ ] **Step 4: Update all call sites of `build_file_entries`**

Every call to `build_file_entries` in `app.rs` needs the two new arguments. There are 7 call sites:

In `load_diff` (~line 603):
```rust
let file_entries = build_file_entries(
    &flat_entries,
    self.pr_comments.borrow().as_ref(),
    Some(&diff_data),
    Some((&self.viewed_state.borrow(), &self.target_key)),
);
```

In `on_folder_toggled` (~line 199):
```rust
let file_entries = build_file_entries(
    &flat_entries,
    pr_comments.borrow().as_ref(),
    diff_data.borrow().as_ref(),
    Some((&viewed_state.borrow(), &target_key)),
);
```

Repeat for `on_expand_all_directories`, `on_collapse_all_directories`, `on_toggle_focused_directory`, `on_expand_focused_recursive` — all follow the same pattern.

For `on_commit_selected` (~line 304), pass `None` for viewed state since commit-specific views don't use persistence:
```rust
let file_entries = build_file_entries(&flat_entries, grouped_comments.as_ref(), Some(&diff_data), None);
```

Each of these callbacks that captures variables needs to also capture `viewed_state` and `target_key`:
```rust
let viewed_state = Rc::clone(&self.viewed_state);
let target_key = self.target_key.clone();
```

- [ ] **Step 5: Add toggle-viewed callback**

Add a new callback declaration in `main.slint` (in the callbacks section around line 69):
```slint
callback toggle-viewed(/* index */ int);
```

Then register the callback in `setup_callbacks` in `app.rs`:

```rust
// Toggle viewed state callback
let window_weak = self.window.as_weak();
let viewed_state = Rc::clone(&self.viewed_state);
let target_key = self.target_key.clone();
let diff_data = Rc::clone(&self.diff_data);
self.window.on_toggle_viewed(move |idx| {
    let window = window_weak.unwrap();
    let files = window.get_files();

    if let Some(entry) = files.row_data(idx as usize) {
        if entry.is_folder {
            return;
        }

        let path = entry.path.to_string();
        let mut vs = viewed_state.borrow_mut();
        let tk = &target_key;

        if entry.viewed {
            // Unmark as viewed
            vs.set_unviewed(tk, &path);
        } else {
            // Mark as viewed with content hash
            let data = diff_data.borrow();
            let hash = data.as_ref()
                .and_then(|d| d.file_hunks.get(&path))
                .map(|h| viewed_state::hash_diff_content(h))
                .unwrap_or(0);
            vs.set_viewed(tk, &path, hash);
        }

        // Persist
        if let Err(e) = vs.save() {
            eprintln!("Warning: Could not save viewed state: {}", e);
        }

        // Toggle in the UI model directly
        let model = files.as_any().downcast_ref::<VecModel<FileEntry>>().unwrap();
        let mut updated = entry.clone();
        updated.viewed = !entry.viewed;
        model.set_row_data(idx as usize, updated);
    }
});
```

- [ ] **Step 6: Modify `find_next_file` to skip viewed files**

Update the existing `on_find_next_file` callback:

```rust
self.window.on_find_next_file(move |current_idx, direction| {
    let window = window_weak.unwrap();
    let files = window.get_files();
    let len = files.row_count() as i32;

    if len == 0 {
        return -1;
    }

    let mut idx = current_idx + direction;
    while idx >= 0 && idx < len {
        if let Some(file) = files.row_data(idx as usize) {
            if !file.is_folder && !file.viewed {
                return idx;
            }
        }
        idx += direction;
    }
    // No unviewed file found in that direction, return current
    if current_idx >= 0 && current_idx < len {
        current_idx
    } else {
        -1
    }
});
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles successfully

- [ ] **Step 8: Run all tests**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/app.rs ui/main.slint
git commit -m "feat(lado): integrate viewed state into app with toggle callback and J/K skip"
```

---

### Task 4: Viewed checkbox UI in file tree

**Files:**
- Modify: `ui/components/file_tree.slint`
- Modify: `ui/main.slint`

- [ ] **Step 1: Add viewed property and checkbox to TreeItem**

In `ui/components/file_tree.slint`, add to `TreeItem`:

Add property:
```slint
in property <bool> viewed: false;
```

Add callback:
```slint
callback toggle-viewed;
```

In the `HorizontalLayout` inside `TreeItem`, add a checkmark after the file/folder icon rectangle (before the Name Text), and only for non-folders:

```slint
// Viewed checkbox for files
if !is-folder: Rectangle {
    width: 16px;
    height: 16px;
    border-radius: 3px;
    border-width: 1px;
    border-color: viewed ? theme.status-added : theme.border-normal;
    background: viewed ? theme.status-added.with-alpha(0.2) : transparent;

    Text {
        text: viewed ? "✓" : "";
        color: theme.status-added;
        font-size: 11px;
        horizontal-alignment: center;
        vertical-alignment: center;
    }

    TouchArea {
        mouse-cursor: pointer;
        clicked => { root.toggle-viewed(); }
    }
}
```

Modify the Name Text to dim when viewed:
```slint
Text {
    text: name;
    color: viewed ? theme.text-muted :
           selected ? theme.text-primary : theme.text-secondary;
    font-size: 13px;
    vertical-alignment: center;
    overflow: elide;
}
```

- [ ] **Step 2: Wire viewed property and callback in FileTree**

In the `FileTree` component, add the callback:
```slint
callback viewed-toggled(/* index */ int);
```

In the `for file[idx] in files:` loop, pass the viewed property and wire the callback:

```slint
for file[idx] in files: TreeItem {
    theme: root.theme;
    name: file.name;
    path: file.path;
    depth: file.depth;
    is-folder: file.is-folder;
    is-expanded: file.is-expanded;
    status: file.status;
    comment-count: file.comment-count;
    viewed: file.viewed;
    selected: file.path == selected-file;
    focused: idx == root.focused-index;

    clicked => {
        if (!file.is-folder) {
            root.file-clicked(file.path, idx);
        }
    }

    toggle-expand => {
        root.folder-toggled(file.path);
    }

    toggle-viewed => {
        root.viewed-toggled(idx);
    }
}
```

- [ ] **Step 3: Wire FileTree callback to MainWindow in main.slint**

In `main.slint`, connect the FileTree's `viewed-toggled` callback:

```slint
FileTree {
    vertical-stretch: 1;
    theme: root.theme;
    selected-file: root.selected-file;
    files: root.files;
    focused-index: root.focused-index;
    file-clicked(path, idx) => {
        root.selected-file = path;
        root.focused-index = idx;
        root.file-selected(path);
    }
    folder-toggled(path) => {
        root.folder-toggled(path);
    }
    viewed-toggled(idx) => {
        root.toggle-viewed(idx);
    }
}
```

- [ ] **Step 4: Add `v` key handler in main.slint**

In the `key-pressed` handler in main.slint, add before the `reject` at the end (around line 181):

```slint
// v - toggle viewed on focused file
if (event.text == "v") {
    root.toggle-viewed(root.focused-index);
    return accept;
}
```

- [ ] **Step 5: Verify it compiles and run manually**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```bash
git add ui/components/file_tree.slint ui/main.slint
git commit -m "feat(lado): add viewed checkbox UI with v key toggle and dimmed styling"
```

---

### Task 5: Help overlay component

**Files:**
- Create: `ui/components/help_overlay.slint`

- [ ] **Step 1: Create the help overlay component**

Create `ui/components/help_overlay.slint`:

```slint
import { ThemeColors } from "../theme.slint";
import { AppSettings } from "settings_panel.slint";

component KeyRow inherits Rectangle {
    in property <ThemeColors> theme;
    in property <string> key;
    in property <string> description;

    height: 28px;

    HorizontalLayout {
        padding-left: 24px;
        padding-right: 24px;
        spacing: 16px;

        // Key badge
        Rectangle {
            width: 48px;
            height: 22px;
            border-radius: 4px;
            background: theme.bg-tertiary;
            border-width: 1px;
            border-color: theme.border-normal;

            Text {
                text: key;
                color: theme.text-primary;
                font-size: 12px;
                font-weight: 600;
                horizontal-alignment: center;
                vertical-alignment: center;
                font-family: "monospace";
            }
        }

        Text {
            text: description;
            color: theme.text-secondary;
            font-size: 12px;
            vertical-alignment: center;
        }
    }
}

component SectionHeader inherits Rectangle {
    in property <ThemeColors> theme;
    in property <string> title;

    height: 32px;

    HorizontalLayout {
        padding-left: 24px;
        padding-top: 8px;

        Text {
            text: title;
            color: theme.text-muted;
            font-size: 10px;
            font-weight: 700;
            letter-spacing: 1.5px;
            vertical-alignment: center;
        }
    }
}

export component HelpOverlay inherits Rectangle {
    in property <ThemeColors> theme;
    in property <AppSettings> settings;
    in-out property <bool> visible: false;

    callback close();

    // Full-screen backdrop
    opacity: visible ? 1.0 : 0.0;
    visible: self.opacity > 0;
    background: #000000.with-alpha(0.5);

    animate opacity { duration: 150ms; }

    // Backdrop click closes
    TouchArea {
        clicked => { root.close(); }
    }

    // Centered card
    Rectangle {
        x: (parent.width - self.width) / 2;
        y: (parent.height - self.height) / 2;
        width: min(480px, parent.width - 80px);
        height: min(520px, parent.height - 80px);
        background: theme.bg-secondary;
        border-radius: 8px;
        border-width: 1px;
        border-color: theme.border-normal;
        clip: true;

        // Prevent backdrop click from closing when clicking the card
        TouchArea { }

        VerticalLayout {
            // Header
            Rectangle {
                height: 48px;

                HorizontalLayout {
                    padding-left: 24px;
                    padding-right: 16px;
                    alignment: space-between;

                    Text {
                        text: "Keyboard Shortcuts";
                        color: theme.text-primary;
                        font-size: 15px;
                        font-weight: 600;
                        vertical-alignment: center;
                    }

                    // Close button
                    Rectangle {
                        width: 28px;
                        height: 28px;
                        border-radius: 4px;
                        background: close-ta.has-hover ? theme.bg-hover : transparent;

                        Text {
                            text: "✕";
                            color: theme.text-muted;
                            font-size: 14px;
                            horizontal-alignment: center;
                            vertical-alignment: center;
                        }

                        close-ta := TouchArea {
                            clicked => { root.close(); }
                        }
                    }
                }
            }

            // Separator
            Rectangle {
                height: 1px;
                background: theme.border-subtle;
            }

            // Scrollable content
            Flickable {
                viewport-height: 480px;

                VerticalLayout {
                    spacing: 0px;

                    SectionHeader { theme: root.theme; title: "NAVIGATION"; }
                    KeyRow { theme: root.theme; key: settings.key-scroll-down; description: "Scroll diff down"; }
                    KeyRow { theme: root.theme; key: settings.key-scroll-up; description: "Scroll diff up"; }
                    KeyRow { theme: root.theme; key: settings.key-file-next; description: "Next file (skip viewed)"; }
                    KeyRow { theme: root.theme; key: settings.key-file-prev; description: "Previous file (skip viewed)"; }
                    KeyRow { theme: root.theme; key: settings.key-prev-commit; description: "Previous commit"; }
                    KeyRow { theme: root.theme; key: settings.key-next-commit; description: "Next commit"; }
                    KeyRow { theme: root.theme; key: "Enter"; description: "Select focused file"; }

                    SectionHeader { theme: root.theme; title: "VIEW"; }
                    KeyRow { theme: root.theme; key: settings.key-unified; description: "Unified diff view"; }
                    KeyRow { theme: root.theme; key: settings.key-side-by-side; description: "Side-by-side diff view"; }

                    SectionHeader { theme: root.theme; title: "FILE TREE"; }
                    KeyRow { theme: root.theme; key: "e"; description: "Toggle expand/collapse folder"; }
                    KeyRow { theme: root.theme; key: "E"; description: "Expand all directories"; }
                    KeyRow { theme: root.theme; key: "c"; description: "Collapse all directories"; }
                    KeyRow { theme: root.theme; key: "C"; description: "Expand folder recursively"; }
                    KeyRow { theme: root.theme; key: "v"; description: "Toggle file as viewed"; }

                    SectionHeader { theme: root.theme; title: "OTHER"; }
                    KeyRow { theme: root.theme; key: "F11"; description: "Toggle fullscreen"; }
                    KeyRow { theme: root.theme; key: "?"; description: "Toggle this help"; }

                    // Footer
                    Rectangle {
                        height: 40px;

                        Text {
                            text: "lado v0.1.0";
                            color: theme.text-muted;
                            font-size: 10px;
                            horizontal-alignment: center;
                            vertical-alignment: center;
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Verify it compiles (won't be wired yet)**

Run: `cargo build 2>&1 | head -10`
Note: This may show warnings about unused imports but should compile. We'll wire it in the next step.

- [ ] **Step 3: Commit**

```bash
git add ui/components/help_overlay.slint
git commit -m "feat(lado): add help overlay component with keybinding reference"
```

---

### Task 6: Integrate help overlay into MainWindow

**Files:**
- Modify: `ui/main.slint`

- [ ] **Step 1: Import and add HelpOverlay to main.slint**

Add import at top of `main.slint`:
```slint
import { HelpOverlay } from "components/help_overlay.slint";
```

Add a property to MainWindow:
```slint
in-out property <bool> help-visible: false;
```

- [ ] **Step 2: Add `?` and Escape key handlers**

In the `key-pressed` handler, add before the settings-visible check (so help works even without settings open, around line 90):

```slint
// ? toggles help overlay
if (event.text == "?") {
    root.help-visible = !root.help-visible;
    return accept;
}
// Escape closes help overlay
if (event.text == Key.Escape) {
    if (root.help-visible) {
        root.help-visible = false;
        return accept;
    }
}
```

Modify the settings-visible guard to also block input when help is visible:
```slint
// Skip keyboard navigation when settings panel or help overlay is open
if (root.settings-visible || root.help-visible) {
    return reject;
}
```

- [ ] **Step 3: Add HelpOverlay component to the window**

Add after the SettingsPanel at the bottom of MainWindow (so it renders on top):

```slint
// Help overlay (centered, on top of everything)
HelpOverlay {
    width: root.width;
    height: root.height;
    theme: root.theme;
    settings: root.app-settings;
    visible: root.help-visible;
    close => {
        root.help-visible = false;
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -30`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add ui/main.slint
git commit -m "feat(lado): integrate help overlay with ? key toggle"
```

---

### Task 7: Manual testing and polish

**Files:** None new — this is verification only.

- [ ] **Step 1: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | head -30`
Expected: No warnings

- [ ] **Step 3: Manual smoke test**

Run: `cargo run 2>&1 | head -5`
Test:
1. Press `?` — help overlay appears centered
2. Press `?` again — closes
3. Press `Escape` — closes
4. Navigate with `J`/`K` — works normally
5. Press `v` on a file — checkbox appears, text dims
6. Press `J` — skips the viewed file
7. Press `v` again — unchecks
8. Close and reopen — viewed state persists

- [ ] **Step 4: Fix any clippy warnings or issues**

- [ ] **Step 5: Final commit if any fixes were needed**

```bash
git add -u
git commit -m "fix(lado): address clippy warnings and polish"
```
