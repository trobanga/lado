/// A single commit in the range under review.
///
/// Sourced either from a local revwalk (`Repository::commits_in_range`) or from
/// the GitHub API (`github::get_pr_commits`), so the UI treats both the same.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub parent_sha: Option<String>,
    pub message: String,
    pub author: String,
}

impl CommitInfo {
    /// Build from a libgit2 commit. Only the first parent is recorded: a merge
    /// commit's diff is shown against its mainline, matching how GitHub renders
    /// a single commit.
    pub fn from_commit(commit: &git2::Commit) -> Self {
        let sha = commit.id().to_string();
        Self {
            short_sha: sha.chars().take(7).collect(),
            sha,
            parent_sha: commit.parent_id(0).ok().map(|id| id.to_string()),
            message: String::from_utf8_lossy(commit.message_bytes()).to_string(),
            author: commit.author().name().unwrap_or("").to_string(),
        }
    }
}
