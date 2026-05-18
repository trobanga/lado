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
    /// True for visual rows produced by wrapping a long line — they keep the
    /// same line_type for background coloring but suppress the sign and gutter
    /// numbers so the user can tell continuations from real code lines.
    pub is_continuation: bool,
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
            is_continuation: false,
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
            is_continuation: model.is_continuation,
            comment_author: model.comment_author.into(),
            comment_body: model.comment_body.into(),
            comment_timestamp: model.comment_timestamp.into(),
            comment_is_reply: model.comment_is_reply,
        }
    }
}

/// Split a diff line into multiple visual rows that each fit within
/// `wrap_column` characters. Only Add/Remove/Context lines wrap; Hunk headers
/// and Comments pass through unchanged. Continuation rows preserve the
/// background (line_type) but drop the line numbers and sign.
pub fn wrap_diff_line(model: DiffLineModel, wrap_column: usize) -> Vec<DiffLineModel> {
    if wrap_column == 0 {
        return vec![model];
    }
    if !matches!(model.line_type.as_str(), "add" | "remove" | "context") {
        return vec![model];
    }
    if model.content.chars().count() <= wrap_column {
        return vec![model];
    }

    let chunks: Vec<(String, Vec<TextSpanModel>)> = if model.spans.is_empty() {
        chunk_str(&model.content, wrap_column)
            .into_iter()
            .map(|s| (s, Vec::new()))
            .collect()
    } else {
        chunk_spans(&model.spans, wrap_column)
    };

    chunks
        .into_iter()
        .enumerate()
        .map(|(i, (content, spans))| DiffLineModel {
            line_type: model.line_type.clone(),
            old_line_num: if i == 0 { model.old_line_num.clone() } else { String::new() },
            new_line_num: if i == 0 { model.new_line_num.clone() } else { String::new() },
            content,
            spans,
            is_continuation: i > 0,
            comment_author: String::new(),
            comment_body: String::new(),
            comment_timestamp: String::new(),
            comment_is_reply: false,
        })
        .collect()
}

fn chunk_str(s: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for ch in s.chars() {
        current.push(ch);
        count += 1;
        if count == n {
            out.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn chunk_spans(spans: &[TextSpanModel], n: usize) -> Vec<(String, Vec<TextSpanModel>)> {
    let mut out: Vec<(String, Vec<TextSpanModel>)> = Vec::new();
    let mut cur_text = String::new();
    let mut cur_spans: Vec<TextSpanModel> = Vec::new();
    let mut cur_count = 0usize;

    for span in spans {
        let mut remaining = span.text.as_str();
        while !remaining.is_empty() {
            let want = n - cur_count;
            let take = remaining.chars().count().min(want);
            let take_bytes = remaining
                .char_indices()
                .nth(take)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            let (head, tail) = remaining.split_at(take_bytes);
            cur_text.push_str(head);
            cur_spans.push(TextSpanModel::new(head.to_string(), span.color));
            cur_count += take;
            remaining = tail;
            if cur_count >= n {
                out.push((std::mem::take(&mut cur_text), std::mem::take(&mut cur_spans)));
                cur_count = 0;
            }
        }
    }
    if !cur_spans.is_empty() {
        out.push((cur_text, cur_spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Color;

    fn model_with(content: &str, spans: Vec<TextSpanModel>, line_type: &str) -> DiffLineModel {
        DiffLineModel {
            line_type: line_type.to_string(),
            old_line_num: "10".to_string(),
            new_line_num: "12".to_string(),
            content: content.to_string(),
            spans,
            is_continuation: false,
            comment_author: String::new(),
            comment_body: String::new(),
            comment_timestamp: String::new(),
            comment_is_reply: false,
        }
    }

    #[test]
    fn wrap_disabled_returns_single() {
        let m = model_with(&"a".repeat(200), vec![], "add");
        let out = wrap_diff_line(m, 0);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn short_line_returns_single() {
        let m = model_with("short", vec![], "add");
        let out = wrap_diff_line(m, 100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, "short");
        assert!(!out[0].is_continuation);
    }

    #[test]
    fn hunk_header_does_not_wrap() {
        let m = model_with(&"@".repeat(150), vec![], "hunk");
        let out = wrap_diff_line(m, 50);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn plain_content_splits_at_column() {
        let m = model_with(&"x".repeat(250), vec![], "context");
        let out = wrap_diff_line(m, 100);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].content.chars().count(), 100);
        assert_eq!(out[1].content.chars().count(), 100);
        assert_eq!(out[2].content.chars().count(), 50);
        assert_eq!(out[0].old_line_num, "10");
        assert_eq!(out[0].new_line_num, "12");
        assert_eq!(out[1].old_line_num, "");
        assert_eq!(out[1].new_line_num, "");
        assert!(!out[0].is_continuation);
        assert!(out[1].is_continuation);
        assert!(out[2].is_continuation);
    }

    #[test]
    fn spans_split_across_boundary() {
        let red = Color::from_rgb_u8(255, 0, 0);
        let blue = Color::from_rgb_u8(0, 0, 255);
        let m = model_with(
            "aaaaabbbbbccccc",
            vec![
                TextSpanModel::new("aaaaa".to_string(), red),
                TextSpanModel::new("bbbbbccccc".to_string(), blue),
            ],
            "add",
        );
        let out = wrap_diff_line(m, 7);
        assert_eq!(out.len(), 3);
        // First chunk: "aaaaabb" → red(5) + blue(2)
        assert_eq!(out[0].content, "aaaaabb");
        assert_eq!(out[0].spans.len(), 2);
        assert_eq!(out[0].spans[0].text, "aaaaa");
        assert_eq!(out[0].spans[1].text, "bb");
        // Second: "bbbcccc" → blue(7)
        assert_eq!(out[1].content, "bbbcccc");
        assert_eq!(out[1].spans.len(), 1);
        assert_eq!(out[1].spans[0].text, "bbbcccc");
        // Third: "c"
        assert_eq!(out[2].content, "c");
        // continuation flags
        assert!(!out[0].is_continuation);
        assert!(out[1].is_continuation);
        assert!(out[2].is_continuation);
    }
}
