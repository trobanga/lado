use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::process::Command;

/// Represents PR branch information
#[derive(Debug)]
#[allow(dead_code)]
pub struct PrInfo {
    pub base_ref: String,
    pub head_ref: String,
    pub title: String,
}

/// Which side of the diff a comment is on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentSide {
    Left,  // Old/original code (deletions)
    Right, // New/modified code (additions)
}

/// A single PR review comment
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PrComment {
    pub id: u64,
    pub in_reply_to_id: Option<u64>,
    pub path: String,
    pub line: Option<u32>,
    pub side: CommentSide,
    pub body: String,
    pub author: String,
    pub created_at: String,
    pub commit_id: String,
    pub original_commit_id: String,
}

/// A single commit in a PR
#[derive(Debug, Clone)]
pub struct PrCommit {
    pub sha: String,
    pub short_sha: String,
    pub parent_sha: Option<String>,
    pub message: String,
    pub author: String,
}

/// Comments grouped by file path, then by line number
pub type FileComments = HashMap<String, Vec<PrComment>>;

/// Fetch PR information using the gh CLI
pub fn get_pr_info(pr_number: u32) -> Result<PrInfo> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "baseRefName,headRefName,title",
        ])
        .output()
        .context("Failed to execute gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh pr view failed: {}", stderr));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output")?;

    let base_ref = json["baseRefName"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing baseRefName"))?
        .to_string();

    let head_ref = json["headRefName"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing headRefName"))?
        .to_string();

    let title = json["title"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing title"))?
        .to_string();

    Ok(PrInfo {
        base_ref,
        head_ref,
        title,
    })
}

/// Get PR info including base/head refs and title
pub fn get_pr_refs(pr_number: u32) -> Result<PrInfo> {
    get_pr_info(pr_number)
}

/// Fetch PR review comments using the gh CLI
pub fn get_pr_comments(pr_number: u32) -> Result<Vec<PrComment>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{{owner}}/{{repo}}/pulls/{}/comments", pr_number),
            "--paginate",
        ])
        .output()
        .context("Failed to execute gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api failed: {}", stderr));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output")?;

    let comments_array = json.as_array().ok_or_else(|| anyhow!("Expected array"))?;

    let mut comments = Vec::new();
    for comment in comments_array {
        let id = comment["id"].as_u64().unwrap_or(0);
        let in_reply_to_id = comment["in_reply_to_id"].as_u64();
        let path = comment["path"].as_str().unwrap_or("").to_string();
        let line = comment["line"].as_u64().map(|n| n as u32);
        let side = match comment["side"].as_str() {
            Some("LEFT") => CommentSide::Left,
            _ => CommentSide::Right,
        };
        let body = comment["body"].as_str().unwrap_or("").to_string();
        let author = comment["user"]["login"].as_str().unwrap_or("").to_string();
        let created_at = comment["created_at"].as_str().unwrap_or("").to_string();

        let commit_id = comment["commit_id"].as_str().unwrap_or("").to_string();
        let original_commit_id = comment["original_commit_id"]
            .as_str()
            .unwrap_or("")
            .to_string();

        comments.push(PrComment {
            id,
            in_reply_to_id,
            path,
            line,
            side,
            body,
            author,
            created_at,
            commit_id,
            original_commit_id,
        });
    }

    Ok(comments)
}

/// Fetch commits for a PR using the gh CLI
pub fn get_pr_commits(pr_number: u32) -> Result<Vec<PrCommit>> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{{owner}}/{{repo}}/pulls/{}/commits", pr_number),
            "--paginate",
        ])
        .output()
        .context("Failed to execute gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api failed: {}", stderr));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output")?;

    let commits_array = json.as_array().ok_or_else(|| anyhow!("Expected array"))?;

    let mut commits = Vec::new();
    for commit in commits_array {
        let sha = commit["sha"].as_str().unwrap_or("").to_string();
        let short_sha = sha.chars().take(7).collect();
        let message = commit["commit"]["message"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let author = commit["commit"]["author"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let parent_sha = commit["parents"]
            .as_array()
            .and_then(|parents| parents.first())
            .and_then(|p| p["sha"].as_str())
            .map(|s| s.to_string());

        commits.push(PrCommit {
            sha,
            short_sha,
            parent_sha,
            message,
            author,
        });
    }

    Ok(commits)
}

/// Group comments by file path
pub fn group_comments_by_file(comments: Vec<PrComment>) -> FileComments {
    let mut grouped: FileComments = HashMap::new();
    for comment in comments {
        grouped.entry(comment.path.clone()).or_default().push(comment);
    }
    // Sort comments within each file by line number, then by creation time for threads
    for comments in grouped.values_mut() {
        comments.sort_by(|a, b| {
            let line_cmp = a.line.cmp(&b.line);
            if line_cmp == std::cmp::Ordering::Equal {
                // Sort by id to maintain thread order (reply ids are higher)
                a.id.cmp(&b.id)
            } else {
                line_cmp
            }
        });
    }
    grouped
}
