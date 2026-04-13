use crate::cli::{Args, DiffTarget};
use crate::git::{
    build_file_tree, collect_folder_paths, collect_folder_paths_under, flatten_tree_with_state,
    DiffData, FileTreeNode, Repository,
};
use crate::github::{self, FileComments, PrCommit};
use crate::highlighting::Highlighter;
use crate::models::{DiffLineModel, FileEntryModel, PrCommitModel, TextSpanModel};
use crate::viewed_state::{self, ViewedState};
use crate::{DiffLine, FileEntry, MainWindow, PrCommitEntry};
use anyhow::{Context, Result};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
    /// Cached file tree for re-flattening when folders are toggled
    file_tree: Rc<RefCell<Vec<FileTreeNode>>>,
    /// Expanded state for folders (path -> is_expanded)
    expanded_state: Rc<RefCell<HashMap<String, bool>>>,
    /// Persisted per-file viewed state
    viewed_state: Rc<RefCell<ViewedState>>,
    /// Key derived from diff target for viewed state persistence
    target_key: String,
}

/// Count comments that actually match a diff line for a given file.
/// Only counts comments whose line number matches a line in the diff,
/// so stale/resolved comments pointing at lines no longer in the diff are excluded.
fn count_matching_comments(
    hunks: &[crate::git::DiffHunk],
    comments: &[github::PrComment],
) -> i32 {
    use std::collections::HashSet;

    // Collect all (side, line) pairs present in the diff
    let mut new_lines = HashSet::new();
    let mut old_lines = HashSet::new();
    for hunk in hunks {
        for line in &hunk.lines {
            if let Some(n) = line.new_line_num {
                new_lines.insert(n);
            }
            if let Some(n) = line.old_line_num {
                old_lines.insert(n);
            }
        }
    }

    comments
        .iter()
        .filter(|c| match c.line {
            Some(line) => match c.side {
                github::CommentSide::Right => new_lines.contains(&line),
                github::CommentSide::Left => old_lines.contains(&line),
            },
            None => false,
        })
        .count() as i32
}

/// Convert flat file entries to Slint FileEntry models, enriching with comment counts.
/// Only counts comments that match actual diff lines (excludes stale comments).
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
            if let Some((vs, tk)) = viewed_state {
                if !f.is_folder {
                    if let Some(data) = diff_data {
                        let hash = data
                            .file_hunks
                            .get(&f.path)
                            .map(|h| viewed_state::hash_diff_content(h))
                            .unwrap_or(0);
                        model.viewed = vs.is_viewed(tk, &f.path, hash);
                    }
                }
            }
            model.into()
        })
        .collect()
}

/// Pick the initial focus row: first unviewed non-folder, else first non-folder, else -1.
/// Matches J/K navigation semantics (which skips folders and viewed files).
fn find_initial_focus_index(entries: &[FileEntry]) -> i32 {
    entries
        .iter()
        .position(|e| !e.is_folder && !e.viewed)
        .or_else(|| entries.iter().position(|e| !e.is_folder))
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// Look up the viewed status of a file by path, independent of the file tree
/// model. Used to keep the diff header's checkbox correct even when the file
/// is hidden by a collapsed ancestor.
fn is_path_viewed(
    path: &str,
    viewed: &ViewedState,
    diff_data: Option<&DiffData>,
    target_key: &str,
) -> bool {
    let Some(data) = diff_data else { return false };
    let hash = data
        .file_hunks
        .get(path)
        .map(|h| viewed_state::hash_diff_content(h))
        .unwrap_or(0);
    viewed.is_viewed(target_key, path, hash)
}

impl App {
    pub fn new(args: Args) -> Result<Self> {
        let window = MainWindow::new().context("Failed to create window")?;
        let repo = Rc::new(Repository::open_current_dir()?);
        let target = DiffTarget::parse(args.target.as_deref());

        // Load persisted settings
        let config = crate::config::load();
        window.set_app_settings(crate::AppSettings {
            ui_theme: config.ui_theme.clone().into(),
            font_size: config.font_size,
            tab_width: config.tab_width,
            line_wrap: config.line_wrap,
            key_unified: config.key_unified.clone().into(),
            key_side_by_side: config.key_side_by_side.clone().into(),
            key_scroll_down: config.key_scroll_down.clone().into(),
            key_scroll_up: config.key_scroll_up.clone().into(),
            key_file_next: config.key_file_next.clone().into(),
            key_file_prev: config.key_file_prev.clone().into(),
            key_prev_commit: config.key_prev_commit.clone().into(),
            key_next_commit: config.key_next_commit.clone().into(),
        });
        // Apply theme from config (theme is derived from theme-name in Slint)
        window.set_theme_name(config.ui_theme.clone().into());
        // Restore persisted panel width
        window.set_left_panel_width(config.panel_width);

        // Set the diff title based on target
        let diff_title = match &target {
            DiffTarget::DefaultBranch => {
                let default_branch = repo.find_default_branch()?;
                format!("HEAD vs {}", default_branch)
            }
            DiffTarget::Ref(r) => format!("HEAD vs {}", r),
            DiffTarget::PullRequest(pr) => format!("PR #{}", pr),
        };
        window.set_diff_title(diff_title.into());

        // Initialize syntax highlighter with theme matching UI theme
        let mut highlighter = Highlighter::new();
        highlighter.set_theme(config.ui_theme.as_str());

        let viewed_state = Rc::new(RefCell::new(ViewedState::load()));
        let target_key = viewed_state::target_key(&target);

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

        app.setup_callbacks()?;
        app.load_diff()?;

        Ok(app)
    }

    fn setup_callbacks(&self) -> Result<()> {
        let window_weak = self.window.as_weak();
        let diff_data = Rc::clone(&self.diff_data);
        let pr_comments = Rc::clone(&self.pr_comments);
        let highlighter = Rc::clone(&self.highlighter);
        let viewed_state_for_select = Rc::clone(&self.viewed_state);
        let target_key_for_select = self.target_key.clone();

        // File selection callback
        self.window.on_file_selected(move |path| {
            let window = window_weak.unwrap();
            let path_str = path.to_string();

            let data_borrow = diff_data.borrow();
            if let Some(ref data) = *data_borrow {
                let comments = pr_comments.borrow();
                let hl = highlighter.borrow();
                let lines = get_lines_for_file(data, &path_str, comments.as_ref(), &hl);
                window.set_lines(lines);
            }

            let viewed = is_path_viewed(
                &path_str,
                &viewed_state_for_select.borrow(),
                data_borrow.as_ref(),
                &target_key_for_select,
            );
            window.set_selected_file(path);
            window.set_selected_file_viewed(viewed);
        });

        // Folder toggle callback for collapsing/expanding directories
        let window_weak = self.window.as_weak();
        let file_tree = Rc::clone(&self.file_tree);
        let expanded_state = Rc::clone(&self.expanded_state);
        let pr_comments = Rc::clone(&self.pr_comments);
        let diff_data = Rc::clone(&self.diff_data);
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        self.window.on_folder_toggled(move |path| {
            let window = window_weak.unwrap();
            let path_str = path.to_string();

            // Toggle the expanded state for this folder
            {
                let mut state = expanded_state.borrow_mut();
                let is_expanded = state.get(&path_str).copied().unwrap_or(true);
                state.insert(path_str.clone(), !is_expanded);
            }

            // Re-flatten the tree with updated expanded state
            let tree = file_tree.borrow();
            let state = expanded_state.borrow();
            let flat_entries = flatten_tree_with_state(&tree, 0, &state);

            let file_entries = build_file_entries(
                &flat_entries,
                pr_comments.borrow().as_ref(),
                diff_data.borrow().as_ref(),
                Some((&viewed_state.borrow(), &target_key)),
            );

            // The re-flatten invalidated focused-index (rows shifted). Re-anchor it
            // to the currently-selected file so the header's viewed indicator stays
            // correct. If the selected file is now hidden (ancestor collapsed),
            // fall back to the toggled folder row so focus stays visible.
            let selected = window.get_selected_file().to_string();
            let new_focus = flat_entries
                .iter()
                .position(|e| e.path == selected)
                .or_else(|| flat_entries.iter().position(|e| e.path == path_str))
                .map(|i| i as i32)
                .unwrap_or(-1);
            window.set_focused_index(new_focus);

            let files_model = Rc::new(VecModel::from(file_entries));
            window.set_files(ModelRc::from(files_model));
        });

        let window_weak = self.window.as_weak();
        self.window.on_toggle_view_mode(move || {
            let _window = window_weak.unwrap();
            println!("Toggle view mode");
        });

        let window_weak = self.window.as_weak();
        self.window.on_toggle_fullscreen(move || {
            let window = window_weak.unwrap();
            let is_fullscreen = window.window().is_fullscreen();
            window.window().set_fullscreen(!is_fullscreen);
        });

        let window_weak = self.window.as_weak();
        self.window.on_refresh_diff(move || {
            let _window = window_weak.unwrap();
            println!("Refresh diff");
        });

        // Commit selection callback for PR commit navigation
        let window_weak = self.window.as_weak();
        let repo = Rc::clone(&self.repo);
        let pr_commits = Rc::clone(&self.pr_commits);
        let pr_base_ref = Rc::clone(&self.pr_base_ref);
        let pr_head_ref = Rc::clone(&self.pr_head_ref);
        let all_pr_comments = Rc::clone(&self.all_pr_comments);
        let highlighter = Rc::clone(&self.highlighter);
        self.window.on_commit_selected(move |idx| {
            let window = window_weak.unwrap();
            let commits = pr_commits.borrow();
            let comments = all_pr_comments.borrow();

            let diff_result: Option<(Result<DiffData>, Option<FileComments>)> = if idx < 0 {
                // "All changes" - diff base to head
                let base_ref = pr_base_ref.borrow();
                let head_ref = pr_head_ref.borrow();
                if let (Some(base), Some(head)) = (base_ref.as_ref(), head_ref.as_ref()) {
                    let base_oid = repo.resolve_ref(base).ok();
                    let head_oid = repo.resolve_ref(head).ok();
                    if let (Some(b), Some(h)) = (base_oid, head_oid) {
                        // Show all comments for full diff
                        let grouped = github::group_comments_by_file(comments.clone());
                        Some((repo.diff_commits(b, h), Some(grouped)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else if let Some(commit) = commits.get(idx as usize) {
                // Single commit - diff parent to this commit
                if let Some(ref parent_sha) = commit.parent_sha {
                    let parent_oid = repo.resolve_ref(parent_sha).ok();
                    let commit_oid = repo.resolve_ref(&commit.sha).ok();
                    if let (Some(p), Some(c)) = (parent_oid, commit_oid) {
                        // Filter comments to only show those on this commit
                        let filtered: Vec<_> = comments
                            .iter()
                            .filter(|c| c.original_commit_id == commit.sha)
                            .cloned()
                            .collect();
                        let grouped = github::group_comments_by_file(filtered);
                        Some((repo.diff_commits(p, c), Some(grouped)))
                    } else {
                        None
                    }
                } else {
                    // First commit in PR - no parent, show empty diff or handle differently
                    // For now, just show the commit itself compared to base
                    let base_ref = pr_base_ref.borrow();
                    if let Some(base) = base_ref.as_ref() {
                        let base_oid = repo.resolve_ref(base).ok();
                        let commit_oid = repo.resolve_ref(&commit.sha).ok();
                        if let (Some(b), Some(c)) = (base_oid, commit_oid) {
                            let filtered: Vec<_> = comments
                                .iter()
                                .filter(|c| c.original_commit_id == commit.sha)
                                .cloned()
                                .collect();
                            let grouped = github::group_comments_by_file(filtered);
                            Some((repo.diff_commits(b, c), Some(grouped)))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            if let Some((Ok(mut diff_data), grouped_comments)) = diff_result {
                diff_data.expand_tabs(window.get_app_settings().tab_width as usize);
                // Build hierarchical file tree and flatten for UI
                // Use empty expanded state for commit-specific views (fresh view each time)
                let tree = build_file_tree(&diff_data.files);
                let flat_entries = flatten_tree_with_state(&tree, 0, &HashMap::new());

                let file_entries =
                    build_file_entries(&flat_entries, grouped_comments.as_ref(), Some(&diff_data), None);

                let initial_focus = find_initial_focus_index(&file_entries);
                let initial_viewed = if initial_focus >= 0 {
                    file_entries
                        .get(initial_focus as usize)
                        .map(|e| e.viewed)
                        .unwrap_or(false)
                } else {
                    false
                };

                let files_model = Rc::new(VecModel::from(file_entries));
                window.set_files(ModelRc::from(files_model));

                if initial_focus >= 0 {
                    if let Some(initial) = flat_entries.get(initial_focus as usize) {
                        window.set_focused_index(initial_focus);
                        window.set_selected_file(initial.path.clone().into());
                        window.set_selected_file_viewed(initial_viewed);
                        let hl = highlighter.borrow();
                        let lines = get_lines_for_file(
                            &diff_data,
                            &initial.path,
                            grouped_comments.as_ref(),
                            &hl,
                        );
                        window.set_lines(lines);
                    }
                }
            }
        });

        // Settings changed callback
        let highlighter = Rc::clone(&self.highlighter);
        let window_weak = self.window.as_weak();
        let diff_data = Rc::clone(&self.diff_data);
        let pr_comments = Rc::clone(&self.pr_comments);
        self.window.on_settings_changed(move |settings| {
            // Persist settings to config file
            let window = window_weak.unwrap();
            let config = crate::config::Config {
                ui_theme: settings.ui_theme.to_string(),
                font_size: settings.font_size,
                tab_width: settings.tab_width,
                line_wrap: settings.line_wrap,
                panel_width: window.get_left_panel_width(),
                key_unified: settings.key_unified.to_string(),
                key_side_by_side: settings.key_side_by_side.to_string(),
                key_scroll_down: settings.key_scroll_down.to_string(),
                key_scroll_up: settings.key_scroll_up.to_string(),
                key_file_next: settings.key_file_next.to_string(),
                key_file_prev: settings.key_file_prev.to_string(),
                key_prev_commit: settings.key_prev_commit.to_string(),
                key_next_commit: settings.key_next_commit.to_string(),
            };
            if let Err(e) = crate::config::save(&config) {
                eprintln!("Warning: Could not save settings: {}", e);
            }

            highlighter.borrow_mut().set_theme(settings.ui_theme.as_str());

            // Re-highlight currently selected file
            let selected_file = window.get_selected_file().to_string();
            if !selected_file.is_empty() {
                if let Some(ref data) = *diff_data.borrow() {
                    let comments = pr_comments.borrow();
                    let hl = highlighter.borrow();
                    let lines = get_lines_for_file(data, &selected_file, comments.as_ref(), &hl);
                    window.set_lines(lines);
                }
            }
        });

        // Find next file callback (skips directories)
        let window_weak = self.window.as_weak();
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
            // No file found in that direction, return current or -1
            if current_idx >= 0 && current_idx < len {
                current_idx
            } else {
                -1
            }
        });

        // Expand all directories callback
        let window_weak = self.window.as_weak();
        let file_tree = Rc::clone(&self.file_tree);
        let expanded_state = Rc::clone(&self.expanded_state);
        let pr_comments = Rc::clone(&self.pr_comments);
        let diff_data = Rc::clone(&self.diff_data);
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        self.window.on_expand_all_directories(move || {
            let window = window_weak.unwrap();
            let tree = file_tree.borrow();
            let folder_paths = collect_folder_paths(&tree);

            // Set all folders to expanded
            {
                let mut state = expanded_state.borrow_mut();
                for path in folder_paths {
                    state.insert(path, true);
                }
            }

            // Re-flatten the tree
            let state = expanded_state.borrow();
            let flat_entries = flatten_tree_with_state(&tree, 0, &state);

            let file_entries = build_file_entries(
                &flat_entries,
                pr_comments.borrow().as_ref(),
                diff_data.borrow().as_ref(),
                Some((&viewed_state.borrow(), &target_key)),
            );

            let files_model = Rc::new(VecModel::from(file_entries));
            window.set_files(ModelRc::from(files_model));
        });

        // Collapse all directories callback
        let window_weak = self.window.as_weak();
        let file_tree = Rc::clone(&self.file_tree);
        let expanded_state = Rc::clone(&self.expanded_state);
        let pr_comments = Rc::clone(&self.pr_comments);
        let diff_data = Rc::clone(&self.diff_data);
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        self.window.on_collapse_all_directories(move || {
            let window = window_weak.unwrap();
            let tree = file_tree.borrow();
            let folder_paths = collect_folder_paths(&tree);

            // Set all folders to collapsed
            {
                let mut state = expanded_state.borrow_mut();
                for path in folder_paths {
                    state.insert(path, false);
                }
            }

            // Re-flatten the tree
            let state = expanded_state.borrow();
            let flat_entries = flatten_tree_with_state(&tree, 0, &state);

            let file_entries = build_file_entries(
                &flat_entries,
                pr_comments.borrow().as_ref(),
                diff_data.borrow().as_ref(),
                Some((&viewed_state.borrow(), &target_key)),
            );

            let files_model = Rc::new(VecModel::from(file_entries));
            window.set_files(ModelRc::from(files_model));
        });

        // Toggle focused directory callback
        let window_weak = self.window.as_weak();
        let file_tree = Rc::clone(&self.file_tree);
        let expanded_state = Rc::clone(&self.expanded_state);
        let pr_comments = Rc::clone(&self.pr_comments);
        let diff_data = Rc::clone(&self.diff_data);
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        self.window.on_toggle_focused_directory(move || {
            let window = window_weak.unwrap();
            let files = window.get_files();
            let focused_idx = window.get_focused_index() as usize;

            // Get the focused file entry
            if let Some(entry) = files.row_data(focused_idx) {
                if entry.is_folder {
                    let path = entry.path.to_string();

                    // Toggle the expanded state
                    {
                        let mut state = expanded_state.borrow_mut();
                        let is_expanded = state.get(&path).copied().unwrap_or(true);
                        state.insert(path, !is_expanded);
                    }

                    // Re-flatten the tree
                    let tree = file_tree.borrow();
                    let state = expanded_state.borrow();
                    let flat_entries = flatten_tree_with_state(&tree, 0, &state);

                    let file_entries = build_file_entries(
                        &flat_entries,
                        pr_comments.borrow().as_ref(),
                        diff_data.borrow().as_ref(),
                        Some((&viewed_state.borrow(), &target_key)),
                    );

                    let files_model = Rc::new(VecModel::from(file_entries));
                    window.set_files(ModelRc::from(files_model));
                }
            }
        });

        // Expand focused directory recursively callback
        let window_weak = self.window.as_weak();
        let file_tree = Rc::clone(&self.file_tree);
        let expanded_state = Rc::clone(&self.expanded_state);
        let pr_comments = Rc::clone(&self.pr_comments);
        let diff_data = Rc::clone(&self.diff_data);
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        self.window.on_expand_focused_recursive(move || {
            let window = window_weak.unwrap();
            let files = window.get_files();
            let focused_idx = window.get_focused_index() as usize;

            // Get the focused file entry
            if let Some(entry) = files.row_data(focused_idx) {
                if entry.is_folder {
                    let path = entry.path.to_string();

                    // Get all folder paths under (and including) the focused folder
                    let tree = file_tree.borrow();
                    let folder_paths = collect_folder_paths_under(&tree, &path);

                    // Set all to expanded
                    {
                        let mut state = expanded_state.borrow_mut();
                        for p in folder_paths {
                            state.insert(p, true);
                        }
                    }

                    // Re-flatten the tree
                    let state = expanded_state.borrow();
                    let flat_entries = flatten_tree_with_state(&tree, 0, &state);

                    let file_entries = build_file_entries(
                        &flat_entries,
                        pr_comments.borrow().as_ref(),
                        diff_data.borrow().as_ref(),
                        Some((&viewed_state.borrow(), &target_key)),
                    );

                    let files_model = Rc::new(VecModel::from(file_entries));
                    window.set_files(ModelRc::from(files_model));
                }
            }
        });

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
                    vs.set_unviewed(tk, &path);
                } else {
                    let data = diff_data.borrow();
                    let hash = data
                        .as_ref()
                        .and_then(|d| d.file_hunks.get(&path))
                        .map(|h| viewed_state::hash_diff_content(h))
                        .unwrap_or(0);
                    vs.set_viewed(tk, &path, hash);
                }

                if let Err(e) = vs.save() {
                    eprintln!("Warning: Could not save viewed state: {}", e);
                }
                drop(vs);

                // Toggle in the UI model directly
                let model = files
                    .as_any()
                    .downcast_ref::<VecModel<FileEntry>>()
                    .unwrap();
                let mut updated = entry.clone();
                updated.viewed = !entry.viewed;
                model.set_row_data(idx as usize, updated);

                // If the toggled file is the one currently displayed, keep the
                // diff header's checkbox in sync.
                if window.get_selected_file().to_string() == path {
                    window.set_selected_file_viewed(!entry.viewed);
                }
            }
        });

        // Toggle viewed state for the currently-displayed file (works even when
        // the file is hidden from the tree by a collapsed ancestor).
        let window_weak = self.window.as_weak();
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        let diff_data = Rc::clone(&self.diff_data);
        self.window.on_toggle_selected_viewed(move || {
            let window = window_weak.unwrap();
            let path = window.get_selected_file().to_string();
            if path.is_empty() {
                return;
            }

            let mut vs = viewed_state.borrow_mut();
            let data_borrow = diff_data.borrow();
            let was_viewed = is_path_viewed(&path, &vs, data_borrow.as_ref(), &target_key);

            if was_viewed {
                vs.set_unviewed(&target_key, &path);
            } else {
                let hash = data_borrow
                    .as_ref()
                    .and_then(|d| d.file_hunks.get(&path))
                    .map(|h| viewed_state::hash_diff_content(h))
                    .unwrap_or(0);
                vs.set_viewed(&target_key, &path, hash);
            }

            if let Err(e) = vs.save() {
                eprintln!("Warning: Could not save viewed state: {}", e);
            }
            drop(vs);
            drop(data_borrow);

            window.set_selected_file_viewed(!was_viewed);

            // If the toggled file is currently visible in the tree, also update
            // the per-row entry so its checkbox reflects the new state.
            let files = window.get_files();
            if let Some(model) = files.as_any().downcast_ref::<VecModel<FileEntry>>() {
                for i in 0..model.row_count() {
                    if let Some(entry) = model.row_data(i) {
                        if entry.path.to_string() == path {
                            let mut updated = entry.clone();
                            updated.viewed = !was_viewed;
                            model.set_row_data(i, updated);
                            break;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    fn load_diff(&self) -> Result<()> {
        // Resolve the target to actual commits
        let (base_oid, head_oid) = match &self.target {
            DiffTarget::DefaultBranch => {
                let default_branch = self.repo.find_default_branch()?;
                let base = self.repo.resolve_ref(&default_branch)?;
                let head = self.repo.head_commit()?;
                (base, head)
            }
            DiffTarget::Ref(ref_name) => {
                let base = self.repo.resolve_ref(ref_name)?;
                let head = self.repo.head_commit()?;
                (base, head)
            }
            DiffTarget::PullRequest(pr_num) => {
                let pr_info = github::get_pr_refs(*pr_num)?;
                let base = self.repo.resolve_ref(&pr_info.base_ref)?;
                let head = self.repo.resolve_ref(&pr_info.head_ref)?;

                // Update toolbar with PR title
                self.window
                    .set_diff_title(format!("PR #{}: {}", pr_num, pr_info.title).into());

                // Store refs for later commit navigation
                *self.pr_base_ref.borrow_mut() = Some(pr_info.base_ref);
                *self.pr_head_ref.borrow_mut() = Some(pr_info.head_ref);

                // Fetch PR commits
                match github::get_pr_commits(*pr_num) {
                    Ok(commits) => {
                        // Convert to UI model
                        let commit_entries: Vec<PrCommitEntry> = commits
                            .iter()
                            .map(|c| PrCommitModel::from(c).into())
                            .collect();
                        let commits_model = Rc::new(VecModel::from(commit_entries));
                        self.window.set_commits(ModelRc::from(commits_model));
                        *self.pr_commits.borrow_mut() = commits;
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not fetch PR commits: {}", e);
                    }
                }

                // Fetch PR comments
                match github::get_pr_comments(*pr_num) {
                    Ok(comments) => {
                        let grouped = github::group_comments_by_file(comments.clone());
                        *self.pr_comments.borrow_mut() = Some(grouped);
                        *self.all_pr_comments.borrow_mut() = comments;
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not fetch PR comments: {}", e);
                    }
                }

                (base, head)
            }
        };

        // Compute the diff
        let mut diff_data = self.repo.diff_commits(base_oid, head_oid)?;
        diff_data.expand_tabs(self.window.get_app_settings().tab_width as usize);

        // Build hierarchical file tree and flatten for UI
        let tree = build_file_tree(&diff_data.files);
        let expanded_state = self.expanded_state.borrow();
        let flat_entries = flatten_tree_with_state(&tree, 0, &expanded_state);
        drop(expanded_state);

        let file_entries = build_file_entries(
            &flat_entries,
            self.pr_comments.borrow().as_ref(),
            Some(&diff_data),
            Some((&self.viewed_state.borrow(), &self.target_key)),
        );

        // Pick the initial focus row before moving file_entries into the model.
        let initial_focus = find_initial_focus_index(&file_entries);

        let files_model = Rc::new(VecModel::from(file_entries));
        self.window.set_files(ModelRc::from(files_model));

        // Load the diff for the initial focus row and keep focused-index in sync
        // with selected-file so the header "viewed" state is driven by the same row.
        if initial_focus >= 0 {
            if let Some(initial) = flat_entries.get(initial_focus as usize) {
                self.window.set_focused_index(initial_focus);
                self.window.set_selected_file(initial.path.clone().into());
                let viewed = is_path_viewed(
                    &initial.path,
                    &self.viewed_state.borrow(),
                    Some(&diff_data),
                    &self.target_key,
                );
                self.window.set_selected_file_viewed(viewed);
                let comments = self.pr_comments.borrow();
                let hl = self.highlighter.borrow();
                let lines = get_lines_for_file(&diff_data, &initial.path, comments.as_ref(), &hl);
                self.window.set_lines(lines);
            }
        }

        // Store for later use in callbacks
        *self.file_tree.borrow_mut() = tree;
        *self.diff_data.borrow_mut() = Some(diff_data);

        Ok(())
    }

    pub fn run(self) -> Result<()> {
        self.window.run().context("Failed to run window")?;

        // Persist panel width on exit
        let mut config = crate::config::load();
        config.panel_width = self.window.get_left_panel_width();
        if let Err(e) = crate::config::save(&config) {
            eprintln!("Warning: Could not save panel width: {}", e);
        }

        Ok(())
    }
}

/// Convert hunks for a file into Slint-compatible DiffLine model, interleaving comments
fn get_lines_for_file(
    data: &DiffData,
    path: &str,
    comments: Option<&FileComments>,
    highlighter: &Highlighter,
) -> ModelRc<DiffLine> {
    use crate::git::{CommentData, DiffLine as GitDiffLine, DiffLineType};
    use crate::models::parse_hex_color;

    let hunks = data.file_hunks.get(path).cloned().unwrap_or_default();

    // Get comments for this file, if any
    let file_comments = comments.and_then(|c| c.get(path));

    // First, collect all diff lines with their line numbers
    let diff_lines: Vec<GitDiffLine> = hunks
        .into_iter()
        .flat_map(|hunk| {
            // Create hunk header line (trim trailing newline from git2)
            let header_line = GitDiffLine {
                line_type: DiffLineType::Hunk,
                old_line_num: None,
                new_line_num: None,
                content: hunk.header.trim_end().to_string(),
                comment: None,
            };
            // Prepend header to hunk lines
            std::iter::once(header_line).chain(hunk.lines)
        })
        .collect();

    // Reconstruct file content from diff lines for syntax highlighting
    // We need to highlight the content to get spans for each line
    let content_lines: Vec<(&GitDiffLine, String)> = diff_lines
        .iter()
        .filter(|l| {
            matches!(
                l.line_type,
                DiffLineType::Add | DiffLineType::Remove | DiffLineType::Context
            )
        })
        .map(|l| (l, l.content.clone()))
        .collect();

    // Create a combined content string for highlighting
    let full_content: String = content_lines
        .iter()
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // Highlight the content
    let highlighted_lines = highlighter.highlight(&full_content, path);

    // Map highlighted lines back to diff lines
    let mut highlight_iter = highlighted_lines.into_iter();

    // Build the final lines, interleaving comments
    let mut result: Vec<DiffLine> = Vec::new();

    for diff_line in &diff_lines {
        // Convert to model
        let mut model = DiffLineModel::from(diff_line);

        // Add syntax highlighting spans for code lines
        if matches!(
            diff_line.line_type,
            DiffLineType::Add | DiffLineType::Remove | DiffLineType::Context
        ) {
            if let Some(hl_line) = highlight_iter.next() {
                model.spans = hl_line
                    .spans
                    .into_iter()
                    .map(|s| TextSpanModel::new(s.text, parse_hex_color(&s.color)))
                    .collect();
            }
        }

        result.push(model.into());

        // Check if there are comments for this line
        if let Some(comments) = file_comments {
            // Get the appropriate line number based on comment side
            let new_line = diff_line.new_line_num;
            let old_line = diff_line.old_line_num;

            // Find comments that target this line
            for comment in comments {
                let is_match = match comment.line {
                    Some(line) => {
                        // Match based on which side the comment is on
                        match comment.side {
                            github::CommentSide::Right => new_line == Some(line),
                            github::CommentSide::Left => old_line == Some(line),
                        }
                    }
                    None => false, // Skip comments without line numbers
                };

                if is_match {
                    // Create a comment line
                    let comment_line = GitDiffLine {
                        line_type: DiffLineType::Comment,
                        old_line_num: None,
                        new_line_num: None,
                        content: String::new(),
                        comment: Some(CommentData {
                            author: comment.author.clone(),
                            body: comment.body.clone(),
                            timestamp: format_timestamp(&comment.created_at),
                            is_reply: comment.in_reply_to_id.is_some(),
                        }),
                    };
                    result.push(DiffLineModel::from(&comment_line).into());
                }
            }
        }
    }

    ModelRc::new(VecModel::from(result))
}

/// Format a GitHub timestamp to a more readable format
fn format_timestamp(timestamp: &str) -> String {
    // GitHub timestamps are in ISO 8601 format: "2024-01-15T10:30:00Z"
    // Parse and format to something more readable
    if timestamp.len() >= 16 {
        // Extract "2024-01-15 10:30"
        let date = &timestamp[0..10];
        let time = &timestamp[11..16];
        format!("{} {}", date, time)
    } else {
        timestamp.to_string()
    }
}
