//! THROWAWAY spike for diff-ict: does the flowing (JetBrains-style) side-by-side
//! diff feel right? Two panes scroll at decoupled offsets driven by one virtual
//! axis (src/flow_map.rs), and the center gutter draws ribbons between change
//! blocks. Run it, scroll, and judge:
//!
//!   cargo run --example flowing_spike
//!
//! Things to look for:
//!   * Do the ribbons stay glued to their blocks while scrolling?
//!   * Does a lopsided change (2 removed vs 4 added) "flow" or feel jerky?
//!   * Does the one-sided comment card (left only) scroll past cleanly with a
//!     straight pass-through — i.e. no bogus slant — in the gutter?
//!   * Pure insert / pure delete: does the empty side pause acceptably?
//!
//! If the answer is "janky", we fall back to aligned+ribbons (same math, filler
//! rows rendered back in). Nothing here is meant to survive except flow_map.rs.

#[path = "../src/flow_map.rs"]
mod flow_map;

use flow_map::{FlowLayout, FlowSegment, SegKind};
use slint::{Color, ModelRc, VecModel};
use std::rc::Rc;

const ROW_H: f32 = 18.0;
const COMMENT_H: f32 = 80.0;
/// Half-thickness of the hairline an insert/delete draws across the empty pane.
/// Total line thickness is `2 * SEAM_HALF` logical px.
const SEAM_HALF: f32 = 0.125;

fn ctx_bg() -> Color {
    Color::from_argb_u8(0, 0, 0, 0)
}
fn add_bg() -> Color {
    Color::from_argb_u8(255, 20, 48, 30)
}
fn rem_bg() -> Color {
    Color::from_argb_u8(255, 58, 22, 22)
}
fn comment_bg() -> Color {
    Color::from_argb_u8(255, 35, 40, 47)
}

/// Ribbon fill (translucent) and stroke (crisp) for a change kind. The stroke
/// keeps a collapsed 1px edge visible where the fill alone would be too faint.
fn ribbon_modified() -> (Color, Color) {
    (
        Color::from_argb_u8(70, 90, 150, 230),
        Color::from_argb_u8(190, 120, 170, 240),
    )
}
fn ribbon_add() -> (Color, Color) {
    (
        Color::from_argb_u8(70, 70, 190, 100),
        Color::from_argb_u8(190, 90, 200, 120),
    )
}
fn ribbon_remove() -> (Color, Color) {
    (
        Color::from_argb_u8(64, 210, 90, 90),
        Color::from_argb_u8(190, 220, 110, 110),
    )
}

/// Vertical extent of one side of a ribbon in content space. A side with no code
/// (pure insert or delete) collapses to a 1px band centred on the change seam,
/// so the ribbon tapers to a thin connector line rather than a bare point.
fn ribbon_edges(start: f32, h: f32) -> (f32, f32) {
    if h <= 0.0 {
        (start - SEAM_HALF, start + SEAM_HALF)
    } else {
        (start, start + h)
    }
}

/// One rendered line on a pane. Height is explicit so a comment card can be
/// taller than a code row without breaking the cumulative offset math.
struct SceneRow {
    text: String,
    bg: Color,
    h: f32,
}

fn code(n: &str, text: &str, bg: Color) -> SceneRow {
    SceneRow {
        text: format!("{n:>4}  {text}"),
        bg,
        h: ROW_H,
    }
}

/// Build the representative diff: parallel left/right row lists plus the segment
/// list that couples them. Each segment's per-side height is the sum of that
/// side's row heights, so pane content offsets line up with FlowLayout.
fn build_scene() -> (Vec<FlowSegment>, Vec<SceneRow>, Vec<SceneRow>) {
    let mut segs = Vec::new();
    let mut left = Vec::new();
    let mut right = Vec::new();

    let equal = |segs: &mut Vec<FlowSegment>,
                 left: &mut Vec<SceneRow>,
                 right: &mut Vec<SceneRow>,
                 from: u32,
                 lines: &[&str]| {
        for (i, t) in lines.iter().enumerate() {
            let n = (from + i as u32).to_string();
            left.push(code(&n, t, ctx_bg()));
            right.push(code(&n, t, ctx_bg()));
        }
        segs.push(FlowSegment {
            left_h: lines.len() as f32 * ROW_H,
            right_h: lines.len() as f32 * ROW_H,
            kind: SegKind::Equal,
        });
    };

    equal(
        &mut segs,
        &mut left,
        &mut right,
        1,
        &[
            "fn render(&self) {",
            "    let theme = self.theme();",
            "    let mut out = String::new();",
            "    for line in &self.lines {",
            "        out.push_str(line);",
            "    }",
        ],
    );

    // Modified block: 2 removed vs 4 added — the asymmetric case that shows flow.
    for (n, t) in [("7", "    return out;"), ("8", "}")] {
        left.push(code(n, t, rem_bg()));
    }
    for (n, t) in [
        ("7", "    out.push('\\n');"),
        ("8", "    self.cache = Some(out.clone());"),
        ("9", "    return out;"),
        ("10", "}"),
    ] {
        right.push(code(n, t, add_bg()));
    }
    segs.push(FlowSegment {
        left_h: 2.0 * ROW_H,
        right_h: 4.0 * ROW_H,
        kind: SegKind::Change,
    });

    equal(
        &mut segs,
        &mut left,
        &mut right,
        11,
        &[
            "",
            "fn theme(&self) -> Theme {",
            "    self.config.theme.clone()",
            "}",
        ],
    );

    // One-sided PR comment card anchored on the LEFT. Alignment-neutral: the
    // gutter must NOT slant here; the right pane simply waits.
    left.push(SceneRow {
        text: "  💬 reviewer: clone() here is wasteful — take &Theme?".to_string(),
        bg: comment_bg(),
        h: COMMENT_H,
    });
    segs.push(FlowSegment {
        left_h: COMMENT_H,
        right_h: 0.0,
        kind: SegKind::Comment,
    });

    equal(
        &mut segs,
        &mut left,
        &mut right,
        15,
        &["", "fn config(&self) -> &Config {", "    &self.config"],
    );

    // Pure insertion: 3 lines added, nothing removed. Left pane pauses.
    for (n, t) in [
        ("18", "    #[cfg(test)]"),
        ("19", "    fn probe() -> bool {"),
        ("20", "        true }"),
    ] {
        right.push(code(n, t, add_bg()));
    }
    segs.push(FlowSegment {
        left_h: 0.0,
        right_h: 3.0 * ROW_H,
        kind: SegKind::Change,
    });

    equal(
        &mut segs,
        &mut left,
        &mut right,
        18,
        &[
            "}",
            "",
            "impl Default for View {",
            "    fn default() -> Self {",
            "        Self::new()",
        ],
    );

    // Pure deletion: 3 lines removed, nothing added. Right pane pauses.
    for (n, t) in [
        ("23", "    // legacy path"),
        ("24", "    fn old_render(&self) {}"),
        ("25", "    // end legacy"),
    ] {
        left.push(code(n, t, rem_bg()));
    }
    segs.push(FlowSegment {
        left_h: 3.0 * ROW_H,
        right_h: 0.0,
        kind: SegKind::Change,
    });

    equal(
        &mut segs,
        &mut left,
        &mut right,
        23,
        &[
            "    }",
            "}",
            "",
            "// trailing context so there is room to scroll",
            "// past the last change block and watch the",
            "// ribbons settle at the bottom of the gutter.",
            "// ...",
            "// end",
        ],
    );

    (segs, left, right)
}

fn to_rows(rows: &[SceneRow]) -> ModelRc<Row> {
    let v: Vec<Row> = rows
        .iter()
        .map(|r| Row {
            text: r.text.clone().into(),
            bg: r.bg,
            h: r.h,
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(v)))
}

fn main() -> Result<(), slint::PlatformError> {
    let (segs, left_rows, right_rows) = build_scene();
    let layout = FlowLayout::build(&segs);

    // One ribbon per change block, positioned in content-space; the UI subtracts
    // the live pane offsets so they track scroll.
    let ribbons: Vec<Ribbon> = layout
        .segments
        .iter()
        .filter(|s| s.kind == SegKind::Change)
        .map(|s| {
            let (fill, stroke) = if s.left_h == 0.0 {
                ribbon_add()
            } else if s.right_h == 0.0 {
                ribbon_remove()
            } else {
                ribbon_modified()
            };
            let (left_top, left_bottom) = ribbon_edges(s.left_start, s.left_h);
            let (right_top, right_bottom) = ribbon_edges(s.right_start, s.right_h);
            Ribbon {
                left_top,
                left_bottom,
                right_top,
                right_bottom,
                fill,
                stroke,
                left_empty: s.left_h == 0.0,
                right_empty: s.right_h == 0.0,
            }
        })
        .collect();

    let win = FlowSpike::new()?;
    win.set_left_rows(to_rows(&left_rows));
    win.set_right_rows(to_rows(&right_rows));
    win.set_ribbons(ModelRc::from(Rc::new(VecModel::from(ribbons))));
    win.set_total_virt(layout.total_virt);

    // Remap the virtual scroll position to each pane's offset. Piecewise, so it
    // lives in Rust rather than a Slint expression.
    let weak = win.as_weak();
    win.on_remap(move || {
        if let Some(w) = weak.upgrade() {
            let (l, r) = layout.map_scroll(w.get_virtual_y());
            w.set_left_y(l);
            w.set_right_y(r);
        }
    });

    win.run()
}

slint::slint! {
    struct Row { text: string, bg: color, h: length }
    struct Ribbon {
        left-top: length, left-bottom: length,
        right-top: length, right-bottom: length,
        fill: color, stroke: color,
        left-empty: bool, right-empty: bool,
    }

    component Pane inherits Rectangle {
        in property <[Row]> rows;
        in property <length> offset;
        clip: true;
        background: #1e1e1e;
        VerticalLayout {
            y: -root.offset;
            alignment: start;
            for r in rows: Rectangle {
                height: r.h;
                background: r.bg;
                Text {
                    x: 6px;
                    text: r.text;
                    font-family: "monospace";
                    font-size: 12px;
                    color: #d8d8d8;
                    vertical-alignment: center;
                    horizontal-alignment: left;
                }
            }
        }
    }

    export component FlowSpike inherits Window {
        in property <[Row]> left-rows;
        in property <[Row]> right-rows;
        in property <[Ribbon]> ribbons;
        in property <length> gutter-w: 64px;
        in property <length> total-virt: 0;

        in-out property <length> virtual-y: 0;
        in-out property <length> left-y: 0;
        in-out property <length> right-y: 0;
        callback remap();

        property <length> max-scroll: max(0px, root.total-virt - self.height);

        preferred-width: 960px;
        preferred-height: 620px;
        title: "flowing diff spike";
        background: #141414;

        HorizontalLayout {
            spacing: 0px;
            Pane {
                rows: root.left-rows;
                offset: root.left-y;
                horizontal-stretch: 1;
            }

            // Plain center gutter; connectors are drawn in the full-width overlay
            // below so an empty side's arm can run across its pane.
            Rectangle {
                width: root.gutter-w;
                background: #101010;
            }

            Pane {
                rows: root.right-rows;
                offset: root.right-y;
                horizontal-stretch: 1;
            }
        }

        // Full-width ribbon overlay. Connectors sit above the panes + gutter, so
        // for a pure insert/delete the empty side continues as a ~1px line all the
        // way to that pane's outer edge — a JetBrains-style change marker. The fill
        // only ever lands on the gutter or an empty pane, never over code text.
        Rectangle {
            width: 100%;
            height: 100%;
            clip: true;

            for rb in root.ribbons: pe := Path {
                // Pane/gutter x boundaries; an empty side extends its arm outward.
                property <length> lpw: (root.width - root.gutter-w) / 2;
                property <length> gl: self.lpw;
                property <length> gr: self.lpw + root.gutter-w;
                property <length> mid: (self.gl + self.gr) / 2;
                property <length> le: rb.left-empty ? 0px : self.gl;
                property <length> re: rb.right-empty ? root.width : self.gr;
                // Screen-space y of each side's band (content minus live offset).
                property <length> lt: rb.left-top - root.left-y;
                property <length> lb: rb.left-bottom - root.left-y;
                property <length> rt: rb.right-top - root.right-y;
                property <length> rbo: rb.right-bottom - root.right-y;

                viewbox-x: 0;
                viewbox-y: 0;
                viewbox-width: root.width / 1px;
                viewbox-height: root.height / 1px;
                clip: true;
                fill: rb.fill;

                MoveTo { x: pe.le / 1px; y: pe.lt / 1px; }
                LineTo { x: pe.gl / 1px; y: pe.lt / 1px; }
                CubicTo {
                    control-1-x: pe.mid / 1px; control-1-y: pe.lt / 1px;
                    control-2-x: pe.mid / 1px; control-2-y: pe.rt / 1px;
                    x: pe.gr / 1px; y: pe.rt / 1px;
                }
                LineTo { x: pe.re / 1px; y: pe.rt / 1px; }
                LineTo { x: pe.re / 1px; y: pe.rbo / 1px; }
                LineTo { x: pe.gr / 1px; y: pe.rbo / 1px; }
                CubicTo {
                    control-1-x: pe.mid / 1px; control-1-y: pe.rbo / 1px;
                    control-2-x: pe.mid / 1px; control-2-y: pe.lb / 1px;
                    x: pe.gl / 1px; y: pe.lb / 1px;
                }
                LineTo { x: pe.le / 1px; y: pe.lb / 1px; }
                Close {}
            }
        }

        // Single wheel driver for both panes.
        TouchArea {
            scroll-event(e) => {
                root.virtual-y = clamp(root.virtual-y - e.delta-y, 0px, root.max-scroll);
                root.remap();
                accept
            }
        }
    }
}
