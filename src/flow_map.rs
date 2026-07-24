//! Scroll-coupling geometry for the flowing side-by-side diff view.
//!
//! The two panes render continuously with no blank filler rows, so they sit at
//! *different* vertical offsets. A single virtual axis drives both: each segment
//! of the diff occupies `max(left_h, right_h)` on that axis, and within a segment
//! each pane's offset is linearly interpolated. Equal segments advance both panes
//! in lockstep; a segment where the sides differ in height makes them advance at
//! different rates — that rate difference is what the ribbon in the center gutter
//! visualises, and it is why the panes "flow" into each other.
//!
//! Deliberately free of UI and git types so the throwaway spike example can
//! include it directly and so the mapping can be unit-tested as pure arithmetic.

/// What a segment represents, which decides whether the gutter draws a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    /// Identical context on both sides: lockstep, no ribbon.
    Equal,
    /// A change block (removes vs adds, either side possibly empty): draws a ribbon.
    Change,
    /// A one-sided insertion such as a PR comment card: no ribbon (alignment-neutral).
    Comment,
}

/// One aligned region of the diff, sized in pixels on each side.
#[derive(Debug, Clone, Copy)]
pub struct FlowSegment {
    pub left_h: f32,
    pub right_h: f32,
    pub kind: SegKind,
}

/// Precomputed placement of one segment on both panes and the virtual axis.
#[derive(Debug, Clone, Copy)]
pub struct SegPlacement {
    pub left_start: f32,
    pub left_h: f32,
    pub right_start: f32,
    pub right_h: f32,
    pub virt_start: f32,
    pub virt_h: f32,
    pub kind: SegKind,
}

/// The laid-out flow: per-segment placements plus totals for each axis.
#[derive(Debug, Clone, Default)]
pub struct FlowLayout {
    pub segments: Vec<SegPlacement>,
    pub total_left: f32,
    pub total_right: f32,
    pub total_virt: f32,
}

impl FlowLayout {
    pub fn build(segments: &[FlowSegment]) -> Self {
        let mut placements = Vec::with_capacity(segments.len());
        let (mut left, mut right, mut virt) = (0.0f32, 0.0f32, 0.0f32);
        for seg in segments {
            let virt_h = seg.left_h.max(seg.right_h);
            placements.push(SegPlacement {
                left_start: left,
                left_h: seg.left_h,
                right_start: right,
                right_h: seg.right_h,
                virt_start: virt,
                virt_h,
                kind: seg.kind,
            });
            left += seg.left_h;
            right += seg.right_h;
            virt += virt_h;
        }
        FlowLayout {
            segments: placements,
            total_left: left,
            total_right: right,
            total_virt: virt,
        }
    }

    /// Map a position on the virtual axis to each pane's scroll offset.
    pub fn map_scroll(&self, virtual_y: f32) -> (f32, f32) {
        let v = virtual_y.clamp(0.0, self.total_virt);
        for seg in &self.segments {
            if v < seg.virt_start + seg.virt_h {
                // Fraction through this segment; a zero-height virtual span can't
                // contain a clamped v below the total, so the guard is defensive.
                let t = if seg.virt_h > 0.0 {
                    (v - seg.virt_start) / seg.virt_h
                } else {
                    0.0
                };
                return (seg.left_start + t * seg.left_h, seg.right_start + t * seg.right_h);
            }
        }
        // v == total_virt (or no segments): rest at the far end.
        (self.total_left, self.total_right)
    }

    pub fn total_virtual_height(&self) -> f32 {
        self.total_virt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_segment_advances_both_panes_in_lockstep() {
        let layout = FlowLayout::build(&[FlowSegment {
            left_h: 100.0,
            right_h: 100.0,
            kind: SegKind::Equal,
        }]);

        assert_eq!(layout.map_scroll(50.0), (50.0, 50.0));
        assert_eq!(layout.total_virtual_height(), 100.0);
    }

    #[test]
    fn change_segment_advances_the_two_panes_at_different_rates() {
        // 100px of shared context, then a change block: 20px removed vs 40px added.
        let layout = FlowLayout::build(&[
            FlowSegment { left_h: 100.0, right_h: 100.0, kind: SegKind::Equal },
            FlowSegment { left_h: 20.0, right_h: 40.0, kind: SegKind::Change },
        ]);

        // Virtual height of the change block is max(20,40) = 40 → total 140.
        assert_eq!(layout.total_virtual_height(), 140.0);
        // Top of the change block: both panes at the block's start.
        assert_eq!(layout.map_scroll(100.0), (100.0, 100.0));
        // Halfway through the block (t=0.5): left crept 10, right raced 20.
        assert_eq!(layout.map_scroll(120.0), (110.0, 120.0));
    }

    #[test]
    fn pure_insertion_pauses_the_left_pane_while_the_right_advances() {
        // Nothing removed, 30px added: the left side has no rows to scroll here.
        let layout = FlowLayout::build(&[
            FlowSegment { left_h: 50.0, right_h: 50.0, kind: SegKind::Equal },
            FlowSegment { left_h: 0.0, right_h: 30.0, kind: SegKind::Change },
        ]);

        let (l0, r0) = layout.map_scroll(50.0);
        let (l1, r1) = layout.map_scroll(65.0);
        assert_eq!((l0, r0), (50.0, 50.0));
        // Left frozen at 50 through the whole insertion; right climbs.
        assert_eq!((l1, r1), (50.0, 65.0));
    }

    #[test]
    fn scroll_is_clamped_at_both_ends() {
        let layout = FlowLayout::build(&[
            FlowSegment { left_h: 20.0, right_h: 40.0, kind: SegKind::Change },
        ]);

        assert_eq!(layout.map_scroll(-100.0), (0.0, 0.0));
        // Past the end rests each pane at its own total height.
        assert_eq!(layout.map_scroll(999.0), (20.0, 40.0));
    }

    #[test]
    fn comment_segment_is_one_sided_and_carries_its_kind() {
        // An 80px comment card anchored on the left side only.
        let layout = FlowLayout::build(&[
            FlowSegment { left_h: 80.0, right_h: 0.0, kind: SegKind::Comment },
        ]);

        assert_eq!(layout.segments[0].kind, SegKind::Comment);
        assert_eq!(layout.total_virtual_height(), 80.0);
        // Right pane stays put while the comment scrolls past on the left.
        assert_eq!(layout.map_scroll(40.0), (40.0, 0.0));
    }
}
