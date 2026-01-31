use crate::git::{DiffLine, DiffLineType};
use crate::DiffLine as SlintDiffLine;

/// Model for a diff line in the UI
pub struct DiffLineModel {
    pub line_type: String,
    pub old_line_num: String,
    pub new_line_num: String,
    pub content: String,
}

impl From<&DiffLine> for DiffLineModel {
    fn from(line: &DiffLine) -> Self {
        let line_type = match line.line_type {
            DiffLineType::Add => "add",
            DiffLineType::Remove => "remove",
            DiffLineType::Context => "context",
            DiffLineType::Hunk => "hunk",
        };

        Self {
            line_type: line_type.to_string(),
            old_line_num: line
                .old_line_num
                .map(|n| n.to_string())
                .unwrap_or_default(),
            new_line_num: line
                .new_line_num
                .map(|n| n.to_string())
                .unwrap_or_default(),
            content: line.content.clone(),
        }
    }
}

impl From<DiffLineModel> for SlintDiffLine {
    fn from(model: DiffLineModel) -> Self {
        Self {
            line_type: model.line_type.into(),
            old_line_num: model.old_line_num.into(),
            new_line_num: model.new_line_num.into(),
            content: model.content.into(),
        }
    }
}
