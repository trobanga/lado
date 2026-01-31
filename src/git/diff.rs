use std::collections::HashMap;

/// Status of a file in the diff
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl FileStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Modified => "modified",
            FileStatus::Deleted => "deleted",
            FileStatus::Renamed => "renamed",
        }
    }
}

/// A changed file in the diff
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
}

/// Type of a diff line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineType {
    Add,
    Remove,
    Context,
    Hunk,
    Comment,
}

/// Data for a comment line
#[derive(Debug, Clone, Default)]
pub struct CommentData {
    pub author: String,
    pub body: String,
    pub timestamp: String,
    pub is_reply: bool,
}

/// A single line in a diff
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: DiffLineType,
    pub old_line_num: Option<u32>,
    pub new_line_num: Option<u32>,
    pub content: String,
    pub comment: Option<CommentData>,
}

/// A hunk in a diff
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// Complete diff data
#[derive(Debug, Clone)]
pub struct DiffData {
    pub files: Vec<FileChange>,
    pub file_hunks: HashMap<String, Vec<DiffHunk>>,
}
