//! Change segments: the unit a reviewer marks as viewed inside one file.
//!
//! A change segment starts as one maximal run of removed and added lines — the
//! same run `flow_scene::build_scene` turns into a `FlowSegment` with
//! `SegKind::Change`. [`split_segments`] then cuts a run at the boundaries of
//! the definitions inside it, so each function or class gets its own toggle; a
//! run in a language without a grammar stays whole. Defining it here once, on
//! the same `RowClass` vocabulary, is what keeps the unified, side-by-side and
//! flowing views agreeing on where a segment starts and ends, rather than each
//! deriving its own.
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
use crate::outline::{FileOutlines, Outline};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

/// One hunk line: its class, its text, and its line number on its own side
/// (the old side for a removed line, the new side for an added one).
pub type Row<'a> = (RowClass, &'a str, Option<u32>);

/// One maximal run of removed and added rows, or one definition-sized piece of
/// it, and the key it is stored under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSegment {
    /// Content key. Stable across line-number changes; changes with the text.
    pub hash: u64,
    /// Indices of the segment's rows in the file's flattened hunk lines, in
    /// stream order.
    pub rows: Vec<usize>,
    pub additions: usize,
    pub deletions: usize,
    /// For a piece of a segment split at definition boundaries, the key of the
    /// whole segment. A mark stored under that key marks every piece.
    pub parent: Option<u64>,
}

/// The segments a reviewer can mark in one file.
///
/// Normally the file's change segments, split at definition boundaries. A file that changed without changing a
/// line — a rename, a mode change, a binary blob — produces no rows and so no
/// segments, and would have nothing to tick; it gets one synthetic segment
/// keyed on `file_hash` instead, which still expires when the file changes.
pub fn segments_for_file(rows: &[Row], outlines: &FileOutlines, file_hash: u64) -> Vec<ChangeSegment> {
    let segments = split_segments(rows, outlines.old.as_deref(), outlines.new.as_deref());
    if !segments.is_empty() {
        return segments;
    }

    let mut hasher = DefaultHasher::new();
    "synthetic-file-segment".hash(&mut hasher);
    file_hash.hash(&mut hasher);
    vec![ChangeSegment {
        hash: hasher.finish(),
        rows: Vec::new(),
        additions: 0,
        deletions: 0,
        parent: None,
    }]
}

/// Split a file's hunk lines into its change segments.
///
/// `rows` pairs each line's class with its text. Only `Add` and `Remove` lines
/// belong to a segment; every other class ends the run in progress, which is
/// exactly `build_scene`'s rule.
fn change_segments(rows: &[Row]) -> Vec<ChangeSegment> {
    let mut out: Vec<ChangeSegment> = Vec::new();
    let mut start: Option<usize> = None;

    for (i, (class, _, _)) in rows.iter().enumerate() {
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

/// Split a file's hunk lines into change segments, and split each of those at
/// the boundaries of the definitions inside it.
///
/// `old` and `new` are the outlines of the two blobs, `None` for a language
/// without a grammar.
pub fn split_segments(rows: &[Row], old: Option<&Outline>, new: Option<&Outline>) -> Vec<ChangeSegment> {
    let mut out = Vec::new();
    for seg in change_segments(rows) {
        match split_one(rows, &seg, old, new) {
            Some(parts) => out.extend(parts.into_iter().map(|p| ChangeSegment {
                parent: Some(seg.hash),
                ..segment_of_rows(rows, p)
            })),
            None => out.push(seg),
        }
    }
    disambiguate_duplicates(&mut out);
    out
}

/// One side of a segment: the stream index and line number of each row.
type Lines = Vec<(usize, u32)>;

/// A piece of one side: the name of its definition, and its rows.
type Piece<'a> = (Option<&'a str>, Vec<usize>);

/// The rows of each part `seg` splits into, or `None` to keep it whole.
///
/// A one-sided segment splits into its pieces. A two-sided one pairs a removed
/// and an added piece by definition name; a piece without a partner stands
/// alone.
fn split_one(
    rows: &[Row],
    seg: &ChangeSegment,
    old: Option<&Outline>,
    new: Option<&Outline>,
) -> Option<Vec<Vec<usize>>> {
    let removed = side_pieces(rows, seg, RowClass::Remove, old)?;
    let added = side_pieces(rows, seg, RowClass::Add, new)?;
    let mut parts = combine(removed, added)?;
    // Each part is shown as one block, in the order of its first row.
    parts.sort_by_key(|p| p[0]);
    (parts.len() > 1 && numbers_only_go_up(rows, &parts)).then_some(parts)
}

/// The parts of a segment from the pieces of its two sides: the pieces of the
/// one side that has any, else the pairs of both. `None` when neither has any.
fn combine(removed: Vec<Piece>, added: Vec<Piece>) -> Option<Vec<Vec<usize>>> {
    match (removed.is_empty(), added.is_empty()) {
        (false, true) => Some(rows_of(removed)),
        (true, false) => Some(rows_of(added)),
        (false, false) => pair(removed, added),
        (true, true) => None,
    }
}

/// The pieces of one side of `seg`: none when the side is empty, `None` when
/// it has lines but no outline, or an outline that can't be trusted.
fn side_pieces<'a>(
    rows: &[Row],
    seg: &ChangeSegment,
    class: RowClass,
    outline: Option<&'a Outline>,
) -> Option<Vec<Piece<'a>>> {
    let lines = side_lines(rows, seg, class);
    if lines.is_empty() {
        return Some(Vec::new());
    }
    named_pieces(&lines, outline?)
}

/// Whether showing `parts` one after another keeps the old and the new line
/// numbers rising. Pairing swapped definitions would break that.
fn numbers_only_go_up(rows: &[Row], parts: &[Vec<usize>]) -> bool {
    [RowClass::Remove, RowClass::Add].iter().all(|&class| {
        let lines: Vec<u32> = parts
            .iter()
            .flatten()
            .filter(|&&i| rows[i].0 == class)
            .filter_map(|&i| rows[i].2)
            .collect();
        lines.windows(2).all(|w| w[0] < w[1])
    })
}

fn rows_of(pieces: Vec<Piece>) -> Vec<Vec<usize>> {
    pieces.into_iter().map(|(_, rows)| rows).collect()
}

/// Pair removed and added pieces that carry the same name. `None` when a name
/// repeats on one side, since the pairing is then a guess.
fn pair(removed: Vec<Piece>, added: Vec<Piece>) -> Option<Vec<Vec<usize>>> {
    let mut partners: HashMap<Option<&str>, Vec<usize>> = HashMap::new();
    for (name, rows) in added {
        if partners.insert(name, rows).is_some() {
            return None;
        }
    }
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    for (name, mut part) in removed {
        if !seen.insert(name) {
            return None;
        }
        part.extend(partners.remove(&name).unwrap_or_default());
        parts.push(part);
    }
    // Order does not matter: the caller sorts the parts by their first row.
    parts.extend(partners.into_values());
    Some(parts)
}

/// The segment's rows of one class, with their line numbers.
fn side_lines(rows: &[Row], seg: &ChangeSegment, class: RowClass) -> Lines {
    seg.rows
        .iter()
        .filter(|&&i| rows[i].0 == class)
        .filter_map(|&i| rows[i].2.map(|line| (i, line)))
        .collect()
}

/// Cut one side of a segment after every definition that ends inside it and is
/// followed by another definition inside it. Lines between two definitions go
/// with the one after. Each piece is named after the definition that starts in
/// it, else one that overlaps it.
///
/// `None` when an ERROR node overlaps the side: the boundaries there are
/// guesses.
fn named_pieces<'a>(lines: &[(usize, u32)], outline: &'a Outline) -> Option<Vec<Piece<'a>>> {
    let (first, last) = (lines.first()?.1, lines.last()?.1);
    if outline.errors.iter().any(|&(start, end)| start <= last && end >= first) {
        return None;
    }
    let defs = &outline.definitions;
    let cuts: Vec<u32> = defs
        .iter()
        .map(|d| d.end)
        .filter(|&end| end >= first && end < last)
        .filter(|&end| defs.iter().any(|d| d.start > end && d.start <= last))
        .collect();

    let mut pieces: Vec<Lines> = vec![Vec::new()];
    let mut prev: Option<u32> = None;
    for &(i, line) in lines {
        if prev.is_some_and(|p| cuts.iter().any(|&c| p <= c && c < line)) {
            pieces.push(Vec::new());
        }
        pieces.last_mut().expect("never empty").push((i, line));
        prev = Some(line);
    }

    let named = pieces
        .into_iter()
        .map(|piece| {
            let (lo, hi) = (piece[0].1, piece[piece.len() - 1].1);
            let name = defs
                .iter()
                .find(|d| d.start >= lo && d.start <= hi)
                .or_else(|| defs.iter().find(|d| d.start <= hi && d.end >= lo))
                .map(|d| d.name.as_str());
            (name, piece.into_iter().map(|(i, _)| i).collect())
        })
        .collect();
    Some(named)
}

/// The order to show a file's hunk lines in: each segment's rows as one
/// block, at the place of its first row. Rows outside every segment keep
/// their place. Without a split this is the identity.
pub fn display_order(len: usize, segments: &[ChangeSegment]) -> Vec<usize> {
    let mut owner: Vec<Option<&ChangeSegment>> = vec![None; len];
    for seg in segments {
        for &i in &seg.rows {
            owner[i] = Some(seg);
        }
    }
    let mut order = Vec::with_capacity(len);
    for (i, seg) in owner.iter().enumerate() {
        match seg {
            None => order.push(i),
            Some(seg) if seg.rows[0] == i => order.extend(&seg.rows),
            Some(_) => {}
        }
    }
    order
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

fn segment(rows: &[Row], start: usize, end: usize) -> ChangeSegment {
    segment_of_rows(rows, (start..end).collect())
}

/// A segment made of the given rows, keyed on their text in stream order.
fn segment_of_rows(rows: &[Row], indices: Vec<usize>) -> ChangeSegment {
    let class_of = |i: &usize| rows[*i].0;
    let deletions = indices.iter().filter(|i| class_of(i) == RowClass::Remove).count();
    let additions = indices.iter().filter(|i| class_of(i) == RowClass::Add).count();

    // The side tag goes into the hash so "-a +b" and "-b +a" are different
    // segments; the text alone would make them the same.
    let mut hasher = DefaultHasher::new();
    for &i in &indices {
        let (class, text, _) = rows[i];
        matches!(class, RowClass::Add).hash(&mut hasher);
        text.hash(&mut hasher);
    }

    ChangeSegment { hash: hasher.finish(), rows: indices, additions, deletions, parent: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::Definition;

    fn rm(text: &str) -> Row<'_> {
        (RowClass::Remove, text, None)
    }
    fn add(text: &str) -> Row<'_> {
        (RowClass::Add, text, None)
    }
    fn ctx(text: &str) -> Row<'_> {
        (RowClass::Context, text, None)
    }

    #[test]
    fn a_run_of_removes_and_adds_is_one_segment() {
        let segments =
            change_segments(&[ctx("a"), rm("b"), rm("c"), add("d"), add("e"), ctx("f")]);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].rows, vec![1, 2, 3, 4]);
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

        assert_ne!(before[0].rows, after[0].rows);
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
        let segments = segments_for_file(&[], &FileOutlines::default(), 0xabc);

        assert_eq!(segments.len(), 1);
        assert!(segments[0].rows.is_empty());
        assert_eq!(segments[0].additions, 0);
        assert_eq!(segments[0].deletions, 0);
    }

    #[test]
    fn the_synthetic_segment_follows_the_file_content_hash() {
        let before = segments_for_file(&[], &FileOutlines::default(), 0xabc);
        let after = segments_for_file(&[], &FileOutlines::default(), 0xdef);

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
        let rows: Vec<Row> = classes.iter().map(|&c| (c, "x", None)).collect();
        let scene_rows: Vec<SceneRow> =
            classes.iter().map(|&class| SceneRow { class, height: 1.0 }).collect();

        let change_runs = build_scene(&scene_rows)
            .segments
            .iter()
            .filter(|s| s.kind == crate::flow_map::SegKind::Change)
            .count();

        assert_eq!(change_segments(&rows).len(), change_runs);
    }

    fn add_at(line: u32, text: &str) -> Row<'_> {
        (RowClass::Add, text, Some(line))
    }

    fn def(name: &str, start: u32, end: u32) -> Definition {
        Definition { name: name.to_string(), start, end }
    }

    fn outline(defs: Vec<Definition>) -> Outline {
        Outline { definitions: defs, errors: Vec::new() }
    }

    #[test]
    fn two_functions_added_back_to_back_split_into_two_pieces() {
        let rows = two_added_functions();
        let new = outline(vec![def("fn:a", 2, 3), def("fn:b", 5, 6)]);

        let segments = split_segments(&rows, None, Some(&new));

        // The blank line between them goes with the definition after it.
        let pieces: Vec<Vec<usize>> = segments.iter().map(|s| s.rows.clone()).collect();
        assert_eq!(pieces, vec![vec![1, 2], vec![3, 4, 5]]);
    }

    /// Two functions added back to back at lines 2..=6, as rows.
    fn two_added_functions() -> Vec<Row<'static>> {
        vec![
            ctx("use x;"),
            add_at(2, "fn a() {"),
            add_at(3, "}"),
            add_at(4, ""),
            add_at(5, "fn b() {"),
            add_at(6, "}"),
            ctx("// end"),
        ]
    }

    #[test]
    fn one_function_is_not_split() {
        let rows = [ctx("use x;"), add_at(2, "fn a() {"), add_at(3, "}"), ctx("// end")];
        let new = outline(vec![def("fn:a", 2, 3)]);

        let split = split_segments(&rows, None, Some(&new));

        assert_eq!(split, change_segments(&rows));
    }

    #[test]
    fn an_error_node_inside_the_segment_keeps_it_whole() {
        let rows = two_added_functions();
        let mut new = outline(vec![def("fn:a", 2, 3), def("fn:b", 5, 6)]);
        new.errors.push((6, 6));

        let split = split_segments(&rows, None, Some(&new));

        assert_eq!(split, change_segments(&rows));
    }

    #[test]
    fn a_language_without_a_grammar_keeps_the_segment_whole() {
        let rows = two_added_functions();

        assert_eq!(split_segments(&rows, None, None), change_segments(&rows));
    }

    #[test]
    fn every_piece_records_the_key_of_the_whole_segment() {
        let rows = two_added_functions();
        let new = outline(vec![def("fn:a", 2, 3), def("fn:b", 5, 6)]);
        let whole = change_segments(&rows)[0].hash;

        let split = split_segments(&rows, None, Some(&new));

        assert_eq!(split.len(), 2);
        assert!(split.iter().all(|s| s.parent == Some(whole)));
        assert_eq!(change_segments(&rows)[0].parent, None);
    }

    fn rm_at(line: u32, text: &str) -> Row<'_> {
        (RowClass::Remove, text, Some(line))
    }

    #[test]
    fn two_functions_removed_back_to_back_split_into_two_pieces() {
        let rows = [
            ctx("use x;"),
            rm_at(2, "fn a() {"),
            rm_at(3, "}"),
            rm_at(4, ""),
            rm_at(5, "fn b() {"),
            rm_at(6, "}"),
            ctx("// end"),
        ];
        let old = outline(vec![def("fn:a", 2, 3), def("fn:b", 5, 6)]);

        let segments = split_segments(&rows, Some(&old), None);

        let pieces: Vec<Vec<usize>> = segments.iter().map(|s| s.rows.clone()).collect();
        assert_eq!(pieces, vec![vec![1, 2], vec![3, 4, 5]]);
    }

    #[test]
    fn rows_without_line_numbers_keep_the_segment_whole() {
        let rows = [
            ctx("use x;"),
            rm("fn a() {"),
            add("fn b() {"),
            ctx("// end"),
        ];
        let both = outline(vec![def("fn:a", 2, 3), def("fn:b", 5, 6)]);

        let split = split_segments(&rows, Some(&both), Some(&both));

        assert_eq!(split, change_segments(&rows));
    }

    /// Rows of one segment that rewrites `fn a` (lines 2..=3) and `fn b` (lines
    /// 4..=5) on both sides, as git emits it: every removed line, then every
    /// added line.
    fn two_rewritten_functions() -> Vec<Row<'static>> {
        vec![
            ctx("use x;"),
            rm_at(2, "fn a() {"),
            rm_at(3, "}"),
            rm_at(4, "fn b() {"),
            rm_at(5, "}"),
            add_at(2, "fn a() { 1 }"),
            add_at(3, ""),
            add_at(4, "fn b() { 2 }"),
            add_at(5, ""),
            ctx("// end"),
        ]
    }

    fn pieces_of(segments: &[ChangeSegment]) -> Vec<Vec<usize>> {
        segments.iter().map(|s| s.rows.clone()).collect()
    }

    #[test]
    fn a_rewrite_splits_into_one_pair_per_definition_name() {
        let rows = two_rewritten_functions();
        let old = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);
        let new = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);

        let split = split_segments(&rows, Some(&old), Some(&new));

        assert_eq!(pieces_of(&split), vec![vec![1, 2, 5, 6], vec![3, 4, 7, 8]]);
    }

    #[test]
    fn duplicate_names_keep_the_segment_whole() {
        let rows = two_rewritten_functions();
        let old = outline(vec![def("fn:a", 2, 3), def("fn:a", 4, 5)]);
        let new = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);

        let split = split_segments(&rows, Some(&old), Some(&new));

        assert_eq!(split, change_segments(&rows));
    }

    #[test]
    fn swapped_definitions_keep_the_segment_whole() {
        // `a b` became `b a`. Pairing them would show the new side's line
        // numbers going backwards, so the segment stays whole.
        let rows = two_rewritten_functions();
        let old = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);
        let new = outline(vec![def("fn:b", 2, 3), def("fn:a", 4, 5)]);

        let split = split_segments(&rows, Some(&old), Some(&new));

        assert_eq!(split, change_segments(&rows));
    }

    #[test]
    fn a_piece_without_a_partner_stands_alone() {
        // `b` was deleted and `c` added in its place.
        let rows = two_rewritten_functions();
        let old = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);
        let new = outline(vec![def("fn:a", 2, 3), def("fn:c", 4, 5)]);

        let split = split_segments(&rows, Some(&old), Some(&new));

        assert_eq!(pieces_of(&split), vec![vec![1, 2, 5, 6], vec![3, 4], vec![7, 8]]);
    }

    #[test]
    fn each_pair_is_shown_as_one_block() {
        let rows = two_rewritten_functions();
        let old = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);
        let new = outline(vec![def("fn:a", 2, 3), def("fn:b", 4, 5)]);
        let split = split_segments(&rows, Some(&old), Some(&new));

        let order = display_order(rows.len(), &split);

        assert_eq!(order, vec![0, 1, 2, 5, 6, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn a_segment_that_starts_inside_a_definition_splits_off_its_tail() {
        // `fn a` spans lines 1..=4 and the segment starts at line 3, so the
        // first piece is named after the definition it overlaps.
        let rows = [
            ctx("fn a() {"),
            add_at(3, "    x();"),
            add_at(4, "}"),
            add_at(5, ""),
            add_at(6, "fn b() {"),
            add_at(7, "}"),
        ];
        let new = outline(vec![def("fn:a", 1, 4), def("fn:b", 6, 7)]);

        let split = split_segments(&rows, None, Some(&new));

        assert_eq!(pieces_of(&split), vec![vec![1, 2], vec![3, 4, 5]]);
    }

    #[test]
    fn three_functions_split_into_three_pieces() {
        let rows = [
            add_at(1, "fn a() {}"),
            add_at(2, "fn b() {}"),
            add_at(3, "fn c() {"),
            add_at(4, "}"),
        ];
        let new = outline(vec![def("fn:a", 1, 1), def("fn:b", 2, 2), def("fn:c", 3, 4)]);

        let split = split_segments(&rows, None, Some(&new));

        assert_eq!(pieces_of(&split), vec![vec![0], vec![1], vec![2, 3]]);
    }
}
