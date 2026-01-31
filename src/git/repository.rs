use super::diff::{DiffData, DiffHunk, DiffLine, DiffLineType, FileChange, FileStatus};
use anyhow::{anyhow, Context, Result};
use git2::{DiffOptions, Oid, Repository as Git2Repo};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

pub struct Repository {
    repo: Git2Repo,
}

impl Repository {
    /// Open the repository at the current directory
    pub fn open_current_dir() -> Result<Self> {
        let repo = Git2Repo::discover(".").context("Not a git repository")?;
        Ok(Self { repo })
    }

    /// Open a repository at the given path
    #[allow(dead_code)]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let repo = Git2Repo::open(path).context("Failed to open repository")?;
        Ok(Self { repo })
    }

    /// Find the default branch (main or master)
    pub fn find_default_branch(&self) -> Result<String> {
        // Try common default branch names
        for branch in &["main", "master"] {
            if self
                .repo
                .find_branch(branch, git2::BranchType::Local)
                .is_ok()
            {
                return Ok(branch.to_string());
            }
        }

        // Try to get from remote HEAD
        if let Ok(remote) = self.repo.find_remote("origin") {
            if let Some(_url) = remote.url() {
                // Check for origin/main or origin/master
                for branch in &["origin/main", "origin/master"] {
                    if self.repo.revparse_single(branch).is_ok() {
                        return Ok(branch.strip_prefix("origin/").unwrap().to_string());
                    }
                }
            }
        }

        Err(anyhow!("Could not find default branch (main or master)"))
    }

    /// Resolve a ref name to an OID
    pub fn resolve_ref(&self, ref_name: &str) -> Result<Oid> {
        // First try as a direct ref
        if let Ok(reference) = self.repo.find_reference(ref_name) {
            if let Some(oid) = reference.target() {
                return Ok(oid);
            }
        }

        // Try as a branch name
        if let Ok(branch) = self.repo.find_branch(ref_name, git2::BranchType::Local) {
            if let Some(reference) = branch.get().target() {
                return Ok(reference);
            }
        }

        // Try as a remote branch
        let remote_ref = format!("origin/{}", ref_name);
        if let Ok(branch) = self.repo.find_branch(&remote_ref, git2::BranchType::Remote) {
            if let Some(reference) = branch.get().target() {
                return Ok(reference);
            }
        }

        // Try revparse as last resort
        let obj = self
            .repo
            .revparse_single(ref_name)
            .context(format!("Could not resolve ref: {}", ref_name))?;

        Ok(obj.id())
    }

    /// Get the HEAD commit OID
    pub fn head_commit(&self) -> Result<Oid> {
        let head = self.repo.head().context("Failed to get HEAD")?;
        head.target().ok_or_else(|| anyhow!("HEAD has no target"))
    }

    /// Compute diff between two commits
    pub fn diff_commits(&self, base_oid: Oid, head_oid: Oid) -> Result<DiffData> {
        let base_commit = self
            .repo
            .find_commit(base_oid)
            .context("Failed to find base commit")?;
        let head_commit = self
            .repo
            .find_commit(head_oid)
            .context("Failed to find head commit")?;

        let base_tree = base_commit
            .tree()
            .context("Failed to get base commit tree")?;
        let head_tree = head_commit
            .tree()
            .context("Failed to get head commit tree")?;

        let mut opts = DiffOptions::new();
        opts.context_lines(3);

        let diff = self
            .repo
            .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))
            .context("Failed to compute diff")?;

        // Use RefCell to allow interior mutability in closures
        let files = RefCell::new(Vec::new());
        let file_hunks: RefCell<HashMap<String, Vec<DiffHunk>>> = RefCell::new(HashMap::new());

        diff.foreach(
            &mut |delta, _| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let status = match delta.status() {
                    git2::Delta::Added => FileStatus::Added,
                    git2::Delta::Deleted => FileStatus::Deleted,
                    git2::Delta::Modified => FileStatus::Modified,
                    git2::Delta::Renamed => FileStatus::Renamed,
                    _ => FileStatus::Modified,
                };

                files.borrow_mut().push(FileChange {
                    path,
                    status,
                    additions: 0,
                    deletions: 0,
                });

                true
            },
            None,
            Some(&mut |delta, hunk| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let hunk_header = String::from_utf8_lossy(hunk.header()).to_string();

                file_hunks
                    .borrow_mut()
                    .entry(path)
                    .or_default()
                    .push(DiffHunk {
                        header: hunk_header,
                        old_start: hunk.old_start(),
                        old_lines: hunk.old_lines(),
                        new_start: hunk.new_start(),
                        new_lines: hunk.new_lines(),
                        lines: Vec::new(),
                    });

                true
            }),
            Some(&mut |delta, _hunk, line| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let content = String::from_utf8_lossy(line.content())
                    .trim_end_matches('\n')
                    .to_string();
                let line_type = match line.origin() {
                    '+' => DiffLineType::Add,
                    '-' => DiffLineType::Remove,
                    ' ' => DiffLineType::Context,
                    _ => DiffLineType::Context,
                };

                let mut hunks = file_hunks.borrow_mut();
                if let Some(file_hunks) = hunks.get_mut(&path) {
                    if let Some(current_hunk) = file_hunks.last_mut() {
                        current_hunk.lines.push(DiffLine {
                            line_type,
                            old_line_num: line.old_lineno(),
                            new_line_num: line.new_lineno(),
                            content,
                            comment: None,
                        });

                        // Update file stats
                        let mut files = files.borrow_mut();
                        if let Some(file) = files.iter_mut().find(|f| f.path == path) {
                            match line_type {
                                DiffLineType::Add => file.additions += 1,
                                DiffLineType::Remove => file.deletions += 1,
                                _ => {}
                            }
                        }
                    }
                }

                true
            }),
        )
        .context("Failed to iterate diff")?;

        Ok(DiffData {
            files: files.into_inner(),
            file_hunks: file_hunks.into_inner(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_current_dir() {
        // This test should pass when run from within a git repo
        let result = Repository::open_current_dir();
        assert!(result.is_ok());
    }
}
