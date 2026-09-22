//! Adapter from the merged diff-row list to the flowing view's per-pane shape.
//!
//! Both existing diff views iterate a single merged `[DiffLine]` where filler is
//! implicit (an "add" row draws blank on the left). The flowing view instead
//! needs each pane's *own* row sequence plus the segment list that couples their
//! scroll offsets. This module turns the merged, already-classified rows into
//! that shape: index lists into the merged row list for each pane, and the
//! `FlowSegment`s fed straight into `FlowLayout::build`.
//!
//! Kept free of Slint and git types so it can be unit-tested as pure logic; the
//! caller classifies each merged row (`RowClass`) and maps the returned indices
//! back to concrete UI rows.

use crate::flow_map::{FlowSegment, SegKind};

/// Which pane an anchored comment belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// How a merged visual row maps onto the two panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClass {
    /// Context line: identical on both sides, appears on both panes.
    Context,
    /// Hunk header: shared, treated like context for flow purposes.
    Hunk,
    /// Added line: right pane only.
    Add,
    /// Removed line: left pane only.
    Remove,
    /// Review comment card anchored to one side; alignment-neutral (no ribbon).
    Comment(Side),
    /// The one-line bar standing in for a change segment the reviewer marked as
    /// viewed. It replaces the segment's removed *and* added rows, so it draws
    /// on both panes at one height — still a change, so the ribbon stays and
    /// simply shrinks to the bar.
    CollapsedChange,
}

/// One merged visual row: its pane classification and rendered height.
#[derive(Debug, Clone, Copy)]
pub struct SceneRow {
    pub class: RowClass,
    pub height: f32,
}

/// The flowing view's per-pane row lists (indices into the input) plus the
/// segments that couple the two panes' scroll offsets.
#[derive(Debug, Clone, Default)]
pub struct FlowScene {
    pub left_rows: Vec<usize>,
    pub right_rows: Vec<usize>,
    pub segments: Vec<FlowSegment>,
}

pub fn build_scene(rows: &[SceneRow]) -> FlowScene {
    let mut scene = FlowScene::default();
    // The open segment being accumulated: (kind, left_h, right_h). Flushed to
    // `segments` whenever the run's kind changes.
    let mut cur: Option<(SegKind, f32, f32)> = None;

    fn flush(scene: &mut FlowScene, cur: &mut Option<(SegKind, f32, f32)>) {
        if let Some((kind, left_h, right_h)) = cur.take() {
            scene.segments.push(FlowSegment { left_h, right_h, kind });
        }
    }

    for (i, row) in rows.iter().enumerate() {
        match row.class {
            RowClass::Context | RowClass::Hunk => {
                scene.left_rows.push(i);
                scene.right_rows.push(i);
                match &mut cur {
                    Some((SegKind::Equal, left_h, right_h)) => {
                        *left_h += row.height;
                        *right_h += row.height;
                    }
                    _ => {
                        flush(&mut scene, &mut cur);
                        cur = Some((SegKind::Equal, row.height, row.height));
                    }
                }
            }
            RowClass::Remove => {
                scene.left_rows.push(i);
                match &mut cur {
                    Some((SegKind::Change, left_h, _)) => *left_h += row.height,
                    _ => {
                        flush(&mut scene, &mut cur);
                        cur = Some((SegKind::Change, row.height, 0.0));
                    }
                }
            }
            RowClass::Add => {
                scene.right_rows.push(i);
                match &mut cur {
                    Some((SegKind::Change, _, right_h)) => *right_h += row.height,
                    _ => {
                        flush(&mut scene, &mut cur);
                        cur = Some((SegKind::Change, 0.0, row.height));
                    }
                }
            }
            // A collapsed bar is one whole segment by itself: it must not merge
            // with an adjacent run, or the bar and the ribbon would cover
            // different rows.
            RowClass::CollapsedChange => {
                flush(&mut scene, &mut cur);
                scene.left_rows.push(i);
                scene.right_rows.push(i);
                scene.segments.push(FlowSegment {
                    left_h: row.height,
                    right_h: row.height,
                    kind: SegKind::Change,
                });
            }
            // A comment is alignment-neutral: it never merges with an adjacent
            // run, so flush first and emit it as its own one-sided segment that
            // pauses the opposite pane while it scrolls past.
            RowClass::Comment(side) => {
                flush(&mut scene, &mut cur);
                let (left_h, right_h) = match side {
                    Side::Left => {
                        scene.left_rows.push(i);
                        (row.height, 0.0)
                    }
                    Side::Right => {
                        scene.right_rows.push(i);
                        (0.0, row.height)
                    }
                };
                scene.segments.push(FlowSegment { left_h, right_h, kind: SegKind::Comment });
            }
        }
    }
    flush(&mut scene, &mut cur);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow_map::FlowLayout;

    fn ctx(h: f32) -> SceneRow {
        SceneRow { class: RowClass::Context, height: h }
    }
    fn rm(h: f32) -> SceneRow {
        SceneRow { class: RowClass::Remove, height: h }
    }
    fn add(h: f32) -> SceneRow {
        SceneRow { class: RowClass::Add, height: h }
    }
    fn cmt(side: Side, h: f32) -> SceneRow {
        SceneRow { class: RowClass::Comment(side), height: h }
    }
    fn collapsed(h: f32) -> SceneRow {
        SceneRow { class: RowClass::CollapsedChange, height: h }
    }

    #[test]
    fn a_collapsed_change_bar_sits_on_both_panes_and_keeps_its_ribbon() {
        // One viewed segment, drawn as a single bar. The bar stands in for both
        // the removed and the added rows, so it renders on both panes at the
        // same height — but it is still a change, so the ribbon survives,
        // shrunk to the bar (D5).
        let scene = build_scene(&[ctx(18.0), collapsed(18.0), ctx(18.0)]);

        assert_eq!(scene.left_rows, vec![0, 1, 2]);
        assert_eq!(scene.right_rows, vec![0, 1, 2]);
        assert_eq!(scene.segments.len(), 3);
        assert_eq!(scene.segments[1].kind, SegKind::Change);
        assert_eq!(scene.segments[1].left_h, 18.0);
        assert_eq!(scene.segments[1].right_h, 18.0);
    }

    #[test]
    fn context_rows_go_to_both_panes_as_one_equal_segment() {
        let scene = build_scene(&[ctx(18.0), ctx(18.0)]);

        // Both context rows render on both panes, in order.
        assert_eq!(scene.left_rows, vec![0, 1]);
        assert_eq!(scene.right_rows, vec![0, 1]);
        // One Equal segment spanning both rows, equal on each side.
        assert_eq!(scene.segments.len(), 1);
        assert_eq!(scene.segments[0].kind, SegKind::Equal);
        assert_eq!(scene.segments[0].left_h, 36.0);
        assert_eq!(scene.segments[0].right_h, 36.0);
    }

    #[test]
    fn a_remove_then_add_run_becomes_one_change_segment() {
        let scene = build_scene(&[ctx(18.0), rm(18.0), rm(18.0), add(18.0), add(18.0), ctx(18.0)]);

        // Removes land on the left pane, adds on the right; shared context on both.
        assert_eq!(scene.left_rows, vec![0, 1, 2, 5]);
        assert_eq!(scene.right_rows, vec![0, 3, 4, 5]);

        // Equal, Change, Equal — the whole change block is one segment.
        assert_eq!(scene.segments.len(), 3);
        assert_eq!(scene.segments[1].kind, SegKind::Change);
        assert_eq!(scene.segments[1].left_h, 36.0);
        assert_eq!(scene.segments[1].right_h, 36.0);
    }

    #[test]
    fn pure_insertion_is_a_change_segment_with_no_left_height() {
        let scene = build_scene(&[ctx(18.0), add(18.0), add(18.0)]);

        assert_eq!(scene.left_rows, vec![0]);
        assert_eq!(scene.right_rows, vec![0, 1, 2]);
        assert_eq!(scene.segments[1].kind, SegKind::Change);
        assert_eq!(scene.segments[1].left_h, 0.0);
        assert_eq!(scene.segments[1].right_h, 36.0);
    }

    #[test]
    fn pure_deletion_is_a_change_segment_with_no_right_height() {
        let scene = build_scene(&[ctx(18.0), rm(18.0), rm(18.0)]);

        assert_eq!(scene.left_rows, vec![0, 1, 2]);
        assert_eq!(scene.right_rows, vec![0]);
        assert_eq!(scene.segments[1].kind, SegKind::Change);
        assert_eq!(scene.segments[1].left_h, 36.0);
        assert_eq!(scene.segments[1].right_h, 0.0);
    }

    #[test]
    fn a_left_comment_is_a_one_sided_comment_segment() {
        let scene = build_scene(&[ctx(18.0), cmt(Side::Left, 80.0)]);

        // Comment renders on its anchored (left) pane only.
        assert_eq!(scene.left_rows, vec![0, 1]);
        assert_eq!(scene.right_rows, vec![0]);
        // Alignment-neutral: its own Comment segment, one-sided height, no ribbon.
        assert_eq!(scene.segments.len(), 2);
        assert_eq!(scene.segments[1].kind, SegKind::Comment);
        assert_eq!(scene.segments[1].left_h, 80.0);
        assert_eq!(scene.segments[1].right_h, 0.0);
    }

    #[test]
    fn a_right_comment_is_a_one_sided_comment_segment_on_the_right() {
        let scene = build_scene(&[ctx(18.0), cmt(Side::Right, 80.0)]);

        assert_eq!(scene.left_rows, vec![0]);
        assert_eq!(scene.right_rows, vec![0, 1]);
        assert_eq!(scene.segments[1].kind, SegKind::Comment);
        assert_eq!(scene.segments[1].left_h, 0.0);
        assert_eq!(scene.segments[1].right_h, 80.0);
    }

    #[test]
    fn empty_input_yields_an_empty_scene() {
        let scene = build_scene(&[]);
        assert!(scene.left_rows.is_empty());
        assert!(scene.right_rows.is_empty());
        assert!(scene.segments.is_empty());
    }

    #[test]
    fn pane_layout_totals_match_the_rows_routed_to_each_pane() {
        // The invariant the whole flowing view rests on: the FlowLayout built from
        // the segments must place each pane's content at exactly the cumulative
        // height of the rows that landed on that pane — otherwise offsets drift.
        let rows = [
            ctx(18.0),
            rm(18.0),
            rm(18.0),
            add(18.0),
            add(18.0),
            add(18.0),
            ctx(18.0),
            cmt(Side::Left, 80.0),
            ctx(18.0),
        ];
        let scene = build_scene(&rows);
        let layout = FlowLayout::build(&scene.segments);

        let sum = |idxs: &[usize]| idxs.iter().map(|&i| rows[i].height).sum::<f32>();
        assert_eq!(layout.total_left, sum(&scene.left_rows));
        assert_eq!(layout.total_right, sum(&scene.right_rows));
    }
}
