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

/// Whether `hunks` already put every line of the file on screen, given each
/// side's total line count.
///
/// A single hunk as long as the file can only be the file itself: libgit2 grows
/// hunks from the change outwards and merges them when their contexts meet, so
/// the moment one hunk accounts for every line there is no second region left
/// to reveal. This is what makes widening a short file a no-op long before the
/// top rung of the ladder.
pub fn hunks_cover_whole_file(hunks: &[DiffHunk], old_total: u32, new_total: u32) -> bool {
    match hunks {
        // Binary files and mode-only changes produce no hunks at all: there is
        // nothing more to show, at any context width.
        [] => true,
        [hunk] => hunk.old_lines >= old_total && hunk.new_lines >= new_total,
        _ => false,
    }
}

/// Replace tab characters with spaces in every line of `hunks`.
///
/// Slint's Text element renders a raw `\t` as a replacement glyph, so tabs must
/// be expanded before reaching the UI — on any hunks, whether they came from the
/// cached diff or were recomputed for a wider context.
pub fn expand_tabs_in_hunks(hunks: &mut [DiffHunk], tab_width: usize) {
    let spaces = " ".repeat(tab_width);
    for hunk in hunks {
        for line in &mut hunk.lines {
            if line.content.contains('\t') {
                line.content = line.content.replace('\t', &spaces);
            }
        }
    }
}

/// Complete diff data
#[derive(Debug, Clone)]
pub struct DiffData {
    pub files: Vec<FileChange>,
    pub file_hunks: HashMap<String, Vec<DiffHunk>>,
}

impl DiffData {
    /// Replace tab characters with spaces in all line content.
    pub fn expand_tabs(&mut self, tab_width: usize) {
        for hunks in self.file_hunks.values_mut() {
            expand_tabs_in_hunks(hunks, tab_width);
        }
    }

}

/// Render hunks as git-diff-style plaintext for the selectable-text view. Each
/// code line is prefixed with its diff sign (`+`/`-`/space) and hunk headers are
/// emitted verbatim, so the copied text mirrors what the diff view shows.
/// Comments live outside `hunk.lines` (they're injected into the UI model
/// later), so nothing extra is filtered.
pub fn selectable_text_for_hunks(hunks: &[DiffHunk]) -> String {
    let mut out: Vec<String> = Vec::new();
    for hunk in hunks {
        let header = hunk.header.trim_end();
        if !header.is_empty() {
            out.push(header.to_string());
        }
        for line in &hunk.lines {
            let sign = match line.line_type {
                DiffLineType::Add => "+",
                DiffLineType::Remove => "-",
                DiffLineType::Context => " ",
                // Hunk/Comment never appear inside hunk.lines; keep the match
                // exhaustive without emitting a stray prefix.
                DiffLineType::Hunk | DiffLineType::Comment => "",
            };
            out.push(format!("{sign}{}", line.content));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tabs_works_on_hunks_outside_a_diff_data() {
        // Hunks recomputed at a wider context arrive as a bare Vec rather than
        // inside DiffData, but need the same treatment before reaching the UI.
        let mut hunks = vec![DiffHunk {
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
        }];

        expand_tabs_in_hunks(&mut hunks, 4);

        assert_eq!(hunks[0].lines[0].content, "    indented");
    }

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

    #[test]
    fn test_selectable_text_git_diff_style() {
        let hunks = vec![DiffHunk {
            header: "@@ -1,3 +1,3 @@ fn main()\n".to_string(),
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 3,
            lines: vec![
                DiffLine {
                    line_type: DiffLineType::Context,
                    old_line_num: Some(1),
                    new_line_num: Some(1),
                    content: "let x = 1;".to_string(),
                    comment: None,
                },
                DiffLine {
                    line_type: DiffLineType::Remove,
                    old_line_num: Some(2),
                    new_line_num: None,
                    content: "foo();".to_string(),
                    comment: None,
                },
                DiffLine {
                    line_type: DiffLineType::Add,
                    old_line_num: None,
                    new_line_num: Some(2),
                    content: "bar();".to_string(),
                    comment: None,
                },
            ],
        }];

        // Hunk header verbatim, then one line per code line with its diff sign.
        assert_eq!(
            selectable_text_for_hunks(&hunks),
            "@@ -1,3 +1,3 @@ fn main()\n let x = 1;\n-foo();\n+bar();"
        );
    }

    #[test]
    fn selectable_text_of_nothing_is_empty() {
        assert_eq!(selectable_text_for_hunks(&[]), "");
    }
}
