use crate::cli::{Args, DiffTarget};
use crate::context_level::ContextLevel;
use crate::git::{
    build_file_tree, collect_folder_paths, collect_folder_paths_under, expand_tabs_in_hunks,
    flatten_tree_with_state, hunks_cover_whole_file, selectable_text_for_hunks, CommitInfo,
    DiffData, DiffHunk, FileTreeNode, Repository,
};
use crate::flow_map::{FlowLayout, SegKind};
use crate::flow_scene::{build_scene, RowClass, SceneRow, Side};
use crate::github::{self, FileComments};
use crate::highlighting::Highlighter;
use crate::Ribbon;
use crate::models::{CommitModel, DiffLineModel, FileEntryModel, TextSpanModel};
use crate::segments::{segments_for_file, ChangeSegment};
use crate::viewed_state::{self, ViewedState};
use crate::watcher::DiffWatcher;
use crate::{CommitEntry, DiffLine, FileEntry, MainWindow};
use anyhow::{Context, Result};
use git2::Oid;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Upper bound on how many commits the sidebar lists. Keeps `lado <old-tag>`
/// from doing a full-history walk and building thousands of UI rows.
const COMMIT_LIST_LIMIT: usize = 200;

pub struct App {
    window: MainWindow,
    repo: Rc<Repository>,
    target: DiffTarget,
    diff_data: Rc<RefCell<Option<DiffData>>>,
    pr_comments: Rc<RefCell<Option<FileComments>>>,
    /// Commits making up the diff, oldest-first. From the GitHub API for PRs,
    /// from a local revwalk otherwise.
    commits: Rc<RefCell<Vec<CommitInfo>>>,
    all_pr_comments: Rc<RefCell<Vec<github::PrComment>>>,
    /// Endpoints of the diff, resolved once at load. Stored as OIDs rather than
    /// ref names so "All changes" can't re-resolve them to something else.
    range_base: Rc<Cell<Option<Oid>>>,
    range_head: Rc<Cell<Option<Oid>>>,
    highlighter: Rc<RefCell<Highlighter>>,
    /// Cached file tree for re-flattening when folders are toggled
    file_tree: Rc<RefCell<Vec<FileTreeNode>>>,
    /// Expanded state for folders (path -> is_expanded)
    expanded_state: Rc<RefCell<HashMap<String, bool>>>,
    /// Persisted per-file viewed state
    viewed_state: Rc<RefCell<ViewedState>>,
    /// Key derived from diff target for viewed state persistence
    target_key: String,
    /// Single owner of "put this file's diff on screen"
    renderer: DiffRenderer,
    /// Base and head ref *names* of the last pull request load. The watcher
    /// resolves these locally, so checking whether a PR diff moved costs no
    /// call to `gh`.
    pr_refs: RefCell<Option<(String, String)>>,
    /// Alive for as long as the window is. Dropping it stops the watching.
    watcher: RefCell<Option<DiffWatcher>>,
    /// Guards against a reload starting inside a reload.
    reloading: Cell<bool>,
}

/// Puts one file's diff on screen.
///
/// Five things trigger a render — initial load, file selected, commit selected,
/// settings changed, context stepped — and each has to set the same window
/// properties from the same sources. Bundling the shared state here is what
/// keeps them from drifting apart.
#[derive(Clone)]
struct DiffRenderer {
    repo: Rc<Repository>,
    diff_data: Rc<RefCell<Option<DiffData>>>,
    pr_comments: Rc<RefCell<Option<FileComments>>>,
    highlighter: Rc<RefCell<Highlighter>>,
    /// The commit pair backing what is on screen. Not the same as the range
    /// endpoints while a single commit is selected, and widening the context
    /// has to re-diff the pair the user is actually looking at.
    base: Rc<Cell<Option<Oid>>>,
    head: Rc<Cell<Option<Oid>>>,
    level: Rc<Cell<ContextLevel>>,
    /// Hunks last recomputed at a wider context. Without it, changing a setting
    /// would re-run the diff.
    widened: Rc<RefCell<Option<WidenedHunks>>>,
    /// Line counts for the file on screen. A function of the commit pair too,
    /// so `set_scope` drops it; cached because deciding whether the view
    /// already reaches both ends otherwise re-reads both blobs on every
    /// settings change.
    file_lines: Rc<RefCell<Option<FileLineCounts>>>,
    /// Whether the last render already put every line of the file on screen.
    /// Consulted when stepping, so `+` on a file with nothing left to reveal
    /// stops rather than re-diffing its way to the same picture.
    saturated: Rc<Cell<bool>>,
    /// Scroll-coupling layout for the file currently on screen. The flowing
    /// view's `remap` callback is registered once but must always see the
    /// current file, so it reads this shared cell rather than a captured value.
    flow_layout: Rc<RefCell<FlowLayout>>,
    /// Which change segments the reviewer has marked, and the key each is
    /// stored under.
    viewed_state: Rc<RefCell<ViewedState>>,
    target_key: String,
    /// The change segments of the file on screen, in the order the UI indexes
    /// them. A toggle arrives as a segment index and needs the hash behind it,
    /// and recomputing the split would re-read the whole file to answer.
    segments: Rc<RefCell<Vec<ChangeSegment>>>,
}

/// One file's hunks at a wider context, tagged with what they were computed
/// for so a stale entry can't be mistaken for a fresh one.
struct WidenedHunks {
    path: String,
    level: ContextLevel,
    hunks: Vec<DiffHunk>,
}

/// How many lines one file has on each side of the diff.
struct FileLineCounts {
    path: String,
    old: u32,
    new: u32,
}

impl DiffRenderer {
    /// Point the renderer at a different diff, dropping anything cached for the
    /// previous one. Context returns to the default: an expansion is about the
    /// file in front of you, not a mode you carry around.
    fn set_scope(&self, base: Oid, head: Oid) {
        self.base.set(Some(base));
        self.head.set(Some(head));
        *self.file_lines.borrow_mut() = None;
        self.reset_level();
    }

    fn reset_level(&self) {
        self.level.set(ContextLevel::DEFAULT);
        *self.widened.borrow_mut() = None;
    }

    fn cached_hunks(&self, path: &str) -> Vec<DiffHunk> {
        self.diff_data
            .borrow()
            .as_ref()
            .and_then(|d| d.file_hunks.get(path).cloned())
            .unwrap_or_default()
    }

    /// The hunks to display for `path`: the ones from the loaded diff at the
    /// default rung, or a re-diff of just this file at a wider one.
    fn hunks_for(&self, path: &str, tab_width: usize) -> Vec<DiffHunk> {
        let level = self.level.get();
        if level == ContextLevel::DEFAULT {
            return self.cached_hunks(path);
        }

        let cached = self
            .widened
            .borrow()
            .as_ref()
            .filter(|w| w.path == path && w.level == level)
            .map(|w| w.hunks.clone());
        if let Some(hunks) = cached {
            return hunks;
        }

        let (Some(base), Some(head)) = (self.base.get(), self.head.get()) else {
            return self.cached_hunks(path);
        };

        match self.repo.diff_file_at_context(base, head, path, level.lines()) {
            Ok(mut hunks) => {
                expand_tabs_in_hunks(&mut hunks, tab_width);
                *self.widened.borrow_mut() = Some(WidenedHunks {
                    path: path.to_string(),
                    level,
                    hunks: hunks.clone(),
                });
                hunks
            }
            Err(e) => {
                // Fall back to the narrow view rather than blanking the file.
                eprintln!("Warning: could not widen context for {}: {}", path, e);
                self.cached_hunks(path)
            }
        }
    }

    /// Lines on each side of `path`, old first. `None` when the commit pair or
    /// the blobs can't be read.
    fn line_counts(&self, path: &str) -> Option<(u32, u32)> {
        let cached = self
            .file_lines
            .borrow()
            .as_ref()
            .filter(|c| c.path == path)
            .map(|c| (c.old, c.new));
        if let Some(counts) = cached {
            return Some(counts);
        }

        let (base, head) = (self.base.get()?, self.head.get()?);
        let (old, new) = self.repo.file_line_counts(base, head, path).ok()?;
        *self.file_lines.borrow_mut() = Some(FileLineCounts {
            path: path.to_string(),
            old,
            new,
        });
        Some((old, new))
    }

    fn render(&self, window: &MainWindow, path: &str) {
        if path.is_empty() {
            return;
        }
        let settings = window.get_app_settings();
        let hunks = self.hunks_for(path, settings.tab_width as usize);

        let comments = self.pr_comments.borrow();
        let hl = self.highlighter.borrow();
        let wrap = settings.line_wrap_column.max(0) as usize;
        // Split the file into change segments, then collapse the ones already
        // marked. Everything downstream — both gutters, both panes and the
        // ribbons — is built from the result, so all three views collapse
        // together.
        let segments = segments_of(&hunks);
        let (lines, old_digits, new_digits) = get_lines_for_file(
            &hunks,
            path,
            comments.as_ref().and_then(|c| c.get(path)),
            &hl,
            wrap,
            &segments,
        );
        let lines = self.collapse_viewed_segments(path, segments, lines);

        // Flowing view: split the merged rows into two panes + ribbons. Heights
        // must match the flowing `.slint`, which is handed these same values.
        let row_h = settings.font_size as f32 * 1.7;
        let comment_h = 80.0;
        let (flow_left, flow_right, ribbons, layout) =
            build_flow_from_rows(&lines, row_h, comment_h);
        let total_virt = layout.total_virt;
        *self.flow_layout.borrow_mut() = layout;
        window.set_flowing_left_rows(ModelRc::new(VecModel::from(flow_left)));
        window.set_flowing_right_rows(ModelRc::new(VecModel::from(flow_right)));
        window.set_flowing_ribbons(ModelRc::new(VecModel::from(ribbons)));
        window.set_flowing_total_virt(total_virt);
        window.set_flowing_row_height(row_h);
        window.set_flowing_comment_height(comment_h);
        // A fresh file starts at the top; the panes rest at offset 0 until a
        // scroll drives remap().
        window.set_flowing_left_y(0.0);
        window.set_flowing_right_y(0.0);

        window.set_lines(ModelRc::new(VecModel::from(lines)));
        window.set_old_gutter_digits(old_digits);
        window.set_new_gutter_digits(new_digits);
        window.set_selectable_text(selectable_text_for_hunks(&hunks).as_str().into());

        // A short file is showing everything long before the top rung, so the
        // control has to read the picture rather than the ladder position.
        let level = self.level.get();
        let covers_whole_file = level.is_whole_file()
            || self
                .line_counts(path)
                .is_some_and(|(old, new)| hunks_cover_whole_file(&hunks, old, new));
        self.saturated.set(covers_whole_file);
        window.set_context_level_label(level.label(covers_whole_file).as_str().into());
        window.set_context_expandable(!covers_whole_file);
    }

    /// Remember this file's change segments for the toggle callback, and
    /// replace every marked segment's rows with a collapsed bar.
    fn collapse_viewed_segments(
        &self,
        path: &str,
        segments: Vec<ChangeSegment>,
        lines: Vec<DiffLine>,
    ) -> Vec<DiffLine> {
        let viewed: Vec<bool> = {
            let vs = self.viewed_state.borrow();
            segments
                .iter()
                .map(|s| vs.is_segment_viewed(&self.target_key, path, s.hash))
                .collect()
        };

        let out = apply_segments(lines, &segments, &viewed);
        *self.segments.borrow_mut() = segments;
        out
    }

    /// The content keys of the file's change segments, in UI index order.
    fn segment_hashes(&self) -> Vec<u64> {
        self.segments.borrow().iter().map(|s| s.hash).collect()
    }

    /// Move one rung and redraw. A step that can't change the picture — past
    /// either end of the ladder, or wider when every line is already on screen
    /// — skips the re-render entirely.
    fn step_context(&self, window: &MainWindow, expand: bool) {
        if expand && self.saturated.get() {
            return;
        }
        let current = self.level.get();
        let next = if expand {
            current.expanded()
        } else {
            current.collapsed()
        };
        if next == current {
            return;
        }
        self.level.set(next);
        let path = window.get_selected_file().to_string();
        self.render(window, &path);
    }
}

/// Format the "behind base" indicator appended to the PR diff title.
/// Returns an empty string when there is nothing to warn about.
fn format_stale_base_note(base_ref: &str, commits_behind: usize) -> String {
    if commits_behind == 0 {
        String::new()
    } else {
        format!(
            " ⚠ behind {} by {} commit{}",
            base_ref,
            commits_behind,
            if commits_behind == 1 { "" } else { "s" }
        )
    }
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
            // A file is viewed when every one of its change segments is; the
            // checkbox is a readout of the segments, never a separate fact.
            if let Some((vs, tk)) = viewed_state {
                if !f.is_folder {
                    if let Some(data) = diff_data {
                        let empty = Vec::new();
                        let hunks = data.file_hunks.get(&f.path).unwrap_or(&empty);
                        model.viewed = file_is_viewed(vs, tk, &f.path, hunks);
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

/// Pick the row to focus after building the file list. `keep` is the file that
/// was on screen before a reload.
fn pick_focus_index(entries: &[FileEntry], keep: Option<&str>) -> i32 {
    keep.and_then(|path| entries.iter().position(|e| e.path == path))
        .map(|i| i as i32)
        .unwrap_or_else(|| find_initial_focus_index(entries))
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
    let empty = Vec::new();
    let hunks = data.file_hunks.get(path).unwrap_or(&empty);
    file_is_viewed(viewed, target_key, path, hunks)
}

impl App {
    pub fn new(args: Args) -> Result<Rc<Self>> {
        let window = MainWindow::new().context("Failed to create window")?;
        let repo = Rc::new(Repository::open_current_dir()?);
        let target = DiffTarget::parse(args.target.as_deref());

        // Load persisted settings
        let config = crate::config::load();
        window.set_app_settings(crate::AppSettings {
            ui_theme: config.ui_theme.clone().into(),
            font_size: config.font_size,
            tab_width: config.tab_width,
            line_wrap_column: config.line_wrap_column,
            key_unified: config.key_unified.clone().into(),
            key_side_by_side: config.key_side_by_side.clone().into(),
            key_flowing: config.key_flowing.clone().into(),
            key_scroll_down: config.key_scroll_down.clone().into(),
            key_scroll_up: config.key_scroll_up.clone().into(),
            key_file_next: config.key_file_next.clone().into(),
            key_file_prev: config.key_file_prev.clone().into(),
            key_prev_commit: config.key_prev_commit.clone().into(),
            key_next_commit: config.key_next_commit.clone().into(),
            key_expand_context: config.key_expand_context.clone().into(),
            key_collapse_context: config.key_collapse_context.clone().into(),
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

        let diff_data = Rc::new(RefCell::new(None));
        let pr_comments = Rc::new(RefCell::new(None));
        let highlighter = Rc::new(RefCell::new(highlighter));

        let renderer = DiffRenderer {
            repo: Rc::clone(&repo),
            diff_data: Rc::clone(&diff_data),
            pr_comments: Rc::clone(&pr_comments),
            highlighter: Rc::clone(&highlighter),
            base: Rc::new(Cell::new(None)),
            head: Rc::new(Cell::new(None)),
            level: Rc::new(Cell::new(ContextLevel::DEFAULT)),
            widened: Rc::new(RefCell::new(None)),
            file_lines: Rc::new(RefCell::new(None)),
            saturated: Rc::new(Cell::new(false)),
            flow_layout: Rc::new(RefCell::new(FlowLayout::default())),
            viewed_state: Rc::clone(&viewed_state),
            target_key: target_key.clone(),
            segments: Rc::new(RefCell::new(Vec::new())),
        };

        let app = Rc::new(Self {
            window,
            repo,
            target,
            diff_data,
            pr_comments,
            commits: Rc::new(RefCell::new(Vec::new())),
            all_pr_comments: Rc::new(RefCell::new(Vec::new())),
            range_base: Rc::new(Cell::new(None)),
            range_head: Rc::new(Cell::new(None)),
            highlighter,
            file_tree: Rc::new(RefCell::new(Vec::new())),
            expanded_state: Rc::new(RefCell::new(HashMap::new())),
            viewed_state,
            target_key,
            renderer,
            pr_refs: RefCell::new(None),
            watcher: RefCell::new(None),
            reloading: Cell::new(false),
        });

        app.setup_callbacks()?;
        app.load_diff()?;
        if config.auto_reload {
            app.start_watching();
        }

        Ok(app)
    }

    /// Reload the diff by itself when the repository changes.
    ///
    /// The watching thread must not touch `repo`: `git2::Repository` is not
    /// `Send`. It only wakes the event loop, which raises `watch-triggered` and
    /// puts the work back on this thread.
    fn start_watching(self: &Rc<Self>) {
        let window_weak = self.window.as_weak();
        let result = DiffWatcher::spawn(self.repo.git_dir(), move || {
            let _ = window_weak.upgrade_in_event_loop(|w| w.invoke_watch_triggered());
        });
        match result {
            Ok(watcher) => *self.watcher.borrow_mut() = Some(watcher),
            // Losing the watcher costs the automatic reload, not the diff. F5
            // still works, so warn and carry on.
            Err(e) => eprintln!("Warning: automatic reload is off: {}", e),
        }
    }

    fn setup_callbacks(self: &Rc<Self>) -> Result<()> {
        let window_weak = self.window.as_weak();
        let diff_data = Rc::clone(&self.diff_data);
        let renderer = self.renderer.clone();
        let viewed_state_for_select = Rc::clone(&self.viewed_state);
        let target_key_for_select = self.target_key.clone();

        // File selection callback
        self.window.on_file_selected(move |path| {
            let window = window_weak.unwrap();
            let path_str = path.to_string();

            // A new file starts at the default width — expansion is scoped to
            // the file you were looking at, not carried across the tree.
            renderer.reset_level();
            renderer.render(&window, &path_str);

            let data_borrow = diff_data.borrow();
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

        // Flowing view: map the virtual scroll axis to each pane's offset. Reads
        // the current file's layout from the shared cell render() keeps fresh.
        let window_weak = self.window.as_weak();
        let flow_layout = Rc::clone(&self.renderer.flow_layout);
        self.window.on_flowing_remap(move |virtual_y| {
            let window = window_weak.unwrap();
            let (left_y, right_y) = flow_layout.borrow().map_scroll(virtual_y);
            window.set_flowing_left_y(left_y);
            window.set_flowing_right_y(right_y);
        });

        let window_weak = self.window.as_weak();
        self.window.on_toggle_fullscreen(move || {
            let window = window_weak.unwrap();
            let is_fullscreen = window.window().is_fullscreen();
            window.window().set_fullscreen(!is_fullscreen);
        });

        // F5 and the toolbar button. Does the full load, so for a pull request
        // it re-fetches from origin — this is how a change on the remote
        // reaches the screen.
        let app_weak = Rc::downgrade(self);
        self.window.on_refresh_diff(move || {
            if let Some(app) = app_weak.upgrade() {
                app.refresh();
            }
        });

        // The watcher saw the git directory change. Reload only if the commits
        // the view is built from actually moved: the directory also churns for
        // reasons no diff depends on, and a refresh of a pull request writes
        // into it itself.
        let app_weak = Rc::downgrade(self);
        self.window.on_watch_triggered(move || {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            if app.range_moved() {
                app.refresh();
            }
        });

        // Commit selection callback for PR commit navigation
        let window_weak = self.window.as_weak();
        let repo = Rc::clone(&self.repo);
        let all_commits = Rc::clone(&self.commits);
        let range_base = Rc::clone(&self.range_base);
        let range_head = Rc::clone(&self.range_head);
        let all_pr_comments = Rc::clone(&self.all_pr_comments);
        let renderer_for_commit = self.renderer.clone();
        self.window.on_commit_selected(move |idx| {
            let window = window_weak.unwrap();
            let commits = all_commits.borrow();
            let comments = all_pr_comments.borrow();

            // The endpoints this view is diffing, kept alongside the result so
            // later renders (and context expansion) work against the same pair.
            let mut scope: Option<(Oid, Oid)> = None;
            let diff_result: Option<(Result<DiffData>, Option<FileComments>)> = if idx < 0 {
                // "All changes" - diff base to head
                if let (Some(b), Some(h)) = (range_base.get(), range_head.get()) {
                    // Show all comments for full diff
                    let grouped = github::group_comments_by_file(comments.clone());
                    scope = Some((b, h));
                    Some((repo.diff_commits(b, h), Some(grouped)))
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
                        scope = Some((p, c));
                        Some((repo.diff_commits(p, c), Some(grouped)))
                    } else {
                        None
                    }
                } else {
                    // Root commit - no parent to diff against, so fall back to
                    // comparing it with the range base.
                    let commit_oid = repo.resolve_ref(&commit.sha).ok();
                    if let (Some(b), Some(c)) = (range_base.get(), commit_oid) {
                        let filtered: Vec<_> = comments
                            .iter()
                            .filter(|c| c.original_commit_id == commit.sha)
                            .cloned()
                            .collect();
                        let grouped = github::group_comments_by_file(filtered);
                        scope = Some((b, c));
                        Some((repo.diff_commits(b, c), Some(grouped)))
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            if let Some((Ok(mut diff_data), grouped_comments)) = diff_result {
                diff_data.expand_tabs(window.get_app_settings().tab_width as usize);
                // Publish before rendering: every later interaction (selecting
                // another file, widening the context) reads this shared state,
                // and leaving it on the previous diff is what made the tree and
                // the diff view disagree.
                if let Some((base, head)) = scope {
                    renderer_for_commit.set_scope(base, head);
                }
                // Build hierarchical file tree and flatten for UI
                // Use empty expanded state for commit-specific views (fresh view each time)
                let tree = build_file_tree(&diff_data.files);
                let flat_entries = flatten_tree_with_state(&tree, 0, &HashMap::new());
                set_diff_summary(&window, &diff_data);

                let file_entries =
                    build_file_entries(&flat_entries, grouped_comments.as_ref(), Some(&diff_data), None);

                *renderer_for_commit.diff_data.borrow_mut() = Some(diff_data);
                *renderer_for_commit.pr_comments.borrow_mut() = grouped_comments;

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
                        renderer_for_commit.render(&window, &initial.path);
                    }
                }
            }
        });

        // Settings changed callback
        let highlighter = Rc::clone(&self.highlighter);
        let window_weak = self.window.as_weak();
        let renderer_for_settings = self.renderer.clone();
        self.window.on_settings_changed(move |settings| {
            // Persist settings to config file
            let window = window_weak.unwrap();
            let config = crate::config::Config {
                ui_theme: settings.ui_theme.to_string(),
                font_size: settings.font_size,
                tab_width: settings.tab_width,
                line_wrap_column: settings.line_wrap_column,
                panel_width: window.get_left_panel_width(),
                key_unified: settings.key_unified.to_string(),
                key_side_by_side: settings.key_side_by_side.to_string(),
                key_flowing: settings.key_flowing.to_string(),
                key_scroll_down: settings.key_scroll_down.to_string(),
                key_scroll_up: settings.key_scroll_up.to_string(),
                key_file_next: settings.key_file_next.to_string(),
                key_file_prev: settings.key_file_prev.to_string(),
                key_prev_commit: settings.key_prev_commit.to_string(),
                key_next_commit: settings.key_next_commit.to_string(),
                key_expand_context: settings.key_expand_context.to_string(),
                key_collapse_context: settings.key_collapse_context.to_string(),
                // Carry over every key the settings panel does not show. Without
                // this, saving the panel resets them to their defaults.
                ..crate::config::load()
            };
            if let Err(e) = crate::config::save(&config) {
                eprintln!("Warning: Could not save settings: {}", e);
            }

            highlighter.borrow_mut().set_theme(settings.ui_theme.as_str());

            // Re-highlight currently selected file, keeping whatever context
            // width the user had expanded to.
            let selected_file = window.get_selected_file().to_string();
            renderer_for_settings.render(&window, &selected_file);
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
        let renderer = self.renderer.clone();
        self.window.on_toggle_viewed(move |idx| {
            let window = window_weak.unwrap();
            let files = window.get_files();

            if let Some(entry) = files.row_data(idx as usize) {
                if entry.is_folder {
                    return;
                }

                let path = entry.path.to_string();
                let now_viewed = !entry.viewed;
                {
                    let data = diff_data.borrow();
                    let empty = HashMap::new();
                    let file_hunks = data.as_ref().map_or(&empty, |d| &d.file_hunks);
                    let mut vs = viewed_state.borrow_mut();
                    set_file_viewed_state(&mut vs, &target_key, &path, file_hunks, now_viewed);
                    if let Err(e) = vs.save() {
                        eprintln!("Warning: Could not save viewed state: {}", e);
                    }
                }

                // Toggle in the UI model directly
                let model = files
                    .as_any()
                    .downcast_ref::<VecModel<FileEntry>>()
                    .unwrap();
                let mut updated = entry.clone();
                updated.viewed = now_viewed;
                model.set_row_data(idx as usize, updated);

                // If the toggled file is the one currently displayed, keep the
                // diff header's checkbox in sync and redraw, so every segment
                // collapses or expands with it.
                if window.get_selected_file().to_string() == path {
                    window.set_selected_file_viewed(now_viewed);
                    renderer.render(&window, &path);
                }
            }
        });

        // Toggle viewed state for the currently-displayed file (works even when
        // the file is hidden from the tree by a collapsed ancestor).
        let window_weak = self.window.as_weak();
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        let diff_data = Rc::clone(&self.diff_data);
        let renderer = self.renderer.clone();
        self.window.on_toggle_selected_viewed(move || {
            let window = window_weak.unwrap();
            let path = window.get_selected_file().to_string();
            if path.is_empty() {
                return;
            }

            let mut vs = viewed_state.borrow_mut();
            let data_borrow = diff_data.borrow();
            let was_viewed = is_path_viewed(&path, &vs, data_borrow.as_ref(), &target_key);

            let empty = HashMap::new();
            let file_hunks = data_borrow.as_ref().map_or(&empty, |d| &d.file_hunks);
            set_file_viewed_state(&mut vs, &target_key, &path, file_hunks, !was_viewed);

            if let Err(e) = vs.save() {
                eprintln!("Warning: Could not save viewed state: {}", e);
            }
            drop(vs);
            drop(data_borrow);

            window.set_selected_file_viewed(!was_viewed);
            // Redraw so every segment collapses or expands with the file.
            renderer.render(&window, &path);

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

        // Toggle one change segment of the current file. The view reports the
        // segment's index; the hash behind it comes from the split the last
        // render remembered, so the two cannot disagree.
        let window_weak = self.window.as_weak();
        let viewed_state = Rc::clone(&self.viewed_state);
        let target_key = self.target_key.clone();
        let diff_data = Rc::clone(&self.diff_data);
        let renderer = self.renderer.clone();
        self.window.on_toggle_segment_viewed(move |idx| {
            let window = window_weak.unwrap();
            let path = window.get_selected_file().to_string();
            let Ok(index) = usize::try_from(idx) else { return };
            let Some(&hash) = renderer.segment_hashes().get(index) else {
                return;
            };

            let file_viewed = {
                let mut vs = viewed_state.borrow_mut();
                toggle_segment_mark(&mut vs, &target_key, &path, hash);
                if let Err(e) = vs.save() {
                    eprintln!("Warning: Could not save viewed state: {}", e);
                }
                is_path_viewed(&path, &vs, diff_data.borrow().as_ref(), &target_key)
            };

            // Redraw the file so the segment collapses or expands.
            renderer.render(&window, &path);

            // The file is viewed exactly when all of its segments are, so the
            // checkbox follows the segment that just moved.
            window.set_selected_file_viewed(file_viewed);
            let files = window.get_files();
            if let Some(model) = files.as_any().downcast_ref::<VecModel<FileEntry>>() {
                for i in 0..model.row_count() {
                    let Some(entry) = model.row_data(i) else { continue };
                    if entry.path == path {
                        let mut updated = entry.clone();
                        updated.viewed = file_viewed;
                        model.set_row_data(i, updated);
                        break;
                    }
                }
            }
        });

        // Context expansion: widen / narrow the unchanged code shown around
        // each change in the current file.
        let window_weak = self.window.as_weak();
        let renderer = self.renderer.clone();
        self.window.on_expand_context(move || {
            renderer.step_context(&window_weak.unwrap(), true);
        });

        let window_weak = self.window.as_weak();
        let renderer = self.renderer.clone();
        self.window.on_collapse_context(move || {
            renderer.step_context(&window_weak.unwrap(), false);
        });

        Ok(())
    }

    /// Publish the commit list to the UI. `total` is the size of the full range,
    /// which exceeds `commits.len()` when the list was truncated.
    fn set_commit_list(&self, total: usize, commits: Vec<CommitInfo>) {
        let entries: Vec<CommitEntry> = commits.iter().map(|c| CommitModel::from(c).into()).collect();
        self.window
            .set_commits(ModelRc::from(Rc::new(VecModel::from(entries))));
        self.window.set_total_commits(total as i32);
        *self.commits.borrow_mut() = commits;
    }

    /// Populate the commit list from a local revwalk of `base..head`. A failure
    /// here only costs commit navigation, so it warns rather than aborting the
    /// diff the user asked for.
    fn load_local_commits(&self, base_oid: Oid, head_oid: Oid) {
        match self
            .repo
            .commits_in_range(base_oid, head_oid, COMMIT_LIST_LIMIT)
        {
            Ok(commits) => {
                let total = self
                    .repo
                    .count_commits_ahead(base_oid, head_oid)
                    .unwrap_or(commits.len());
                self.set_commit_list(total, commits);
            }
            Err(e) => eprintln!("Warning: Could not list commits in range: {}", e),
        }
    }

    /// Rebuild the whole view from the repository, keeping the file that is on
    /// screen. A failure leaves the old view up: during a rebase `HEAD` points
    /// at nothing for a moment, and losing the diff over that would be worse
    /// than showing a stale one.
    fn refresh(&self) {
        if self.reloading.get() {
            return;
        }
        self.reloading.set(true);
        let keep = self.window.get_selected_file().to_string();
        let keep = (!keep.is_empty()).then_some(keep);
        if let Err(e) = self.load_diff_keeping(keep) {
            eprintln!("Warning: Could not reload the diff: {}", e);
        }
        self.reloading.set(false);
    }

    /// Whether the commit pair the view is built from has moved. Resolves the
    /// refs locally — no network, so the watcher can ask this on every event.
    fn range_moved(&self) -> bool {
        let resolved = match &self.target {
            DiffTarget::DefaultBranch => self
                .repo
                .find_default_branch()
                .and_then(|b| self.repo.resolve_ref(&b))
                .and_then(|base| Ok((base, self.repo.head_commit()?))),
            DiffTarget::Ref(name) => self
                .repo
                .resolve_ref(name)
                .and_then(|base| Ok((base, self.repo.head_commit()?))),
            DiffTarget::PullRequest(_) => {
                let refs = self.pr_refs.borrow();
                let Some((base_ref, head_ref)) = refs.as_ref() else {
                    return false;
                };
                self.repo
                    .resolve_ref(&format!("origin/{}", base_ref))
                    .or_else(|_| self.repo.resolve_ref(base_ref))
                    .and_then(|base| Ok((base, self.repo.resolve_ref(head_ref)?)))
            }
        };

        // A ref that will not resolve means the repository is mid-operation.
        // Wait for the next event rather than reloading into a broken state.
        let Ok((base, head)) = resolved else {
            return false;
        };
        (Some(base), Some(head)) != (self.range_base.get(), self.range_head.get())
    }

    fn load_diff(&self) -> Result<()> {
        self.load_diff_keeping(None)
    }

    /// Build the whole view from the repository. `keep` names the file to put
    /// back on screen; without it the view opens on the first unviewed file.
    fn load_diff_keeping(&self, keep: Option<String>) -> Result<()> {
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

                // Fetch latest base from origin so diff reflects current remote state,
                // not whatever the local checkout happens to be at.
                if let Err(e) = self.repo.fetch_remote_ref("origin", &pr_info.base_ref) {
                    eprintln!(
                        "Warning: could not fetch origin/{}: {}",
                        pr_info.base_ref, e
                    );
                }

                let origin_base_ref = format!("origin/{}", pr_info.base_ref);
                let base = self
                    .repo
                    .resolve_ref(&origin_base_ref)
                    .or_else(|_| self.repo.resolve_ref(&pr_info.base_ref))?;
                let head = self.repo.resolve_ref(&pr_info.head_ref)?;

                // Remember the names so the watcher can re-resolve them without
                // asking `gh` again.
                *self.pr_refs.borrow_mut() =
                    Some((pr_info.base_ref.clone(), pr_info.head_ref.clone()));

                // Detect stale: PR's recorded base SHA vs the fresh origin/<base> tip.
                let stale_note = match git2::Oid::from_str(&pr_info.base_oid) {
                    Ok(pr_base_oid) if pr_base_oid != base => self
                        .repo
                        .count_commits_ahead(pr_base_oid, base)
                        .map(|n| format_stale_base_note(&pr_info.base_ref, n))
                        .unwrap_or_else(|_| format!(" ⚠ behind {}", pr_info.base_ref)),
                    _ => String::new(),
                };

                // Update toolbar with PR title (plus stale warning if any)
                self.window.set_diff_title(
                    format!("PR #{}: {}{}", pr_num, pr_info.title, stale_note).into(),
                );

                // Fetch PR commits. Preferred over a local revwalk because the
                // API's SHAs are what review comments are keyed against.
                match github::get_pr_commits(*pr_num) {
                    Ok(commits) => self.set_commit_list(commits.len(), commits),
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

        self.range_base.set(Some(base_oid));
        self.range_head.set(Some(head_oid));
        self.renderer.set_scope(base_oid, head_oid);

        // The PR arm already filled the list from the GitHub API; for plain refs
        // the commits come from a local walk of base..HEAD.
        if !matches!(self.target, DiffTarget::PullRequest(_)) {
            self.load_local_commits(base_oid, head_oid);
        }

        // Compute the diff
        let mut diff_data = self.repo.diff_commits(base_oid, head_oid)?;
        diff_data.expand_tabs(self.window.get_app_settings().tab_width as usize);

        // Promote any marks written before segment-level viewing, before
        // anything reads the segment store.
        {
            let mut vs = self.viewed_state.borrow_mut();
            migrate_legacy_marks(
                &mut vs,
                &self.target_key,
                diff_data.files.iter().map(|f| f.path.as_str()),
                &diff_data.file_hunks,
            );
            if let Err(e) = vs.save() {
                eprintln!("Warning: Could not save viewed state: {}", e);
            }
        }

        // Build hierarchical file tree and flatten for UI
        let tree = build_file_tree(&diff_data.files);
        set_diff_summary(&self.window, &diff_data);
        let expanded_state = self.expanded_state.borrow();
        let flat_entries = flatten_tree_with_state(&tree, 0, &expanded_state);
        drop(expanded_state);

        let file_entries = build_file_entries(
            &flat_entries,
            self.pr_comments.borrow().as_ref(),
            Some(&diff_data),
            Some((&self.viewed_state.borrow(), &self.target_key)),
        );

        // Pick the focus row before moving file_entries into the model.
        let initial_focus = pick_focus_index(&file_entries, keep.as_deref());

        let files_model = Rc::new(VecModel::from(file_entries));
        self.window.set_files(ModelRc::from(files_model));

        // Load the diff for the initial focus row and keep focused-index in sync
        // with selected-file so the header "viewed" state is driven by the same row.
        let mut initial_path: Option<String> = None;
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
                initial_path = Some(initial.path.clone());
            }
        }

        // Store for later use in callbacks
        *self.file_tree.borrow_mut() = tree;
        *self.diff_data.borrow_mut() = Some(diff_data);

        // Renders from the shared state above, so it has to run after the store.
        if let Some(path) = initial_path {
            self.renderer.render(&self.window, &path);
        }

        Ok(())
    }

    pub fn run(self: Rc<Self>) -> Result<()> {
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
/// Set the toolbar's overall diff summary (files changed + total added/removed)
/// from the complete file list, so it stays stable regardless of which folders
/// are expanded in the tree.
fn set_diff_summary(window: &MainWindow, data: &DiffData) {
    let additions: usize = data.files.iter().map(|f| f.additions).sum();
    let deletions: usize = data.files.iter().map(|f| f.deletions).sum();
    window.set_files_changed(data.files.len() as i32);
    window.set_total_additions(additions as i32);
    window.set_total_deletions(deletions as i32);
}

fn get_lines_for_file(
    hunks: &[DiffHunk],
    path: &str,
    file_comments: Option<&Vec<github::PrComment>>,
    highlighter: &Highlighter,
    wrap_column: usize,
    segments: &[ChangeSegment],
) -> (Vec<DiffLine>, i32, i32) {
    use crate::git::{CommentData, DiffLine as GitDiffLine, DiffLineType};
    use crate::models::{parse_hex_color, wrap_diff_line};

    // First, collect all diff lines with their line numbers
    let diff_lines: Vec<GitDiffLine> = flatten_hunk_lines(hunks);

    // `segments` indexes into `diff_lines`. Inverting it once gives every
    // rendered row its segment in one lookup, including the extra rows that
    // wrapping produces from a single long line.
    let mut segment_of: Vec<i32> = vec![-1; diff_lines.len()];
    for (i, seg) in segments.iter().enumerate() {
        for slot in segment_of.iter_mut().take(seg.end).skip(seg.start) {
            *slot = i as i32;
        }
    }

    // Widest old/new line number → gutter digit count (0 = column has none).
    let old_gutter_digits = diff_lines
        .iter()
        .filter_map(|l| l.old_line_num)
        .max()
        .map_or(0, |n| n.to_string().len() as i32);
    let new_gutter_digits = diff_lines
        .iter()
        .filter_map(|l| l.new_line_num)
        .max()
        .map_or(0, |n| n.to_string().len() as i32);

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

    for (i, diff_line) in diff_lines.iter().enumerate() {
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

        // Wrap long lines into multiple visual rows (no-op when wrap_column == 0)
        for wrapped in wrap_diff_line(model, wrap_column) {
            let mut line: DiffLine = wrapped.into();
            line.segment_index = segment_of[i];
            result.push(line);
        }

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
                            side: comment.side,
                        }),
                    };
                    result.push(DiffLineModel::from(&comment_line).into());
                }
            }
        }
    }

    (result, old_gutter_digits, new_gutter_digits)
}

/// The file's hunks as one line stream, each hunk's header first.
///
/// This is the sequence a change segment is defined over, and the sequence
/// `get_lines_for_file` renders, so an index into it means the same thing to
/// both.
fn flatten_hunk_lines(hunks: &[DiffHunk]) -> Vec<crate::git::DiffLine> {
    use crate::git::{DiffLine as GitDiffLine, DiffLineType};

    hunks
        .iter()
        .cloned()
        .flat_map(|hunk| {
            // Hunk header line (trim the trailing newline git2 leaves on it).
            let header = GitDiffLine {
                line_type: DiffLineType::Hunk,
                old_line_num: None,
                new_line_num: None,
                content: hunk.header.trim_end().to_string(),
                comment: None,
            };
            std::iter::once(header).chain(hunk.lines)
        })
        .collect()
}

/// The same stream as [`flatten_hunk_lines`], classified and borrowed rather
/// than cloned. This runs for every file in the tree on every rebuild, so it
/// must not copy the diff to answer.
fn hunk_line_classes(hunks: &[DiffHunk]) -> Vec<(RowClass, &str)> {
    use crate::git::DiffLineType;

    hunks
        .iter()
        .flat_map(|hunk| {
            let header = std::iter::once((RowClass::Hunk, hunk.header.trim_end()));
            header.chain(hunk.lines.iter().map(|l| {
                let class = match l.line_type {
                    DiffLineType::Add => RowClass::Add,
                    DiffLineType::Remove => RowClass::Remove,
                    DiffLineType::Hunk => RowClass::Hunk,
                    // Comments are injected into the rendered rows later and
                    // never appear in a hunk; Context is the only other case.
                    _ => RowClass::Context,
                };
                (class, l.content.as_str())
            }))
        })
        .collect()
}

/// The change segments a reviewer can mark in one file.
///
/// Works straight off the hunks, so it costs no highlighting and no layout —
/// cheap enough to run for every file in the tree on every rebuild.
fn segments_of(hunks: &[DiffHunk]) -> Vec<ChangeSegment> {
    let classes = hunk_line_classes(hunks);
    segments_for_file(&classes, viewed_state::hash_diff_content(hunks))
}

/// Apply a file-level viewed toggle.
///
/// The file checkbox is a bulk operation on the file's change segments (D4):
/// checking it marks every one, unchecking it clears every one. There is no
/// separate per-file fact to keep in step.
fn set_file_viewed_state(
    state: &mut ViewedState,
    target_key: &str,
    path: &str,
    file_hunks: &HashMap<String, Vec<DiffHunk>>,
    viewed: bool,
) {
    if !viewed {
        state.set_file_unviewed(target_key, path);
        return;
    }
    let empty = Vec::new();
    let hunks = file_hunks.get(path).unwrap_or(&empty);
    let hashes: Vec<u64> = segments_of(hunks).into_iter().map(|s| s.hash).collect();
    state.set_file_viewed(target_key, path, &hashes);
}

/// Flip one change segment's mark. Returns whether it is now viewed.
///
/// Extracted from the Slint callback so the decision — mark if unmarked, clear
/// if marked — is testable on its own rather than only through the UI.
fn toggle_segment_mark(
    state: &mut ViewedState,
    target_key: &str,
    path: &str,
    hash: u64,
) -> bool {
    if state.is_segment_viewed(target_key, path, hash) {
        state.set_segment_unviewed(target_key, path, hash);
        false
    } else {
        state.set_segment_viewed(target_key, path, hash);
        true
    }
}

/// Promote every legacy per-file mark in this diff to segment marks.
///
/// Runs once when a diff loads, before the file tree is built. Migrating
/// lazily, as each file is opened, would be cheaper but wrong: the tree's
/// checkboxes are derived from the segment store, so an un-migrated file would
/// read as unviewed and a reviewer upgrading from an older version would find
/// all of their marks apparently gone.
/// `paths` is every file in the diff, which is not the same as the keys of
/// `file_hunks`: a rename or a binary file appears in the diff with no hunks at
/// all, and still has a synthetic segment to carry its mark.
fn migrate_legacy_marks<'a>(
    state: &mut ViewedState,
    target_key: &str,
    paths: impl Iterator<Item = &'a str>,
    file_hunks: &HashMap<String, Vec<DiffHunk>>,
) {
    let empty = Vec::new();
    for path in paths {
        let hunks = file_hunks.get(path).unwrap_or(&empty);
        let hashes: Vec<u64> = segments_of(hunks).into_iter().map(|s| s.hash).collect();
        state.migrate_file(
            target_key,
            path,
            viewed_state::hash_diff_content(hunks),
            &hashes,
        );
    }
}

/// Whether every change segment of a file has been marked as viewed.
fn file_is_viewed(state: &ViewedState, target_key: &str, path: &str, hunks: &[DiffHunk]) -> bool {
    let hashes: Vec<u64> = segments_of(hunks).into_iter().map(|s| s.hash).collect();
    state.is_file_viewed(target_key, path, &hashes)
}

/// Classify one merged row for both the segment split and the flowing view.
///
/// One function, so a change segment and the ribbon over it can never be drawn
/// from two different readings of the same row (D1).
fn row_class(row: &DiffLine) -> RowClass {
    match row.line_type.as_str() {
        "add" => RowClass::Add,
        "remove" => RowClass::Remove,
        "hunk" => RowClass::Hunk,
        "seg-bar" => RowClass::CollapsedChange,
        "comment" => {
            let side = if row.comment_side.as_str() == "left" {
                Side::Left
            } else {
                Side::Right
            };
            RowClass::Comment(side)
        }
        // "context" and wrap-continuation rows (which keep their base
        // add/remove/context type but here only context reaches this arm).
        _ => RowClass::Context,
    }
}

/// Tag each row with the change segment it belongs to, collapsing the segments
/// the reviewer has already marked.
///
/// This is the single place the three views learn about segments. They all read
/// the same merged row list, so substituting one bar row for a viewed segment's
/// rows here collapses that segment in the unified view, in both panes of the
/// side-by-side view, and — because the flowing view routes the bar to both
/// panes — in one step in the flowing view too.
///
/// Each row already carries the index of the segment it came from (`-1` for a
/// row that belongs to none, such as a review comment card). `viewed[i]` says
/// whether `segments[i]` is marked; the two run in step.
///
/// A marked segment's code rows are replaced by one bar, emitted where the
/// first of them stood. Rows that belong to no segment pass through untouched —
/// a review comment anchored inside a change stays on screen after the code
/// around it is collapsed.
fn apply_segments(
    rows: Vec<DiffLine>,
    segments: &[ChangeSegment],
    viewed: &[bool],
) -> Vec<DiffLine> {
    let mut out: Vec<DiffLine> = Vec::with_capacity(rows.len());
    let mut open: Option<i32> = None;
    let mut barred: Vec<bool> = vec![false; segments.len()];

    for mut row in rows {
        let index = row.segment_index;
        let Some(seg) = usize::try_from(index).ok().and_then(|i| segments.get(i)) else {
            row.segment_first = false;
            out.push(row);
            continue;
        };

        if viewed.get(index as usize).copied().unwrap_or(false) {
            // One bar per segment, however many runs its rows arrive in.
            if !std::mem::replace(&mut barred[index as usize], true) {
                out.push(collapsed_bar(seg, index));
            }
            continue;
        }

        // The toggle hangs off the segment's first row, so mark that one only.
        row.segment_first = open != Some(index);
        open = Some(index);
        row.segment_additions = seg.additions as i32;
        row.segment_deletions = seg.deletions as i32;
        out.push(row);
    }
    out
}

fn collapsed_bar(seg: &ChangeSegment, index: i32) -> DiffLine {
    DiffLine {
        line_type: "seg-bar".into(),
        segment_index: index,
        segment_additions: seg.additions as i32,
        segment_deletions: seg.deletions as i32,
        ..Default::default()
    }
}

/// Half-thickness of the seam an insert/delete draws across its empty pane. The
/// full line is `2 * SEAM_HALF` logical px — sub-pixel, so it renders as a faint
/// anti-aliased hairline (a JetBrains-style change marker); tune on-screen.
const FLOW_SEAM_HALF: f32 = 0.125;

/// Split the merged diff rows into the flowing view's two panes and the ribbons
/// between change blocks. `row_h`/`comment_h` must match the heights the flowing
/// `.slint` renders (it is handed the same values), so the panes and the ribbons
/// stay locked together.
fn build_flow_from_rows(
    rows: &[DiffLine],
    row_h: f32,
    comment_h: f32,
) -> (Vec<DiffLine>, Vec<DiffLine>, Vec<Ribbon>, FlowLayout) {
    let scene_rows: Vec<SceneRow> = rows
        .iter()
        .map(|r| {
            let class = row_class(r);
            let height = match class {
                RowClass::Comment(_) => comment_h,
                _ => row_h,
            };
            SceneRow { class, height }
        })
        .collect();

    let scene = build_scene(&scene_rows);
    let layout = FlowLayout::build(&scene.segments);

    let left: Vec<DiffLine> = scene.left_rows.iter().map(|&i| rows[i].clone()).collect();
    let right: Vec<DiffLine> = scene.right_rows.iter().map(|&i| rows[i].clone()).collect();

    // One ribbon per change block, in content-space y. An empty side collapses
    // to a thin seam band centred on the change so the connector tapers to a
    // hairline rather than a bare point.
    let ribbons: Vec<Ribbon> = layout
        .segments
        .iter()
        .filter(|s| s.kind == SegKind::Change)
        .map(|s| {
            let (left_top, left_bottom) = if s.left_h == 0.0 {
                (s.left_start - FLOW_SEAM_HALF, s.left_start + FLOW_SEAM_HALF)
            } else {
                (s.left_start, s.left_start + s.left_h)
            };
            let (right_top, right_bottom) = if s.right_h == 0.0 {
                (s.right_start - FLOW_SEAM_HALF, s.right_start + FLOW_SEAM_HALF)
            } else {
                (s.right_start, s.right_start + s.right_h)
            };
            Ribbon {
                left_top,
                left_bottom,
                right_top,
                right_bottom,
                left_empty: s.left_h == 0.0,
                right_empty: s.right_h == 0.0,
            }
        })
        .collect();

    (left, right, ribbons, layout)
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

#[cfg(test)]
mod tests {
    use super::{apply_segments, format_stale_base_note, pick_focus_index};
    use crate::segments::ChangeSegment;
    use crate::DiffLine;
    use crate::FileEntry;

    /// A rendered row, already carrying the index of the segment it came from
    /// (`-1` for a row that belongs to none), as `get_lines_for_file` tags it.
    fn row(line_type: &str, content: &str, segment: i32) -> DiffLine {
        DiffLine {
            line_type: line_type.into(),
            content: content.into(),
            segment_index: segment,
            ..Default::default()
        }
    }

    fn segment(hash: u64) -> ChangeSegment {
        ChangeSegment { hash, start: 0, end: 0, additions: 1, deletions: 1 }
    }

    /// The rows of one file: context, a two-line edit, context.
    fn one_edit() -> (Vec<DiffLine>, Vec<ChangeSegment>) {
        let rows = vec![
            row("context", "a", -1),
            row("remove", "old", 0),
            row("add", "new", 0),
            row("context", "b", -1),
        ];
        (rows, vec![segment(7)])
    }

    #[test]
    fn a_viewed_segment_collapses_to_one_bar_row() {
        let (rows, segments) = one_edit();

        let out = apply_segments(rows, &segments, &[true]);

        // Both the removed and the added row are gone, replaced by one bar that
        // reports what it stands for.
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].line_type, "seg-bar");
        assert_eq!(out[1].segment_index, 0);
        assert_eq!(out[1].segment_additions, 1);
        assert_eq!(out[1].segment_deletions, 1);
        assert_eq!(out[0].line_type, "context");
        assert_eq!(out[2].line_type, "context");
    }

    #[test]
    fn an_unviewed_segment_keeps_its_rows_and_marks_only_the_first() {
        let (rows, segments) = one_edit();

        let out = apply_segments(rows, &segments, &[false]);

        assert_eq!(out.len(), 4);
        assert_eq!(out[1].line_type, "remove");
        assert_eq!(out[2].line_type, "add");
        // The toggle hangs off the first row of the run, once.
        assert!(out[1].segment_first);
        assert!(!out[2].segment_first);
        assert_eq!(out[1].segment_index, 0);
        assert_eq!(out[2].segment_index, 0);
        // Context rows belong to no segment.
        assert_eq!(out[0].segment_index, -1);
        assert_eq!(out[3].segment_index, -1);
    }

    #[test]
    fn collapsing_one_segment_does_not_shift_the_index_of_the_next() {
        // The row list shrinks as segments collapse, so the index a toggle
        // reports has to keep counting segments, not rows.
        let rows = vec![
            row("remove", "a1", 0),
            row("add", "a2", 0),
            row("context", "-", -1),
            row("remove", "b1", 1),
            row("add", "b2", 1),
        ];

        let out = apply_segments(rows, &[segment(1), segment(2)], &[true, false]);

        assert_eq!(out.len(), 4);
        assert_eq!(out[0].line_type, "seg-bar");
        assert_eq!(out[0].segment_index, 0);
        assert_eq!(out[1].line_type, "context");
        assert_eq!(out[2].line_type, "remove");
        assert_eq!(out[2].segment_index, 1);
        assert!(out[2].segment_first);
        assert_eq!(out[3].segment_index, 1);
        assert!(!out[3].segment_first);
    }

    #[test]
    fn collapsing_a_segment_keeps_a_review_comment_anchored_inside_it() {
        // A comment card belongs to no segment. Hiding a reviewer's comment
        // because the code beside it was marked read would lose real content.
        let rows = vec![
            row("remove", "old", 0),
            row("comment", "", -1),
            row("add", "new", 0),
        ];

        let out = apply_segments(rows, &[segment(7)], &[true]);

        // One bar for the whole segment, however many runs its rows arrive in.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].line_type, "seg-bar");
        assert_eq!(out[1].line_type, "comment");
    }

    #[test]
    fn the_two_hunk_flattenings_index_the_same_lines() {
        // A segment's range indexes the stream `hunk_line_classes` produces,
        // and `get_lines_for_file` renders the stream `flatten_hunk_lines`
        // produces. If the two ever disagree, every segment tag lands on the
        // wrong row and the collapse hides the wrong code.
        use crate::git::{DiffHunk, DiffLine as GitDiffLine, DiffLineType};

        let line = |t: DiffLineType, c: &str| GitDiffLine {
            line_type: t,
            old_line_num: None,
            new_line_num: None,
            content: c.to_string(),
            comment: None,
        };
        let hunks = vec![
            DiffHunk {
                header: "@@ -1,2 +1,2 @@\n".to_string(),
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    line(DiffLineType::Context, "keep"),
                    line(DiffLineType::Remove, "old"),
                    line(DiffLineType::Add, "new"),
                ],
            },
            DiffHunk {
                header: "@@ -9,1 +9,1 @@\n".to_string(),
                old_start: 9,
                old_lines: 1,
                new_start: 9,
                new_lines: 1,
                lines: vec![line(DiffLineType::Add, "tail")],
            },
        ];

        let owned = super::flatten_hunk_lines(&hunks);
        let classes = super::hunk_line_classes(&hunks);

        assert_eq!(owned.len(), classes.len());
        for (line, (_, text)) in owned.iter().zip(&classes) {
            assert_eq!(line.content, *text);
        }
    }

    /// One file whose single hunk adds `text`.
    fn hunks_adding(text: &str) -> Vec<crate::git::DiffHunk> {
        use crate::git::{DiffHunk, DiffLine as GitDiffLine, DiffLineType};

        vec![DiffHunk {
            header: "@@ -1,1 +1,1 @@\n".to_string(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![GitDiffLine {
                line_type: DiffLineType::Add,
                old_line_num: None,
                new_line_num: Some(1),
                content: text.to_string(),
                comment: None,
            }],
        }]
    }

    #[test]
    fn editing_a_segment_returns_it_to_unviewed() {
        use crate::viewed_state::ViewedState;

        let before = hunks_adding("added");
        let mut state = ViewedState::default();
        let hashes: Vec<u64> = super::segments_of(&before).into_iter().map(|s| s.hash).collect();
        state.set_file_viewed("ref:main", "f.rs", &hashes);
        assert!(super::file_is_viewed(&state, "ref:main", "f.rs", &before));

        // The same segment, one character different. Its key changes, so the
        // reviewer has to look at it again.
        let after = hunks_adding("added!");

        assert!(!super::file_is_viewed(&state, "ref:main", "f.rs", &after));
    }

    #[test]
    fn a_segment_that_only_moved_stays_viewed() {
        use crate::git::{DiffHunk, DiffLine as GitDiffLine, DiffLineType};
        use crate::viewed_state::ViewedState;

        let before = hunks_adding("added");
        let mut state = ViewedState::default();
        let hashes: Vec<u64> = super::segments_of(&before).into_iter().map(|s| s.hash).collect();
        state.set_file_viewed("ref:main", "f.rs", &hashes);

        // Same edit, further down the file: different line numbers, different
        // hunk header, identical text.
        let mut moved: Vec<DiffHunk> = hunks_adding("added");
        moved[0].header = "@@ -90,1 +90,1 @@\n".to_string();
        moved[0].old_start = 90;
        moved[0].new_start = 90;
        moved[0].lines.insert(
            0,
            GitDiffLine {
                line_type: DiffLineType::Context,
                old_line_num: Some(90),
                new_line_num: Some(90),
                content: "unchanged".to_string(),
                comment: None,
            },
        );

        assert!(super::file_is_viewed(&state, "ref:main", "f.rs", &moved));
    }

    #[test]
    fn toggling_a_segment_flips_it_in_both_directions() {
        use crate::viewed_state::ViewedState;

        let mut state = ViewedState::default();

        // Unviewed -> viewed.
        assert!(super::toggle_segment_mark(&mut state, "ref:main", "f.rs", 7));
        assert!(state.is_segment_viewed("ref:main", "f.rs", 7));

        // Viewed -> unviewed. A7: the bar expands and the hash is gone.
        assert!(!super::toggle_segment_mark(&mut state, "ref:main", "f.rs", 7));
        assert!(!state.is_segment_viewed("ref:main", "f.rs", 7));
    }

    #[test]
    fn toggling_one_segment_leaves_its_neighbours_alone() {
        use crate::viewed_state::ViewedState;

        let mut state = ViewedState::default();
        state.set_file_viewed("ref:main", "f.rs", &[1, 2, 3]);

        super::toggle_segment_mark(&mut state, "ref:main", "f.rs", 2);

        assert!(state.is_segment_viewed("ref:main", "f.rs", 1));
        assert!(!state.is_segment_viewed("ref:main", "f.rs", 2));
        assert!(state.is_segment_viewed("ref:main", "f.rs", 3));
    }

    #[test]
    fn unchecking_a_file_clears_it_and_expands_every_segment() {
        // A8 end to end: the checkbox writes the state, the state drives
        // `file_is_viewed`, and `apply_segments` puts the rows back.
        use crate::viewed_state::ViewedState;
        use std::collections::HashMap;

        let hunks = hunks_adding("added");
        let file_hunks = HashMap::from([("f.rs".to_string(), hunks.clone())]);
        let mut state = ViewedState::default();

        super::set_file_viewed_state(&mut state, "ref:main", "f.rs", &file_hunks, true);
        assert!(super::file_is_viewed(&state, "ref:main", "f.rs", &hunks));

        super::set_file_viewed_state(&mut state, "ref:main", "f.rs", &file_hunks, false);

        assert!(!super::file_is_viewed(&state, "ref:main", "f.rs", &hunks));

        // Nothing is marked, so no row is replaced by a bar.
        let (rows, segments) = one_edit();
        let out = apply_segments(rows, &segments, &[false]);
        assert!(out.iter().all(|r| r.line_type != "seg-bar"));
    }

    #[test]
    fn widening_the_context_does_not_change_a_segments_key() {
        // Stepping the context re-diffs the file at more surrounding lines. The
        // change itself is untouched, so its key must not move — otherwise
        // expanding the context would silently drop every mark in the file, and
        // the file checkbox (which reads the un-widened hunks) would write
        // hashes the view could never match.
        use crate::git::{DiffLine as GitDiffLine, DiffLineType};

        let narrow = hunks_adding("added");

        let mut wide = hunks_adding("added");
        wide[0].header = "@@ -1,7 +1,8 @@\n".to_string();
        for (offset, text) in ["ctx one", "ctx two", "ctx three"].iter().enumerate() {
            wide[0].lines.insert(
                offset,
                GitDiffLine {
                    line_type: DiffLineType::Context,
                    old_line_num: Some(offset as u32 + 1),
                    new_line_num: Some(offset as u32 + 1),
                    content: text.to_string(),
                    comment: None,
                },
            );
        }
        wide[0].lines.push(GitDiffLine {
            line_type: DiffLineType::Context,
            old_line_num: Some(5),
            new_line_num: Some(6),
            content: "ctx after".to_string(),
            comment: None,
        });

        let keys = |h: &[crate::git::DiffHunk]| -> Vec<u64> {
            super::segments_of(h).into_iter().map(|s| s.hash).collect()
        };

        assert_eq!(keys(&narrow), keys(&wide));
    }

    #[test]
    fn upgrading_keeps_the_tree_checkboxes_of_files_marked_by_an_older_version() {
        // The legacy store holds one content hash per file. Until it is
        // promoted to segment hashes, `file_is_viewed` reports false — so
        // without a migration at load, a reviewer's marks would all appear to
        // vanish from the tree on the first launch after an upgrade.
        use crate::viewed_state::{hash_diff_content, ViewedState};
        use std::collections::HashMap;

        let hunks = hunks_adding("added");
        let file_hunks = HashMap::from([("src/app.rs".to_string(), hunks.clone())]);

        // State as an older version wrote it: the whole file, one hash.
        let mut state = ViewedState::default();
        state.set_legacy_file_viewed_for_test("ref:main", "src/app.rs", hash_diff_content(&hunks));

        super::migrate_legacy_marks(
            &mut state,
            "ref:main",
            ["src/app.rs"].into_iter(),
            &file_hunks,
        );

        assert!(super::file_is_viewed(&state, "ref:main", "src/app.rs", &hunks));
    }

    fn file(path: &str, viewed: bool) -> FileEntry {
        FileEntry {
            path: path.into(),
            name: path.into(),
            viewed,
            ..Default::default()
        }
    }

    /// A reload must put you back on the file you were reading, not on the
    /// first unviewed file in the tree.
    #[test]
    fn keeps_the_file_that_was_open() {
        let entries = [file("a.rs", false), file("b.rs", false)];
        assert_eq!(pick_focus_index(&entries, Some("b.rs")), 1);
    }

    /// The file can leave the diff between two reloads, or a collapsed folder
    /// can hide it. Focus then falls back to the first unviewed file.
    #[test]
    fn falls_back_when_the_kept_file_is_gone() {
        let entries = [file("a.rs", true), file("b.rs", false)];
        assert_eq!(pick_focus_index(&entries, Some("gone.rs")), 1);
        assert_eq!(pick_focus_index(&entries, None), 1);
    }

    #[test]
    fn stale_note_empty_when_up_to_date() {
        assert_eq!(format_stale_base_note("main", 0), "");
    }

    #[test]
    fn stale_note_singular() {
        assert_eq!(
            format_stale_base_note("main", 1),
            " ⚠ behind main by 1 commit"
        );
    }

    #[test]
    fn stale_note_plural() {
        assert_eq!(
            format_stale_base_note("develop", 76),
            " ⚠ behind develop by 76 commits"
        );
    }
}
