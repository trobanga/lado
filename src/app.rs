use crate::cli::{Args, DiffTarget};
use crate::git::{build_file_tree, flatten_tree, DiffData, Repository};
use crate::github;
use crate::models::{DiffLineModel, FileEntryModel};
use crate::{DiffLine, FileEntry, MainWindow};
use anyhow::{Context, Result};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub struct App {
    window: MainWindow,
    repo: Repository,
    target: DiffTarget,
    diff_data: Rc<RefCell<Option<DiffData>>>,
}

impl App {
    pub fn new(args: Args) -> Result<Self> {
        let window = MainWindow::new().context("Failed to create window")?;
        let repo = Repository::open_current_dir()?;
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
        };

        app.setup_callbacks()?;
        app.load_diff()?;

        Ok(app)
    }

    fn setup_callbacks(&self) -> Result<()> {
        let window_weak = self.window.as_weak();
        let diff_data = Rc::clone(&self.diff_data);

        // File selection callback
        self.window.on_file_selected(move |path| {
            let window = window_weak.unwrap();
            let path_str = path.to_string();

            if let Some(ref data) = *diff_data.borrow() {
                let lines = get_lines_for_file(data, &path_str);
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
            let lines = get_lines_for_file(&diff_data, &first_file.path);
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

/// Convert hunks for a file into Slint-compatible DiffLine model
fn get_lines_for_file(data: &DiffData, path: &str) -> ModelRc<DiffLine> {
    use crate::git::{DiffLine as GitDiffLine, DiffLineType};

    let hunks = data.file_hunks.get(path).cloned().unwrap_or_default();
    let lines: Vec<DiffLine> = hunks
        .into_iter()
        .flat_map(|hunk| {
            // Create hunk header line (trim trailing newline from git2)
            let header_line = GitDiffLine {
                line_type: DiffLineType::Hunk,
                old_line_num: None,
                new_line_num: None,
                content: hunk.header.trim_end().to_string(),
            };
            // Prepend header to hunk lines
            std::iter::once(header_line).chain(hunk.lines)
        })
        .map(|l| DiffLineModel::from(&l).into())
        .collect();
    ModelRc::new(VecModel::from(lines))
}
