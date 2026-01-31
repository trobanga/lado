use clap::{CommandFactory, Parser, ValueHint};
use clap_complete::{generate, Shell};
use std::io;

/// lado - Git diff viewer with a side-by-side interface
#[derive(Parser, Debug)]
#[command(name = "lado", version, about, long_about = None)]
pub struct Args {
    /// Target to diff against HEAD.
    /// Can be: branch name, commit hash, PR number (42 or #42).
    /// If omitted, diffs against main/master branch.
    #[arg(value_hint = ValueHint::Other)]
    pub target: Option<String>,

    /// Generate shell completions
    #[arg(long, value_enum)]
    pub completions: Option<Shell>,
}

/// The resolved diff target
#[derive(Debug, Clone)]
pub enum DiffTarget {
    /// Diff against the default branch (main/master)
    DefaultBranch,
    /// Diff against a specific git ref (branch or commit)
    Ref(String),
    /// Diff for a pull request
    PullRequest(u32),
}

impl DiffTarget {
    /// Parse the target argument into a DiffTarget
    pub fn parse(target: Option<&str>) -> Self {
        match target {
            None => DiffTarget::DefaultBranch,
            Some(s) => {
                // Check if it's a PR number (e.g., "42" or "#42")
                let pr_str = s.strip_prefix('#').unwrap_or(s);
                if let Ok(pr_num) = pr_str.parse::<u32>() {
                    // Only treat as PR if it looks like a PR number
                    // (pure digits or #digits)
                    if s.starts_with('#') || s.chars().all(|c| c.is_ascii_digit()) {
                        return DiffTarget::PullRequest(pr_num);
                    }
                }
                // Otherwise treat as a git ref
                DiffTarget::Ref(s.to_string())
            }
        }
    }
}

/// Generate shell completions to stdout
pub fn generate_completions(shell: Shell) {
    let mut cmd = Args::command();
    generate(shell, &mut cmd, "lado", &mut io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_default() {
        assert!(matches!(DiffTarget::parse(None), DiffTarget::DefaultBranch));
    }

    #[test]
    fn test_parse_branch() {
        assert!(matches!(
            DiffTarget::parse(Some("feature-branch")),
            DiffTarget::Ref(s) if s == "feature-branch"
        ));
    }

    #[test]
    fn test_parse_commit() {
        assert!(matches!(
            DiffTarget::parse(Some("abc123")),
            DiffTarget::Ref(s) if s == "abc123"
        ));
    }

    #[test]
    fn test_parse_pr_number() {
        assert!(matches!(
            DiffTarget::parse(Some("42")),
            DiffTarget::PullRequest(42)
        ));
    }

    #[test]
    fn test_parse_pr_hash() {
        assert!(matches!(
            DiffTarget::parse(Some("#42")),
            DiffTarget::PullRequest(42)
        ));
    }
}
