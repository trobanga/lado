//! PROTOTYPE — THROWAWAY. Do not import from this file.
//!
//! Question (ticket `diff-93a.1` on map `diff-93a`):
//! does c4ai's `render_focus_view` layout + visual encoding survive being asked
//! to draw a *diff* (added / removed / changed), or does a diff need its own renderer?
//!
//! Method: the layout algorithm from `../c4ai/crates/c4ai-gui/src/renderer.rs` is copied
//! verbatim, then four different *visual encodings* of the same synthetic before/after
//! graph are rendered so they can be compared side by side. The layout code is untouched
//! so that any layout problem observed here is c4ai's, not the prototype's.
//!
//! NOTE ON TEXT: the real renderer emits NO `<text>` — c4ai draws labels as Slint overlays
//! because resvg ships an empty fontdb. This prototype DOES emit text, so the pictures are
//! legible. Everything drawn in grey italic is simulating what Slint would overlay; it is
//! not something the SVG gives you for free.
//!
//! Run: cargo run --example c4diff_spike

use std::collections::HashMap;

// ─────────────────────────── copied verbatim from c4ai ───────────────────────────

const NODE_W: f32 = 200.0;
const NODE_H: f32 = 70.0;
const BOUNDARY_PAD: f32 = 40.0;
const CANVAS_PAD: f32 = 40.0;
const TITLE_H: f32 = 48.0;
const GRID_GAP_X: f32 = 60.0;
const GRID_GAP_Y: f32 = 60.0;
const EXTERNAL_OFFSET: f32 = 160.0;

const COLOR_INTRA: &str = "#1d4ed8";
const COLOR_CROSS: &str = "#ea580c";
const COLOR_BOUNDARY: &str = "#475569";
const COLOR_BG: &str = "#f8fafc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Child,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeClass {
    Intra,
    Cross,
}

// ─────────────────────────── the prototype's addition ───────────────────────────

/// What the diff says happened to this node/edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffState {
    Unchanged,
    Added,
    Removed,
    Changed,
}

const COLOR_ADDED: &str = "#16a34a";
const COLOR_REMOVED: &str = "#dc2626";
const COLOR_CHANGED: &str = "#d97706";
const COLOR_UNCHANGED: &str = "#94a3b8";

fn diff_color(d: DiffState) -> &'static str {
    match d {
        DiffState::Added => COLOR_ADDED,
        DiffState::Removed => COLOR_REMOVED,
        DiffState::Changed => COLOR_CHANGED,
        DiffState::Unchanged => COLOR_UNCHANGED,
    }
}

/// Which visual channel carries the diff state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    /// Fill = diff state. Layer information is destroyed.
    DiffAsFill,
    /// Fill = layer (as c4ai does today), thick stroke = diff state.
    DiffAsStroke,
    /// Fill = diff state, stroke = layer. The two encodings swapped.
    DiffFillLayerStroke,
    /// Fill = layer, diff state as a badge stripe down the left edge.
    DiffAsBadge,
}

impl Encoding {
    fn slug(self) -> &'static str {
        match self {
            Encoding::DiffAsFill => "v1-diff-as-fill",
            Encoding::DiffAsStroke => "v2-diff-as-stroke",
            Encoding::DiffFillLayerStroke => "v3-diff-fill-layer-stroke",
            Encoding::DiffAsBadge => "v4-diff-as-badge",
        }
    }
}

#[derive(Debug, Clone)]
struct FocusNode {
    id: String,
    title: String,
    layer: String,
    kind: NodeKind,
    diff: DiffState,
}

#[derive(Debug, Clone)]
struct FocusEdge {
    from: String,
    to: String,
    class: EdgeClass,
    diff: DiffState,
}

#[derive(Debug, Clone)]
struct FocusInput {
    nodes: Vec<FocusNode>,
    edges: Vec<FocusEdge>,
}

// ─────────────────────── layout: copied verbatim, unmodified ───────────────────────

fn compute_layout(input: &FocusInput) -> HashMap<String, (f32, f32)> {
    let mut out: HashMap<String, (f32, f32)> = HashMap::new();
    let pitch_x = NODE_W + GRID_GAP_X;
    let pitch_y = NODE_H + GRID_GAP_Y;

    let children: Vec<&FocusNode> = input
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Child)
        .collect();

    let nc = children.len();
    let cols = (nc as f32).sqrt().ceil().max(1.0) as usize;
    let rows = nc.div_ceil(cols).max(1);
    let grid_w = cols as f32 * pitch_x;
    let grid_h = rows as f32 * pitch_y;

    for (i, node) in children.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = col as f32 * pitch_x - grid_w / 2.0 + pitch_x / 2.0;
        let y = row as f32 * pitch_y - grid_h / 2.0 + pitch_y / 2.0;
        out.insert(node.id.clone(), (x, y));
    }

    let externals: Vec<&FocusNode> = input
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::External)
        .collect();
    if externals.is_empty() {
        return out;
    }

    let child_half = (grid_w.max(grid_h) / 2.0).max(NODE_W / 2.0);
    let outer = child_half + EXTERNAL_OFFSET;

    let mut used_angles: Vec<f32> = Vec::new();
    for (i, ext) in externals.iter().enumerate() {
        let mut cx = 0.0_f32;
        let mut cy = 0.0_f32;
        let mut count = 0u32;
        for edge in &input.edges {
            if edge.to == ext.id {
                if let Some(&(px, py)) = out.get(&edge.from) {
                    cx += px;
                    cy += py;
                    count += 1;
                }
            }
        }
        let mut theta = if count > 0 {
            cy.atan2(cx)
        } else {
            -std::f32::consts::FRAC_PI_2
                + (i as f32 + 1.0) / (externals.len() as f32 + 1.0) * std::f32::consts::PI
        };
        for &t in &used_angles {
            if (theta - t).abs() < 0.35 {
                theta += 0.45;
            }
        }
        used_angles.push(theta);
        out.insert(ext.id.clone(), (outer * theta.cos(), outer * theta.sin()));
    }

    out
}

fn bbox_of_kind(
    positions: &HashMap<String, (f32, f32)>,
    nodes: &[FocusNode],
    kind: NodeKind,
) -> ((f32, f32, f32, f32), usize) {
    let mut count = 0;
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for n in nodes.iter().filter(|n| n.kind == kind) {
        let Some(&(px, py)) = positions.get(&n.id) else {
            continue;
        };
        count += 1;
        min_x = min_x.min(px - NODE_W / 2.0);
        min_y = min_y.min(py - NODE_H / 2.0);
        max_x = max_x.max(px + NODE_W / 2.0);
        max_y = max_y.max(py + NODE_H / 2.0);
    }
    if count == 0 {
        return ((0.0, 0.0, 0.0, 0.0), 0);
    }
    ((min_x, min_y, max_x - min_x, max_y - min_y), count)
}

fn layer_color(layer: &str) -> &'static str {
    match layer {
        "Context" => "#3b82f6",
        "Container" => "#22c55e",
        "Component" => "#f97316",
        "Code" => "#a855f7",
        _ => "#6b7280",
    }
}

fn trim_to_box(x1: f32, y1: f32, x2: f32, y2: f32) -> (f32, f32, f32, f32) {
    let half_w = NODE_W / 2.0;
    let half_h = NODE_H / 2.0;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let t_start = intersect_t(ux, uy, half_w, half_h);
    let t_end = intersect_t(-ux, -uy, half_w, half_h);
    (
        x1 + ux * t_start,
        y1 + uy * t_start,
        x2 - ux * t_end,
        y2 - uy * t_end,
    )
}

fn intersect_t(ux: f32, uy: f32, half_w: f32, half_h: f32) -> f32 {
    let tx = if ux.abs() > 1e-4 {
        half_w / ux.abs()
    } else {
        f32::INFINITY
    };
    let ty = if uy.abs() > 1e-4 {
        half_h / uy.abs()
    } else {
        f32::INFINITY
    };
    tx.min(ty)
}

// ─────────────────────────────── rendering ───────────────────────────────

fn render(input: &FocusInput, enc: Encoding, title: &str) -> (String, HashMap<String, (f32, f32)>) {
    let positions = compute_layout(input);
    let (children_bb, child_count) = bbox_of_kind(&positions, &input.nodes, NodeKind::Child);

    let (mut min_x, mut min_y) = (children_bb.0, children_bb.1);
    let (mut max_x, mut max_y) = (children_bb.0 + children_bb.2, children_bb.1 + children_bb.3);
    for (px, py) in positions.values() {
        min_x = min_x.min(px - NODE_W / 2.0);
        min_y = min_y.min(py - NODE_H / 2.0);
        max_x = max_x.max(px + NODE_W / 2.0);
        max_y = max_y.max(py + NODE_H / 2.0);
    }

    let dx = -min_x + CANVAS_PAD;
    let dy = -min_y + CANVAS_PAD + TITLE_H;
    let canvas_w = (max_x - min_x) + CANVAS_PAD * 2.0;
    let canvas_h = (max_y - min_y) + CANVAS_PAD * 2.0 + TITLE_H;

    let translated: HashMap<String, (f32, f32)> = positions
        .iter()
        .map(|(id, (x, y))| (id.clone(), (x + dx, y + dy)))
        .collect();

    let bnd = if child_count > 0 {
        Some((
            children_bb.0 + dx - BOUNDARY_PAD,
            children_bb.1 + dy - BOUNDARY_PAD,
            children_bb.2 + BOUNDARY_PAD * 2.0,
            children_bb.3 + BOUNDARY_PAD * 2.0,
        ))
    } else {
        None
    };

    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{canvas_w:.0}\" height=\"{canvas_h:.0}\" viewBox=\"0 0 {canvas_w:.0} {canvas_h:.0}\">\n"
    ));
    s.push_str(&format!(
        "  <rect width=\"{canvas_w:.0}\" height=\"{canvas_h:.0}\" fill=\"{COLOR_BG}\"/>\n"
    ));

    s.push_str("  <defs>\n");
    for (id, col) in [
        ("intra", COLOR_INTRA),
        ("cross", COLOR_CROSS),
        ("added", COLOR_ADDED),
        ("removed", COLOR_REMOVED),
        ("changed", COLOR_CHANGED),
        ("unchanged", COLOR_UNCHANGED),
    ] {
        s.push_str(&format!(
            "    <marker id=\"arrow-{id}\" markerWidth=\"10\" markerHeight=\"8\" refX=\"9\" refY=\"4\" orient=\"auto\"><path d=\"M0,0 L10,4 L0,8 Z\" fill=\"{col}\"/></marker>\n"
        ));
    }
    s.push_str("  </defs>\n");

    // SIMULATED SLINT OVERLAY (the real SVG has no text at all)
    s.push_str(&format!(
        "  <text x=\"{CANVAS_PAD}\" y=\"32\" font-family=\"sans-serif\" font-size=\"20\" font-weight=\"bold\" fill=\"#0f172a\">{title}</text>\n"
    ));

    if let Some((bx, by, bw, bh)) = bnd {
        s.push_str(&format!(
            "  <rect x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{bw:.1}\" height=\"{bh:.1}\" rx=\"12\" fill=\"none\" stroke=\"{COLOR_BOUNDARY}\" stroke-width=\"2\" stroke-dasharray=\"6,5\"/>\n"
        ));
    }

    for edge in &input.edges {
        let (Some(&(x1, y1)), Some(&(x2, y2))) =
            (translated.get(&edge.from), translated.get(&edge.to))
        else {
            continue;
        };
        let (sx, sy, ex, ey) = trim_to_box(x1, y1, x2, y2);
        // Edge diff state has nowhere else to go — it always takes the colour channel,
        // which means Intra/Cross (c4ai's existing edge encoding) is destroyed.
        let (color, marker) = match edge.diff {
            DiffState::Unchanged => match edge.class {
                EdgeClass::Intra => (COLOR_INTRA, "url(#arrow-intra)"),
                EdgeClass::Cross => (COLOR_CROSS, "url(#arrow-cross)"),
            },
            d => (diff_color(d), {
                match d {
                    DiffState::Added => "url(#arrow-added)",
                    DiffState::Removed => "url(#arrow-removed)",
                    _ => "url(#arrow-changed)",
                }
            }),
        };
        let dash = if edge.diff == DiffState::Removed {
            " stroke-dasharray=\"8,6\""
        } else {
            ""
        };
        let width = if edge.diff == DiffState::Unchanged {
            2.0
        } else {
            3.0
        };
        s.push_str(&format!(
            "  <line x1=\"{sx:.1}\" y1=\"{sy:.1}\" x2=\"{ex:.1}\" y2=\"{ey:.1}\" stroke=\"{color}\" stroke-width=\"{width}\"{dash} marker-end=\"{marker}\"/>\n"
        ));
    }

    for node in &input.nodes {
        let Some(&(cx, cy)) = translated.get(&node.id) else {
            continue;
        };
        draw_node(&mut s, cx, cy, node, enc);
    }

    s.push_str("</svg>");
    (s, translated)
}

fn draw_node(s: &mut String, cx: f32, cy: f32, node: &FocusNode, enc: Encoding) {
    let x = cx - NODE_W / 2.0;
    let y = cy - NODE_H / 2.0;
    let lc = layer_color(&node.layer);
    let dc = diff_color(node.diff);
    let ext_op = if node.kind == NodeKind::External {
        0.45
    } else {
        1.0
    };

    let (fill, stroke, stroke_w) = match enc {
        Encoding::DiffAsFill => (dc, "none", 0.0),
        Encoding::DiffAsStroke => (lc, dc, 6.0),
        Encoding::DiffFillLayerStroke => (dc, lc, 6.0),
        Encoding::DiffAsBadge => (lc, "none", 0.0),
    };

    let dash = if node.diff == DiffState::Removed {
        " stroke-dasharray=\"10,6\""
    } else {
        ""
    };

    s.push_str(&format!("  <g opacity=\"{ext_op}\">\n"));
    s.push_str(&format!(
        "    <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{NODE_W:.0}\" height=\"{NODE_H:.0}\" rx=\"8\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_w}\"{dash}/>\n"
    ));

    if enc == Encoding::DiffAsBadge {
        s.push_str(&format!(
            "    <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"14\" height=\"{NODE_H:.0}\" rx=\"8\" fill=\"{dc}\"/>\n"
        ));
    }

    // SIMULATED SLINT OVERLAY — the real SVG emits no text.
    let tx = if enc == Encoding::DiffAsBadge {
        cx + 7.0
    } else {
        cx
    };
    s.push_str(&format!(
        "    <text x=\"{tx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"15\" font-weight=\"bold\" fill=\"#ffffff\">{}</text>\n",
        cy + 1.0,
        node.title
    ));
    s.push_str(&format!(
        "    <text x=\"{tx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"11\" font-style=\"italic\" fill=\"#f1f5f9\">{}</text>\n",
        cy + 19.0,
        node.layer
    ));
    s.push_str("  </g>\n");
}

// ─────────────────────── synthetic scenario ───────────────────────

fn n(id: &str, title: &str, layer: &str, kind: NodeKind, diff: DiffState) -> FocusNode {
    FocusNode {
        id: id.into(),
        title: title.into(),
        layer: layer.into(),
        kind,
        diff,
    }
}

fn e(from: &str, to: &str, class: EdgeClass, diff: DiffState) -> FocusEdge {
    FocusEdge {
        from: from.into(),
        to: to.into(),
        class,
        diff,
    }
}

/// The "before" side: lado as it exists today.
fn scenario_before() -> FocusInput {
    use DiffState::Unchanged as U;
    use EdgeClass::*;
    use NodeKind::*;
    FocusInput {
        nodes: vec![
            n("app", "App", "Component", Child, U),
            n("repo", "Repository", "Component", Child, U),
            n("gh", "GitHub", "Component", Child, U),
            n("diffm", "DiffModel", "Component", Child, U),
            n("cfg", "Config", "Component", Child, U),
            n("git2", "git2", "Container", External, U),
            n("ghcli", "gh CLI", "Container", External, U),
        ],
        edges: vec![
            e("app", "repo", Intra, U),
            e("app", "gh", Intra, U),
            e("app", "diffm", Intra, U),
            e("app", "cfg", Intra, U),
            e("repo", "git2", Cross, U),
            e("gh", "ghcli", Cross, U),
        ],
    }
}

/// The "after" side: lado once the agent-task subsystem lands.
fn scenario_after() -> FocusInput {
    use DiffState::*;
    use EdgeClass::*;
    use NodeKind::*;
    FocusInput {
        nodes: vec![
            n("app", "App", "Component", Child, Changed),
            n("repo", "Repository", "Component", Child, Unchanged),
            n("gh", "GitHub", "Component", Child, Unchanged),
            n("diffm", "DiffModel", "Component", Child, Unchanged),
            n("cfg", "Config", "Component", Child, Changed),
            n("agent", "AgentRunner", "Component", Child, Added),
            n("store", "ArtifactStore", "Component", Child, Added),
            n("render", "DiagramRenderer", "Component", Child, Added),
            n("git2", "git2", "Container", External, Unchanged),
            n("ghcli", "gh CLI", "Container", External, Unchanged),
            n("claude", "claude CLI", "Container", External, Added),
        ],
        edges: vec![
            e("app", "repo", Intra, Unchanged),
            e("app", "gh", Intra, Unchanged),
            e("app", "diffm", Intra, Unchanged),
            e("app", "cfg", Intra, Changed),
            e("repo", "git2", Cross, Unchanged),
            e("gh", "ghcli", Cross, Unchanged),
            e("app", "agent", Intra, Added),
            e("agent", "store", Intra, Added),
            e("render", "store", Intra, Added),
            e("app", "render", Intra, Added),
            e("agent", "claude", Cross, Added),
        ],
    }
}

/// The combined diff view: after, plus the nodes/edges that were removed.
fn scenario_combined() -> FocusInput {
    let mut input = scenario_after();
    input.nodes.push(n(
        "legacy",
        "LegacyDiffCache",
        "Component",
        NodeKind::Child,
        DiffState::Removed,
    ));
    input
        .edges
        .push(e("app", "legacy", EdgeClass::Intra, DiffState::Removed));
    input
}

fn main() -> std::io::Result<()> {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/c4diff-spike".to_string());
    std::fs::create_dir_all(&out_dir)?;

    let combined = scenario_combined();
    for enc in [
        Encoding::DiffAsFill,
        Encoding::DiffAsStroke,
        Encoding::DiffFillLayerStroke,
        Encoding::DiffAsBadge,
    ] {
        let (svg, _) = render(&combined, enc, &format!("COMBINED DIFF — {}", enc.slug()));
        let p = format!("{out_dir}/{}.svg", enc.slug());
        std::fs::write(&p, svg)?;
        println!("wrote {p}");
    }

    // Layout-stability check: render before and after independently, using the
    // UNMODIFIED layout algorithm, and measure how far shared nodes move.
    let before = scenario_before();
    let after = scenario_after();
    let (svg_b, pos_b) = render(
        &before,
        Encoding::DiffAsStroke,
        "BEFORE (independent render)",
    );
    let (svg_a, pos_a) = render(&after, Encoding::DiffAsStroke, "AFTER (independent render)");
    std::fs::write(format!("{out_dir}/v5-before.svg"), svg_b)?;
    std::fs::write(format!("{out_dir}/v5-after.svg"), svg_a)?;
    println!("wrote {out_dir}/v5-before.svg");
    println!("wrote {out_dir}/v5-after.svg");

    println!("\n=== LAYOUT STABILITY: displacement of nodes present in BOTH renders ===");
    let mut shared: Vec<&String> = pos_b.keys().filter(|k| pos_a.contains_key(*k)).collect();
    shared.sort();
    let mut moved = 0;
    for id in &shared {
        let (bx, by) = pos_b[*id];
        let (ax, ay) = pos_a[*id];
        let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
        if d > 1.0 {
            moved += 1;
        }
        println!(
            "  {id:<8} before=({bx:7.1},{by:7.1})  after=({ax:7.1},{ay:7.1})  moved {d:7.1}px"
        );
    }
    println!(
        "\n  {moved} of {} shared nodes changed position between the two renders.",
        shared.len()
    );
    Ok(())
}
