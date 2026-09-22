//! Change segments: the unit a reviewer marks as viewed inside one file.
//!
//! A change segment is one maximal run of removed and added lines — the same
//! run `flow_scene::build_scene` turns into a `FlowSegment` with
//! `SegKind::Change`. Defining it here once, on the same `RowClass` vocabulary,
//! is what keeps the unified, side-by-side and flowing views agreeing on where a
//! segment starts and ends, rather than each deriving its own.
//!
//! The input is the file's *hunk* lines, not its rendered rows. Line wrapping
//! and the font size split one line into several rows and would otherwise
//! change a segment's key, expiring a reviewer's marks on a settings change
//! that altered nothing about the code. It also means the segments of a file
//! can be worked out without highlighting or laying it out, which is what makes
//! the file tree's checkboxes affordable.
//!
//! A segment is keyed by a hash of its removed and added *text*, never by index
//! or line number, so a segment that only moved keeps its identity and a segment
//! whose text changed loses it.
//!
//! Kept free of Slint and git types so it can be unit-tested as pure logic.

use crate::flow_scene::RowClass;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// One maximal run of removed and added rows, and the key it is stored under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSegment {
    /// Content key. Stable across line-number changes; changes with the text.
    pub hash: u64,
    /// Half-open range `[start, end)` in the file's flattened hunk lines.
    pub start: usize,
    pub end: usize,
    pub additions: usize,
    pub deletions: usize,
}

/// The segments a reviewer can mark in one file.
///
/// Normally the file's change segments. A file that changed without changing a
/// line — a rename, a mode change, a binary blob — produces no rows and so no
/// segments, and would have nothing to tick; it gets one synthetic segment
/// keyed on `file_hash` instead, which still expires when the file changes.
pub fn segments_for_file(rows: &[(RowClass, &str)], file_hash: u64) -> Vec<ChangeSegment> {
    let segments = change_segments(rows);
    if !segments.is_empty() {
        return segments;
    }

    let mut hasher = DefaultHasher::new();
    "synthetic-file-segment".hash(&mut hasher);
    file_hash.hash(&mut hasher);
    vec![ChangeSegment {
        hash: hasher.finish(),
        start: 0,
        end: 0,
        additions: 0,
        deletions: 0,
    }]
}

/// Split a file's hunk lines into its change segments.
///
/// `rows` pairs each line's class with its text. Only `Add` and `Remove` lines
/// belong to a segment; every other class ends the run in progress, which is
/// exactly `build_scene`'s rule.
pub fn change_segments(rows: &[(RowClass, &str)]) -> Vec<ChangeSegment> {
    let mut out: Vec<ChangeSegment> = Vec::new();
    let mut start: Option<usize> = None;

    for (i, (class, _)) in rows.iter().enumerate() {
        match class {
            RowClass::Add | RowClass::Remove => {
                start.get_or_insert(i);
            }
            _ => {
                if let Some(s) = start.take() {
                    out.push(segment(rows, s, i));
                }
            }
        }
    }
    if let Some(s) = start {
        out.push(segment(rows, s, rows.len()));
    }
    disambiguate_duplicates(&mut out);
    out
}

/// Give every repeat of an identical edit its own key.
///
/// The same edit applied twice in one file hashes the same both times, which
/// would make the two toggle as one. Folding each segment's occurrence count
/// into its hash separates them while keeping the key content-derived: inserting
/// unrelated code between them does not renumber anything.
///
/// The residual cost is narrow and accepted: among a set of *identical*
/// segments, editing an earlier one renumbers the later ones and so drops their
/// marks. Keying on position instead would drop marks on every edit anywhere in
/// the file, which is far worse.
fn disambiguate_duplicates(segments: &mut [ChangeSegment]) {
    let mut seen: HashMap<u64, u32> = HashMap::new();
    for seg in segments.iter_mut() {
        let occurrence = seen.entry(seg.hash).or_insert(0);
        if *occurrence > 0 {
            let mut hasher = DefaultHasher::new();
            seg.hash.hash(&mut hasher);
            occurrence.hash(&mut hasher);
            seg.hash = hasher.finish();
        }
        *occurrence += 1;
    }
}

fn segment(rows: &[(RowClass, &str)], start: usize, end: usize) -> ChangeSegment {
    let run = &rows[start..end];
    let deletions = run.iter().filter(|(c, _)| *c == RowClass::Remove).count();
    let additions = run.iter().filter(|(c, _)| *c == RowClass::Add).count();

    // The side tag goes into the hash so "-a +b" and "-b +a" are different
    // segments; the text alone would make them the same.
    let mut hasher = DefaultHasher::new();
    for (class, text) in run {
        matches!(class, RowClass::Add).hash(&mut hasher);
        text.hash(&mut hasher);
    }

    ChangeSegment { hash: hasher.finish(), start, end, additions, deletions }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rm(text: &str) -> (RowClass, &str) {
        (RowClass::Remove, text)
    }
    fn add(text: &str) -> (RowClass, &str) {
        (RowClass::Add, text)
    }
    fn ctx(text: &str) -> (RowClass, &str) {
        (RowClass::Context, text)
    }

    #[test]
    fn a_run_of_removes_and_adds_is_one_segment() {
        let segments =
            change_segments(&[ctx("a"), rm("b"), rm("c"), add("d"), add("e"), ctx("f")]);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, 1);
        assert_eq!(segments[0].end, 5);
        assert_eq!(segments[0].deletions, 2);
        assert_eq!(segments[0].additions, 2);
    }

    #[test]
    fn one_changed_character_changes_the_hash() {
        let before = change_segments(&[ctx("a"), rm("old"), add("new"), ctx("b")]);
        let after = change_segments(&[ctx("a"), rm("old"), add("newX"), ctx("b")]);

        assert_ne!(before[0].hash, after[0].hash);
    }

    #[test]
    fn the_same_text_at_a_new_line_number_keeps_the_hash() {
        // Two extra context rows above shift the segment down the file. Its own
        // text is untouched, so the reviewer's mark must survive.
        let before = change_segments(&[ctx("a"), rm("old"), add("new"), ctx("b")]);
        let after = change_segments(&[
            ctx("x"),
            ctx("y"),
            ctx("a"),
            rm("old"),
            add("new"),
            ctx("b"),
        ]);

        assert_ne!(before[0].start, after[0].start);
        assert_eq!(before[0].hash, after[0].hash);
    }

    #[test]
    fn two_identical_segments_in_one_file_get_different_hashes() {
        // Same edit applied twice in one file. Marking the first one viewed must
        // not silently collapse the second one too.
        let segments = change_segments(&[
            ctx("a"),
            rm("old"),
            add("new"),
            ctx("b"),
            rm("old"),
            add("new"),
            ctx("c"),
        ]);

        assert_eq!(segments.len(), 2);
        assert_ne!(segments[0].hash, segments[1].hash);
    }

    #[test]
    fn a_file_with_no_change_rows_still_gets_one_segment_to_mark() {
        // A rename, a mode change or a binary file produces no hunks at all.
        // Without a segment there would be nothing for the reviewer to tick.
        let segments = segments_for_file(&[], 0xabc);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, segments[0].end);
        assert_eq!(segments[0].additions, 0);
        assert_eq!(segments[0].deletions, 0);
    }

    #[test]
    fn the_synthetic_segment_follows_the_file_content_hash() {
        let before = segments_for_file(&[], 0xabc);
        let after = segments_for_file(&[], 0xdef);

        assert_ne!(before[0].hash, after[0].hash);
    }

    #[test]
    fn segments_match_the_change_runs_the_flowing_view_couples() {
        // D1: one definition of a segment, shared by all three views. The
        // flowing view's ribbons are drawn per SegKind::Change run, so a
        // reviewer's toggle and the ribbon it collapses must be the same runs.
        // Hunk lines carry no comment rows — comments are injected into the
        // rendered rows later — so the classes here are the ones that reach a
        // real split.
        use crate::flow_scene::{build_scene, SceneRow};

        let classes = [
            RowClass::Context,
            RowClass::Remove,
            RowClass::Add,
            RowClass::Context,
            RowClass::Add,
            RowClass::Hunk,
            RowClass::Remove,
        ];
        let rows: Vec<(RowClass, &str)> = classes.iter().map(|&c| (c, "x")).collect();
        let scene_rows: Vec<SceneRow> =
            classes.iter().map(|&class| SceneRow { class, height: 1.0 }).collect();

        let change_runs = build_scene(&scene_rows)
            .segments
            .iter()
            .filter(|s| s.kind == crate::flow_map::SegKind::Change)
            .count();

        assert_eq!(change_segments(&rows).len(), change_runs);
    }
}
