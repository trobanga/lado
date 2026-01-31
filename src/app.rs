use crate::cli::{Args, DiffTarget};
use crate::git::Repository;
use crate::github;
use crate::models::{DiffLineModel, FileEntryModel};
use crate::{FileEntry, MainWindow};
use anyhow::{Context, Result};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

pub struct App {
    window: MainWindow,
    repo: Repository,
    target: DiffTarget,
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
        };

        app.setup_callbacks()?;
        app.load_diff()?;

        Ok(app)
    }

    fn setup_callbacks(&self) -> Result<()> {
        let window_weak = self.window.as_weak();

        // File selection callback
        self.window.on_file_selected(move |path| {
            let window = window_weak.unwrap();
            // TODO: Load diff for selected file
            println!("Selected file: {}", path);
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

        // Convert to UI models
        let file_entries: Vec<FileEntry> = diff_data
            .files
            .iter()
            .map(|f| FileEntryModel::from(f).into())
            .collect();

        let files_model = Rc::new(VecModel::from(file_entries));
        self.window.set_files(ModelRc::from(files_model));

        // If there are files, select the first one
        if let Some(first_file) = diff_data.files.first() {
            self.window.set_selected_file(first_file.path.clone().into());

            // Load diff lines for first file
            if let Some(hunks) = diff_data.file_hunks.get(&first_file.path) {
                let lines: Vec<_> = hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .map(|l| DiffLineModel::from(l).into())
                    .collect();
                let lines_model = Rc::new(VecModel::from(lines));
                self.window.set_lines(ModelRc::from(lines_model));
            }
        }

        Ok(())
    }

    pub fn run(self) -> Result<()> {
        self.window.run().context("Failed to run window")?;
        Ok(())
    }
}
