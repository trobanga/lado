use crate::github::PrCommit;
use crate::PrCommitEntry;

/// Model for a PR commit entry in the UI
pub struct PrCommitModel {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
    pub is_selected: bool,
}

impl From<&PrCommit> for PrCommitModel {
    fn from(commit: &PrCommit) -> Self {
        // Extract the first line of the commit message as the summary
        let summary = commit
            .message
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        Self {
            sha: commit.sha.clone(),
            short_sha: commit.short_sha.clone(),
            summary,
            author: commit.author.clone(),
            is_selected: false,
        }
    }
}

impl From<PrCommitModel> for PrCommitEntry {
    fn from(model: PrCommitModel) -> Self {
        Self {
            sha: model.sha.into(),
            short_sha: model.short_sha.into(),
            summary: model.summary.into(),
            author: model.author.into(),
            is_selected: model.is_selected,
        }
    }
}
