use super::commit::CommitInfo;
use super::diff::{DiffData, DiffHunk, DiffLine, DiffLineType, FileChange, FileStatus};
use anyhow::{anyhow, Context, Result};
use git2::{DiffOptions, Oid, Repository as Git2Repo};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Unchanged lines git keeps on each side of a change by default.
pub const DEFAULT_CONTEXT_LINES: u32 = 3;

/// Context width that makes libgit2 emit a file as a single whole-file hunk.
/// Comfortably larger than any plausible source file, yet far enough below
/// `u32::MAX` that libgit2's internal context arithmetic cannot overflow.
pub const FULL_FILE_CONTEXT_LINES: u32 = 1_000_000;

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

    /// Path of the git directory. The file watcher observes this tree: every
    /// change that can move the diff's endpoints lands somewhere under it.
    pub fn git_dir(&self) -> &Path {
        self.repo.path()
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
            if let Ok(_url) = remote.url() {
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

    /// Fetch a specific ref from a remote via `git fetch`.
    /// Uses the git CLI to inherit user's credential helpers and SSH config.
    pub fn fetch_remote_ref(&self, remote: &str, ref_name: &str) -> Result<()> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow!("Repository has no working directory"))?;

        let output = Command::new("git")
            .current_dir(workdir)
            .args(["fetch", remote, ref_name])
            .output()
            .context("Failed to execute git fetch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git fetch {} {} failed: {}", remote, ref_name, stderr));
        }
        Ok(())
    }

    /// Count commits reachable from `to` but not from `from` (i.e. how far `to` is ahead of `from`).
    pub fn count_commits_ahead(&self, from: Oid, to: Oid) -> Result<usize> {
        if from == to {
            return Ok(0);
        }
        let mut walk = self.repo.revwalk().context("Failed to create revwalk")?;
        walk.push(to).context("Failed to push 'to' onto revwalk")?;
        walk.hide(from)
            .context("Failed to hide 'from' on revwalk")?;
        Ok(walk.count())
    }

    /// Commits reachable from `head` but not from `base`, newest-first,
    /// truncated to the `limit` most recent.
    ///
    /// Topological sorting rather than the default date sort: commit timestamps
    /// can tie or run backwards (rebases, imports), and the list must line up
    /// with parent/child order for per-commit diffs to make sense. Newest-first
    /// is both the display order and what lets `limit` drop the oldest commits
    /// rather than the ones under review.
    pub fn commits_in_range(&self, base: Oid, head: Oid, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut walk = self.repo.revwalk().context("Failed to create revwalk")?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)
            .context("Failed to set revwalk sorting")?;
        walk.push(head).context("Failed to push 'head' onto revwalk")?;
        walk.hide(base).context("Failed to hide 'base' on revwalk")?;

        let mut commits = Vec::new();
        for oid in walk.take(limit) {
            let oid = oid.context("Failed to walk commit range")?;
            let commit = self
                .repo
                .find_commit(oid)
                .context("Failed to find commit in range")?;
            commits.push(CommitInfo::from_commit(&commit));
        }
        Ok(commits)
    }

    /// Compute diff between two commits, with the default amount of
    /// surrounding context.
    pub fn diff_commits(&self, base_oid: Oid, head_oid: Oid) -> Result<DiffData> {
        self.diff_trees(base_oid, head_oid, DEFAULT_CONTEXT_LINES, None)
    }

    /// Re-diff a single file with a chosen amount of surrounding context.
    ///
    /// Widening the context is delegated to libgit2 rather than spliced in from
    /// the file blob: that way hunks merge when their contexts meet, the last
    /// hunk clamps at EOF, and added/deleted/binary files need no special
    /// casing. Restricting to one path keeps the cost proportional to the file
    /// the user is looking at, not the whole diff.
    pub fn diff_file_at_context(
        &self,
        base_oid: Oid,
        head_oid: Oid,
        path: &str,
        context_lines: u32,
    ) -> Result<Vec<DiffHunk>> {
        let data = self.diff_trees(base_oid, head_oid, context_lines, Some(path))?;
        Ok(data.file_hunks.get(path).cloned().unwrap_or_default())
    }

    /// How many lines `path` has in each of the two trees, old side first.
    ///
    /// A side the file is absent from — or that holds something without lines,
    /// like a binary blob — counts as zero, which is what an added or deleted
    /// file's hunk reports for that side too.
    pub fn file_line_counts(&self, base_oid: Oid, head_oid: Oid, path: &str) -> Result<(u32, u32)> {
        Ok((
            self.blob_line_count(base_oid, path)?,
            self.blob_line_count(head_oid, path)?,
        ))
    }

    fn blob_line_count(&self, oid: Oid, path: &str) -> Result<u32> {
        let tree = self
            .repo
            .find_commit(oid)
            .context("Failed to find commit")?
            .tree()
            .context("Failed to get commit tree")?;

        // Absent from this side (added or deleted), or not a blob: no lines.
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(0);
        };
        let object = entry.to_object(&self.repo).context("Failed to read entry")?;
        let Some(blob) = object.as_blob() else {
            return Ok(0);
        };

        Ok(count_lines(blob.content()))
    }

    fn diff_trees(
        &self,
        base_oid: Oid,
        head_oid: Oid,
        context_lines: u32,
        pathspec: Option<&str>,
    ) -> Result<DiffData> {
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
        opts.context_lines(context_lines);
        if let Some(path) = pathspec {
            opts.pathspec(path);
        }

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

/// Count lines the way a diff does: a trailing newline terminates the last
/// line rather than starting an empty one, but content without it still ends
/// in a line.
fn count_lines(content: &[u8]) -> u32 {
    if content.is_empty() {
        return 0;
    }
    let terminated = content.iter().filter(|b| **b == b'\n').count();
    let unterminated_tail = usize::from(!content.ends_with(b"\n"));
    (terminated + unterminated_tail) as u32
}

#[cfg(test)]
mod tests {
    use super::super::diff::hunks_cover_whole_file;
    use super::*;
    use tempfile::TempDir;

    /// Build a repo with a linear history of `count` commits, each touching
    /// `file.txt`. Returns the temp dir (kept alive for the repo's lifetime),
    /// the repo, and the commit OIDs in creation order.
    fn linear_repo(count: usize) -> (TempDir, Repository, Vec<Oid>) {
        let dir = TempDir::new().expect("create temp dir");
        let git = Git2Repo::init(dir.path()).expect("git init");

        let mut oids = Vec::new();
        for i in 0..count {
            let blob = git
                .blob(format!("line {i}\n").as_bytes())
                .expect("write blob");
            let mut builder = git.treebuilder(None).expect("tree builder");
            builder
                .insert("file.txt", blob, git2::FileMode::Blob.into())
                .expect("insert blob");
            let tree_oid = builder.write().expect("write tree");
            let tree = git.find_tree(tree_oid).expect("find tree");

            let sig = git2::Signature::now("Tester", "tester@example.com").expect("signature");
            let parents: Vec<git2::Commit> = oids
                .last()
                .map(|oid| git.find_commit(*oid).expect("find parent"))
                .into_iter()
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            let oid = git
                .commit(
                    Some("HEAD"),
                    &sig,
                    &sig,
                    &format!("commit {i}"),
                    &tree,
                    &parent_refs,
                )
                .expect("commit");
            oids.push(oid);
        }

        (dir, Repository { repo: git }, oids)
    }

    /// A repo whose `name` holds `total` numbered lines, plus a second
    /// commit that rewrites line `edited` (1-based). Returns base and head.
    fn repo_with_one_line_edited(
        name: &str,
        total: usize,
        edited: usize,
    ) -> (TempDir, Repository, Oid, Oid) {
        let dir = TempDir::new().expect("create temp dir");
        let git = Git2Repo::init(dir.path()).expect("git init");

        let original: String = (1..=total).map(|i| format!("line {i}\n")).collect();
        let modified: String = (1..=total)
            .map(|i| {
                if i == edited {
                    "CHANGED\n".to_string()
                } else {
                    format!("line {i}\n")
                }
            })
            .collect();

        let mut oids = Vec::new();
        for content in [original, modified] {
            let blob = git.blob(content.as_bytes()).expect("write blob");
            let mut builder = git.treebuilder(None).expect("tree builder");
            builder
                .insert(name, blob, git2::FileMode::Blob.into())
                .expect("insert blob");
            let tree_oid = builder.write().expect("write tree");
            let tree = git.find_tree(tree_oid).expect("find tree");

            let sig = git2::Signature::now("Tester", "tester@example.com").expect("signature");
            let parents: Vec<git2::Commit> = oids
                .last()
                .map(|oid| git.find_commit(*oid).expect("find parent"))
                .into_iter()
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            let oid = git
                .commit(Some("HEAD"), &sig, &sig, "commit", &tree, &parent_refs)
                .expect("commit");
            oids.push(oid);
        }

        (dir, Repository { repo: git }, oids[0], oids[1])
    }

    fn has_line(hunks: &[DiffHunk], content: &str) -> bool {
        hunks
            .iter()
            .any(|h| h.lines.iter().any(|l| l.content == content))
    }

    #[test]
    fn full_context_reaches_lines_far_from_the_change() {
        let (_dir, repo, base, head) = repo_with_one_line_edited("file.txt", 30, 15);

        // Default context keeps the view tight around the edit...
        let narrow = repo
            .diff_file_at_context(base, head, "file.txt", DEFAULT_CONTEXT_LINES)
            .expect("narrow diff");
        assert!(has_line(&narrow, "CHANGED"));
        assert!(!has_line(&narrow, "line 1"));
        assert!(!has_line(&narrow, "line 30"));

        // ...while full context pulls in the whole file, top to bottom.
        let full = repo
            .diff_file_at_context(base, head, "file.txt", FULL_FILE_CONTEXT_LINES)
            .expect("full diff");
        assert!(has_line(&full, "line 1"));
        assert!(has_line(&full, "line 30"));
    }

    #[test]
    fn a_short_file_is_already_whole_at_the_default_context() {
        // Five lines with one edit: git's usual ±3 already reaches both ends,
        // so there is nothing left for an expansion to reveal.
        let (_dir, repo, base, head) = repo_with_one_line_edited("file.txt", 5, 3);
        let (old_total, new_total) = repo
            .file_line_counts(base, head, "file.txt")
            .expect("line counts");
        let hunks = repo
            .diff_file_at_context(base, head, "file.txt", DEFAULT_CONTEXT_LINES)
            .expect("narrow diff");

        assert!(hunks_cover_whole_file(&hunks, old_total, new_total));

        // A file long enough to have lines outside that window does not.
        let (_dir, repo, base, head) = repo_with_one_line_edited("long.txt", 30, 15);
        let (old_total, new_total) = repo
            .file_line_counts(base, head, "long.txt")
            .expect("line counts");
        let hunks = repo
            .diff_file_at_context(base, head, "long.txt", DEFAULT_CONTEXT_LINES)
            .expect("narrow diff");

        assert!(!hunks_cover_whole_file(&hunks, old_total, new_total));
    }

    #[test]
    fn a_middle_rung_can_reach_both_ends_of_a_middling_file() {
        // Saturation is a property of the rung on screen, not of the top of the
        // ladder: 40 lines edited in the middle are fully covered by ±25 even
        // though ±3 leaves plenty hidden.
        let (_dir, repo, base, head) = repo_with_one_line_edited("file.txt", 40, 20);
        let (old_total, new_total) = repo
            .file_line_counts(base, head, "file.txt")
            .expect("line counts");

        let narrow = repo
            .diff_file_at_context(base, head, "file.txt", 3)
            .expect("narrow diff");
        assert!(!hunks_cover_whole_file(&narrow, old_total, new_total));

        let wide = repo
            .diff_file_at_context(base, head, "file.txt", 25)
            .expect("wide diff");
        assert!(hunks_cover_whole_file(&wide, old_total, new_total));
    }

    #[test]
    fn expands_paths_containing_glob_metacharacters() {
        // libgit2 pathspecs are fnmatch globs by default, so a Next.js-style
        // dynamic route would be read as a character class and match nothing —
        // expansion would silently blank the file.
        let (_dir, repo, base, head) = repo_with_one_line_edited("[id].tsx", 30, 15);

        let hunks = repo
            .diff_file_at_context(base, head, "[id].tsx", FULL_FILE_CONTEXT_LINES)
            .expect("full diff");

        assert!(has_line(&hunks, "line 1"));
        assert!(has_line(&hunks, "CHANGED"));
    }

    #[test]
    fn test_open_current_dir() {
        // This test should pass when run from within a git repo
        let result = Repository::open_current_dir();
        assert!(result.is_ok());
    }

    #[test]
    fn commits_in_range_excludes_the_base() {
        let (_dir, repo, oids) = linear_repo(4);

        let commits = repo
            .commits_in_range(oids[0], oids[3], 10)
            .expect("walk range");

        assert_eq!(commits.len(), 3);
    }

    #[test]
    fn commits_in_range_is_newest_first() {
        let (_dir, repo, oids) = linear_repo(4);

        let commits = repo
            .commits_in_range(oids[0], oids[3], 10)
            .expect("walk range");

        // The sidebar lists these top-down, and reviewers read a commit list
        // like `git log`: the tip of the branch first.
        let summaries: Vec<&str> = commits.iter().map(|c| c.message.trim()).collect();
        assert_eq!(summaries, vec!["commit 3", "commit 2", "commit 1"]);
    }

    #[test]
    fn commits_in_range_limit_keeps_the_most_recent() {
        let (_dir, repo, oids) = linear_repo(4);

        let commits = repo
            .commits_in_range(oids[0], oids[3], 2)
            .expect("walk range");

        // Truncation drops the oldest, not the newest: when reviewing a long
        // range the recent commits are the interesting ones.
        let summaries: Vec<&str> = commits.iter().map(|c| c.message.trim()).collect();
        assert_eq!(summaries, vec!["commit 3", "commit 2"]);
    }

    #[test]
    fn commits_in_range_records_first_parent() {
        let (_dir, repo, oids) = linear_repo(4);

        let commits = repo
            .commits_in_range(oids[0], oids[3], 10)
            .expect("walk range");

        // The parent is what each commit's own diff is computed against.
        let parents: Vec<Option<String>> = commits.iter().map(|c| c.parent_sha.clone()).collect();
        assert_eq!(
            parents,
            vec![
                Some(oids[2].to_string()),
                Some(oids[1].to_string()),
                Some(oids[0].to_string()),
            ]
        );
    }
}
