use crate::git::{DiffLine, DiffLineType};
use crate::models::TextSpanModel;
use crate::DiffLine as SlintDiffLine;
use crate::TextSpan as SlintTextSpan;
use slint::ModelRc;

/// Model for a diff line in the UI
pub struct DiffLineModel {
    pub line_type: String,
    pub old_line_num: String,
    pub new_line_num: String,
    pub content: String,
    pub spans: Vec<TextSpanModel>,
    // Comment fields
    pub comment_author: String,
    pub comment_body: String,
    pub comment_timestamp: String,
    pub comment_is_reply: bool,
}

impl From<&DiffLine> for DiffLineModel {
    fn from(line: &DiffLine) -> Self {
        let line_type = match line.line_type {
            DiffLineType::Add => "add",
            DiffLineType::Remove => "remove",
            DiffLineType::Context => "context",
            DiffLineType::Hunk => "hunk",
            DiffLineType::Comment => "comment",
        };

        let (author, body, timestamp, is_reply) = match &line.comment {
            Some(c) => (
                c.author.clone(),
                c.body.clone(),
                c.timestamp.clone(),
                c.is_reply,
            ),
            None => (String::new(), String::new(), String::new(), false),
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
            spans: Vec::new(), // Spans populated later by highlighter
            comment_author: author,
            comment_body: body,
            comment_timestamp: timestamp,
            comment_is_reply: is_reply,
        }
    }
}

impl From<DiffLineModel> for SlintDiffLine {
    fn from(model: DiffLineModel) -> Self {
        // Convert spans to Slint model
        let slint_spans: Vec<SlintTextSpan> = model
            .spans
            .into_iter()
            .map(SlintTextSpan::from)
            .collect();
        let spans_model = ModelRc::new(slint::VecModel::from(slint_spans));

        Self {
            line_type: model.line_type.into(),
            old_line_num: model.old_line_num.into(),
            new_line_num: model.new_line_num.into(),
            content: model.content.into(),
            spans: spans_model,
            comment_author: model.comment_author.into(),
            comment_body: model.comment_body.into(),
            comment_timestamp: model.comment_timestamp.into(),
            comment_is_reply: model.comment_is_reply,
        }
    }
}
