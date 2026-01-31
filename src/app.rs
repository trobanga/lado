use crate::cli::{Args, DiffTarget};
use crate::git::{build_file_tree, flatten_tree, DiffData, Repository};
use crate::github::{self, FileComments, PrCommit};
use crate::models::{DiffLineModel, FileEntryModel, PrCommitModel};
use crate::{DiffLine, FileEntry, MainWindow, PrCommitEntry};
use anyhow::{Context, Result};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
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
}

impl App {
    pub fn new(args: Args) -> Result<Self> {
        let window = MainWindow::new().context("Failed to create window")?;
        let repo = Rc::new(Repository::open_current_dir()?);
        let target = DiffTarget::parse(args.target.as_deref());

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
        };

        app.setup_callbacks()?;
        app.load_diff()?;

        Ok(app)
    }

    fn setup_callbacks(&self) -> Result<()> {
        let window_weak = self.window.as_weak();
        let diff_data = Rc::clone(&self.diff_data);
        let pr_comments = Rc::clone(&self.pr_comments);

        // File selection callback
        self.window.on_file_selected(move |path| {
            let window = window_weak.unwrap();
            let path_str = path.to_string();

            if let Some(ref data) = *diff_data.borrow() {
                let comments = pr_comments.borrow();
                let lines = get_lines_for_file(data, &path_str, comments.as_ref());
                window.set_lines(lines);
            }

            window.set_selected_file(path);
        });

        let window_weak = self.window.as_weak();
        self.window.on_toggle_view_mode(move || {
            let _window = window_weak.unwrap();
            println!("Toggle view mode");
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

            if let Some((Ok(diff_data), grouped_comments)) = diff_result {
                // Build hierarchical file tree and flatten for UI
                let tree = build_file_tree(&diff_data.files);
                let flat_entries = flatten_tree(&tree, 0);

                // Convert to UI models
                let file_entries: Vec<FileEntry> = flat_entries
                    .iter()
                    .map(|f| FileEntryModel::from(f).into())
                    .collect();

                let files_model = Rc::new(VecModel::from(file_entries));
                window.set_files(ModelRc::from(files_model));

                // If there are files, select the first file (not folder) and load its diff
                if let Some(first_file) = flat_entries.iter().find(|e| !e.is_folder) {
                    window.set_selected_file(first_file.path.clone().into());
                    let lines =
                        get_lines_for_file(&diff_data, &first_file.path, grouped_comments.as_ref());
                    window.set_lines(lines);
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
                let (base_ref, head_ref) = github::get_pr_refs(*pr_num)?;
                let base = self.repo.resolve_ref(&base_ref)?;
                let head = self.repo.resolve_ref(&head_ref)?;

                // Store refs for later commit navigation
                *self.pr_base_ref.borrow_mut() = Some(base_ref);
                *self.pr_head_ref.borrow_mut() = Some(head_ref);

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
        let diff_data = self.repo.diff_commits(base_oid, head_oid)?;

        // Build hierarchical file tree and flatten for UI
        let tree = build_file_tree(&diff_data.files);
        let flat_entries = flatten_tree(&tree, 0);

        // Convert to UI models
        let file_entries: Vec<FileEntry> = flat_entries
            .iter()
            .map(|f| FileEntryModel::from(f).into())
            .collect();

        let files_model = Rc::new(VecModel::from(file_entries));
        self.window.set_files(ModelRc::from(files_model));

        // If there are files, select the first file (not folder) and load its diff
        if let Some(first_file) = flat_entries.iter().find(|e| !e.is_folder) {
            self.window.set_selected_file(first_file.path.clone().into());
            let comments = self.pr_comments.borrow();
            let lines = get_lines_for_file(&diff_data, &first_file.path, comments.as_ref());
            self.window.set_lines(lines);
        }

        // Store for later use in callbacks
        *self.diff_data.borrow_mut() = Some(diff_data);

        Ok(())
    }

    pub fn run(self) -> Result<()> {
        self.window.run().context("Failed to run window")?;
        Ok(())
    }
}

/// Convert hunks for a file into Slint-compatible DiffLine model, interleaving comments
fn get_lines_for_file(
    data: &DiffData,
    path: &str,
    comments: Option<&FileComments>,
) -> ModelRc<DiffLine> {
    use crate::git::{CommentData, DiffLine as GitDiffLine, DiffLineType};

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

    // Build the final lines, interleaving comments
    let mut result: Vec<DiffLine> = Vec::new();

    for diff_line in diff_lines {
        // Add the diff line itself
        result.push(DiffLineModel::from(&diff_line).into());

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
