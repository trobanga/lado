use crate::git::CommitInfo;
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::process::Command;

/// Which side of the diff a comment is on. Defined in the git layer as the
/// diff-domain notion of side; re-exported here so `github::CommentSide` keeps
/// resolving and PR parsing maps its "LEFT"/"RIGHT" onto the shared type.
pub use crate::git::CommentSide;

/// Represents PR branch information
#[derive(Debug)]
#[allow(dead_code)]
pub struct PrInfo {
    pub base_ref: String,
    pub head_ref: String,
    pub title: String,
    /// SHA of base branch tip recorded by GitHub (PR's view of base).
    pub base_oid: String,
    /// SHA of PR head recorded by GitHub.
    pub head_oid: String,
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
    /// GraphQL node id of the review thread the comment belongs to.
    pub thread_id: String,
    /// Whether the comment's review thread is resolved.
    pub is_resolved: bool,
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
            "baseRefName,headRefName,title,baseRefOid,headRefOid",
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

    let base_oid = json["baseRefOid"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing baseRefOid"))?
        .to_string();

    let head_oid = json["headRefOid"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing headRefOid"))?
        .to_string();

    Ok(PrInfo {
        base_ref,
        head_ref,
        title,
        base_oid,
        head_oid,
    })
}

/// Get PR info including base/head refs and title
pub fn get_pr_refs(pr_number: u32) -> Result<PrInfo> {
    get_pr_info(pr_number)
}

/// The review threads of a PR with their comments. GraphQL, not REST, because
/// only `reviewThreads` knows whether a thread is resolved. `$endCursor` and
/// `pageInfo` are what `gh api graphql --paginate` needs to page through.
/// Only the threads are paged; a thread's comments past the first 100 are cut.
const REVIEW_THREADS_QUERY: &str = r#"
query($owner: String!, $repo: String!, $number: Int!, $endCursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes {
          id
          isResolved
          path
          diffSide
          comments(first: 100) {
            nodes {
              databaseId
              replyTo { databaseId }
              line
              body
              author { login }
              createdAt
              commit { oid }
              originalCommit { oid }
            }
          }
        }
      }
    }
  }
}"#;

/// Fetch PR review comments, resolved ones included, using the gh CLI
pub fn get_pr_comments(pr_number: u32) -> Result<Vec<PrComment>> {
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "--paginate",
            "-F",
            "owner={owner}",
            "-F",
            "repo={repo}",
            "-F",
            &format!("number={}", pr_number),
            "-f",
            &format!("query={}", REVIEW_THREADS_QUERY),
        ])
        .output()
        .context("Failed to execute gh CLI. Is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api graphql failed: {}", stderr));
    }

    parse_review_threads(&output.stdout)
}

/// Convert the `reviewThreads` output of `gh api graphql` into `PrComment`s.
///
/// With `--paginate`, gh prints one JSON document per page, back to back.
/// Path and side belong to the thread in GraphQL; every comment of the thread
/// gets a copy, so each comment still stands on its own.
fn parse_review_threads(output: &[u8]) -> Result<Vec<PrComment>> {
    let mut comments = Vec::new();
    for page in serde_json::Deserializer::from_slice(output).into_iter::<serde_json::Value>() {
        let page = page.context("Failed to parse gh output")?;
        let threads = page["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]
            .as_array()
            .ok_or_else(|| anyhow!("Expected reviewThreads.nodes array"))?;
        comments.extend(threads.iter().flat_map(thread_comments));
    }
    Ok(comments)
}

/// The comments of one review thread.
fn thread_comments(thread: &serde_json::Value) -> Vec<PrComment> {
    let str_at = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let side = match thread["diffSide"].as_str() {
        Some("LEFT") => CommentSide::Left,
        _ => CommentSide::Right,
    };
    let is_resolved = thread["isResolved"].as_bool().unwrap_or(false);
    let path = str_at(&thread["path"]);
    let thread_id = str_at(&thread["id"]);

    thread["comments"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|comment| PrComment {
            id: comment["databaseId"].as_u64().unwrap_or(0),
            in_reply_to_id: comment["replyTo"]["databaseId"].as_u64(),
            path: path.clone(),
            line: comment["line"].as_u64().map(|n| n as u32),
            side,
            body: str_at(&comment["body"]),
            author: str_at(&comment["author"]["login"]),
            created_at: str_at(&comment["createdAt"]),
            commit_id: str_at(&comment["commit"]["oid"]),
            original_commit_id: str_at(&comment["originalCommit"]["oid"]),
            thread_id: thread_id.clone(),
            is_resolved,
        })
        .collect()
}

/// Fetch commits for a PR using the gh CLI
pub fn get_pr_commits(pr_number: u32) -> Result<Vec<CommitInfo>> {
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

    parse_pr_commits(&json)
}

/// Convert the `pulls/{n}/commits` payload into `CommitInfo`s, newest-first.
///
/// The API lists a PR's commits oldest-first; the sidebar shows the branch tip
/// at the top, matching `commits_in_range` so the order does not depend on
/// whether the target was a PR or a local ref.
fn parse_pr_commits(json: &serde_json::Value) -> Result<Vec<CommitInfo>> {
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

        commits.push(CommitInfo {
            sha,
            short_sha,
            parent_sha,
            message,
            author,
        });
    }

    commits.reverse();
    Ok(commits)
}

/// Group the unresolved comments by file path.
///
/// Resolved threads are done with, so they neither show in the diff view nor
/// count towards a file's comment badge.
pub fn group_unresolved_by_file(comments: Vec<PrComment>) -> FileComments {
    let mut grouped: FileComments = HashMap::new();
    for comment in comments.into_iter().filter(|c| !c.is_resolved) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_commits_are_newest_first() {
        // The API returns a PR's commits oldest-first; the sidebar reads
        // top-down like `git log`, and must match the local-walk source.
        let json = serde_json::json!([
            {"sha": "aaa", "commit": {"message": "first", "author": {"name": "A"}}, "parents": [{"sha": "base"}]},
            {"sha": "bbb", "commit": {"message": "second", "author": {"name": "A"}}, "parents": [{"sha": "aaa"}]},
        ]);

        let commits = parse_pr_commits(&json).expect("parse commits");

        let messages: Vec<&str> = commits.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(messages, vec!["second", "first"]);
    }

    /// One `reviewThreads` page as `gh api graphql` prints it.
    fn threads_page(threads: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": threads,
        }}}}})
    }

    fn thread(
        id: &str,
        resolved: bool,
        path: &str,
        comments: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id, "isResolved": resolved, "path": path, "diffSide": "RIGHT",
            "comments": {"nodes": comments},
        })
    }

    fn comment(id: u64, reply_to: Option<u64>, line: u32) -> serde_json::Value {
        serde_json::json!({
            "databaseId": id,
            "replyTo": reply_to.map(|r| serde_json::json!({"databaseId": r})),
            "line": line, "body": format!("body {id}"),
            "author": {"login": "rev"}, "createdAt": "2026-01-31T13:02:49Z",
            "commit": {"oid": "head"}, "originalCommit": {"oid": "orig"},
        })
    }

    #[test]
    fn review_threads_become_comments() {
        let mut left = thread(
            "PRRT_1",
            false,
            "src/a.rs",
            serde_json::json!([comment(10, None, 7), comment(11, Some(10), 7)]),
        );
        left["diffSide"] = "LEFT".into();
        let page = threads_page(serde_json::json!([left]));

        let comments = parse_review_threads(page.to_string().as_bytes()).expect("parse threads");

        assert_eq!(comments.len(), 2);
        let (first, reply) = (&comments[0], &comments[1]);
        assert_eq!(first.id, 10);
        assert_eq!(first.in_reply_to_id, None);
        assert_eq!(reply.in_reply_to_id, Some(10));
        assert_eq!(first.path, "src/a.rs");
        assert_eq!(first.line, Some(7));
        assert_eq!(first.side, CommentSide::Left);
        assert_eq!(first.body, "body 10");
        assert_eq!(first.author, "rev");
        assert_eq!(first.created_at, "2026-01-31T13:02:49Z");
        assert_eq!(first.commit_id, "head");
        assert_eq!(first.original_commit_id, "orig");
    }

    /// A thread holding the single comment `id` on `line`.
    fn one_comment(thread_id: &str, resolved: bool, path: &str, id: u64, line: u32) -> serde_json::Value {
        thread(thread_id, resolved, path, serde_json::json!([comment(id, None, line)]))
    }

    #[test]
    fn comments_carry_their_threads_resolution() {
        let page = threads_page(serde_json::json!([
            one_comment("PRRT_open", false, "a.rs", 1, 3),
            one_comment("PRRT_done", true, "a.rs", 2, 5),
        ]));

        let comments = parse_review_threads(page.to_string().as_bytes()).expect("parse threads");

        let states: Vec<(&str, bool)> = comments
            .iter()
            .map(|c| (c.thread_id.as_str(), c.is_resolved))
            .collect();
        assert_eq!(states, vec![("PRRT_open", false), ("PRRT_done", true)]);
    }

    #[test]
    fn resolved_comments_are_left_out_of_file_comments() {
        let page = threads_page(serde_json::json!([
            one_comment("PRRT_open", false, "a.rs", 1, 3),
            one_comment("PRRT_done", true, "a.rs", 2, 5),
            one_comment("PRRT_gone", true, "b.rs", 3, 1),
        ]));
        let comments = parse_review_threads(page.to_string().as_bytes()).expect("parse threads");

        let grouped = group_unresolved_by_file(comments);

        let ids: Vec<u64> = grouped["a.rs"].iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1]);
        assert!(!grouped.contains_key("b.rs"), "a file with only resolved comments has no entry");
    }

    #[test]
    fn outdated_comment_has_no_line() {
        // GitHub nulls `line` once the commented line is gone from the diff.
        let mut outdated = one_comment("PRRT_old", false, "a.rs", 1, 3);
        outdated["comments"]["nodes"][0]["line"] = serde_json::Value::Null;
        let page = threads_page(serde_json::json!([outdated]));

        let comments = parse_review_threads(page.to_string().as_bytes()).expect("parse threads");

        assert_eq!(comments[0].line, None);
    }

    #[cfg(unix)]
    #[test]
    fn pr_comments_are_fetched_through_gh_graphql() {
        use std::os::unix::fs::PermissionsExt;

        // A stand-in `gh` that answers only the review-threads call for PR 7.
        let dir = tempfile::tempdir().expect("temp dir");
        let page = threads_page(serde_json::json!([one_comment("PRRT_1", true, "a.rs", 1, 3)]));
        std::fs::write(dir.path().join("page.json"), page.to_string()).expect("write page");
        let script = format!(
            "#!/bin/sh\ncase \"$*\" in\n  *'api graphql --paginate'*'owner={{owner}}'*'repo={{repo}}'*'number=7'*) cat '{}' ;;\n  *) echo \"unexpected: $*\" >&2; exit 1 ;;\nesac\n",
            dir.path().join("page.json").display()
        );
        let gh = dir.path().join("gh");
        std::fs::write(&gh, script).expect("write gh");
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("chmod gh");
        // Prepend, so the other tests still find `git` on PATH.
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![dir.path().to_path_buf()];
        dirs.extend(std::env::split_paths(&path));
        std::env::set_var("PATH", std::env::join_paths(dirs).expect("join PATH"));

        let comments = get_pr_comments(7).expect("fetch comments");

        let got: Vec<(u64, bool)> = comments.iter().map(|c| (c.id, c.is_resolved)).collect();
        assert_eq!(got, vec![(1, true)]);
    }

    #[test]
    fn every_paginated_page_is_read() {
        // `gh api graphql --paginate` prints one JSON document per page, back to back.
        let first = threads_page(serde_json::json!([one_comment("PRRT_1", false, "a.rs", 1, 3)]));
        let second = threads_page(serde_json::json!([one_comment("PRRT_2", false, "b.rs", 2, 4)]));
        let output = format!("{first}\n{second}");

        let comments = parse_review_threads(output.as_bytes()).expect("parse threads");

        let ids: Vec<u64> = comments.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }
}
