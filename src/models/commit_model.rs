use crate::git::CommitInfo;
use crate::CommitEntry;

/// Model for a commit entry in the UI
pub struct CommitModel {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
}

impl From<&CommitInfo> for CommitModel {
    fn from(commit: &CommitInfo) -> Self {
        // Extract the first line of the commit message as the summary
        let summary = commit.message.lines().next().unwrap_or("").to_string();

        Self {
            sha: commit.sha.clone(),
            short_sha: commit.short_sha.clone(),
            summary,
            author: commit.author.clone(),
        }
    }
}

impl From<CommitModel> for CommitEntry {
    fn from(model: CommitModel) -> Self {
        Self {
            sha: model.sha.into(),
            short_sha: model.short_sha.into(),
            summary: model.summary.into(),
            author: model.author.into(),
        }
    }
}
