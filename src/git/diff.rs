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

impl DiffData {
    /// Replace tab characters with spaces in all line content.
    ///
    /// Slint's Text element renders raw `\t` as a replacement glyph,
    /// so tabs must be expanded before reaching the UI.
    pub fn expand_tabs(&mut self, tab_width: usize) {
        let spaces = " ".repeat(tab_width);
        for hunks in self.file_hunks.values_mut() {
            for hunk in hunks {
                for line in &mut hunk.lines {
                    if line.content.contains('\t') {
                        line.content = line.content.replace('\t', &spaces);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tabs_replaces_tabs_with_spaces() {
        let mut data = DiffData {
            files: vec![],
            file_hunks: HashMap::from([(
                "test.go".to_string(),
                vec![DiffHunk {
                    header: String::new(),
                    old_start: 1,
                    old_lines: 3,
                    new_start: 1,
                    new_lines: 3,
                    lines: vec![
                        DiffLine {
                            line_type: DiffLineType::Context,
                            old_line_num: Some(1),
                            new_line_num: Some(1),
                            content: "\t\tfmt.Println(\"hello\")".to_string(),
                            comment: None,
                        },
                        DiffLine {
                            line_type: DiffLineType::Add,
                            old_line_num: None,
                            new_line_num: Some(2),
                            content: "\treturn nil".to_string(),
                            comment: None,
                        },
                        DiffLine {
                            line_type: DiffLineType::Context,
                            old_line_num: Some(3),
                            new_line_num: Some(3),
                            content: "no tabs here".to_string(),
                            comment: None,
                        },
                    ],
                }],
            )]),
        };

        data.expand_tabs(4);

        let lines = &data.file_hunks["test.go"][0].lines;
        assert_eq!(lines[0].content, "        fmt.Println(\"hello\")");
        assert_eq!(lines[1].content, "    return nil");
        assert_eq!(lines[2].content, "no tabs here");
    }

    #[test]
    fn test_expand_tabs_custom_width() {
        let mut data = DiffData {
            files: vec![],
            file_hunks: HashMap::from([(
                "test.py".to_string(),
                vec![DiffHunk {
                    header: String::new(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![DiffLine {
                        line_type: DiffLineType::Context,
                        old_line_num: Some(1),
                        new_line_num: Some(1),
                        content: "\tindented".to_string(),
                        comment: None,
                    }],
                }],
            )]),
        };

        data.expand_tabs(2);

        assert_eq!(data.file_hunks["test.py"][0].lines[0].content, "  indented");
    }
}
