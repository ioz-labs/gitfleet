use gitfleet_git::CommitSummary;
use iced::widget::canvas::{self, Canvas, Geometry, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme};

pub const ROW_HEIGHT: f32 = 28.0;
const LANE_SPACING: f32 = 14.0;
const LEFT_PAD: f32 = 10.0;

const LANE_COLORS: [Color; 8] = [
    Color::from_rgb(0.224, 0.549, 0.902), // Blue (#398ce6)
    Color::from_rgb(0.255, 0.725, 0.451), // Green (#41b973)
    Color::from_rgb(0.922, 0.678, 0.255), // Amber (#ebad41)
    Color::from_rgb(0.667, 0.478, 0.949), // Violet (#aa7af2)
    Color::from_rgb(0.922, 0.427, 0.455), // Coral (#eb6d74)
    Color::from_rgb(0.220, 0.741, 0.808), // Cyan (#38bdce)
    Color::from_rgb(0.898, 0.486, 0.753), // Pink (#e57cc0)
    Color::from_rgb(0.584, 0.784, 0.349), // Lime (#95c859)
];

pub fn lane_color(idx: usize) -> Color {
    LANE_COLORS[idx % LANE_COLORS.len()]
}

#[derive(Debug, Clone)]
pub struct GraphRow {
    pub node_col: usize,
    pub node_color: usize,
    pub is_merge: bool,
    /// Pass-through vertical lines in the top half: (col, color_idx)
    pub top_lines: Vec<(usize, usize)>,
    /// Pass-through vertical lines in the bottom half: (col, color_idx)
    pub bottom_lines: Vec<(usize, usize)>,
    /// Curved joins entering `node_col` from above: (from_col, to_col, color_idx)
    pub top_curves: Vec<(usize, usize, usize)>,
    /// Curved forks/joins leaving `node_col` going down: (from_col, to_col, color_idx)
    pub bottom_curves: Vec<(usize, usize, usize)>,
}

/// Computes an accurate multi-lane Git commit graph layout from topologically ordered commits.
pub fn compute_graph_layout(commits: &[CommitSummary]) -> (Vec<GraphRow>, usize) {
    // Each lane slot holds `Some((expected_oid, color_idx))`
    let mut lanes: Vec<Option<(String, usize)>> = Vec::new();
    let mut next_color: usize = 0;
    let mut rows = Vec::with_capacity(commits.len());
    let mut max_lanes: usize = 1;

    let alloc_lane = |lanes: &mut Vec<Option<(String, usize)>>,
                      oid: String,
                      color: usize|
     -> usize {
        if let Some(pos) = lanes.iter().position(|slot| slot.is_none()) {
            lanes[pos] = Some((oid, color));
            pos
        } else {
            lanes.push(Some((oid, color)));
            lanes.len() - 1
        }
    };

    for commit in commits {
        // 1. Find all lanes currently expecting this commit's OID
        let matching_cols: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.as_ref()
                    .and_then(|(oid, _)| (oid == &commit.oid).then_some(i))
            })
            .collect();

        let mut top_lines: Vec<(usize, usize)> = Vec::new();
        let mut top_curves: Vec<(usize, usize, usize)> = Vec::new();

        let (node_col, node_color) = if let Some(&first_col) = matching_cols.first() {
            let color = lanes[first_col].as_ref().map(|(_, c)| *c).unwrap_or(0);
            // Record incoming vertical lines for every currently active lane
            for (i, slot) in lanes.iter().enumerate() {
                if let Some((_, c)) = slot {
                    if i == first_col {
                        top_lines.push((i, *c));
                    } else if matching_cols.contains(&i) {
                        // Additional lane converging into this commit from above
                        top_curves.push((i, first_col, *c));
                    } else {
                        top_lines.push((i, *c));
                    }
                }
            }
            // Clear merged-in extra lanes
            for &extra in matching_cols.iter().skip(1) {
                lanes[extra] = None;
            }
            (first_col, color)
        } else {
            // New branch tip — record existing pass-through top lines first
            for (i, slot) in lanes.iter().enumerate() {
                if let Some((_, c)) = slot {
                    top_lines.push((i, *c));
                }
            }
            let c = next_color;
            next_color = next_color.wrapping_add(1);
            let col = alloc_lane(&mut lanes, commit.oid.clone(), c);
            (col, c)
        };

        // Track lanes that were ALREADY active before wiring parents (they pass straight through)
        let mut bottom_lines: Vec<(usize, usize)> = Vec::new();
        let mut bottom_curves: Vec<(usize, usize, usize)> = Vec::new();

        for (i, slot) in lanes.iter().enumerate() {
            if i != node_col
                && let Some((_, c)) = slot
            {
                bottom_lines.push((i, *c));
            }
        }

        // 2. Wire parents for bottom half
        if commit.parents.is_empty() {
            // Root commit: lane terminates here
            lanes[node_col] = None;
        } else {
            // First parent (main lineage)
            let p0 = &commit.parents[0];
            let existing_p0 = lanes.iter().enumerate().find_map(|(i, slot)| {
                (i != node_col)
                    .then(|| slot.as_ref().and_then(|(o, _)| (o == p0).then_some(i)))
                    .flatten()
            });

            if let Some(target_col) = existing_p0 {
                if target_col < node_col {
                    // Side lane converging left into main/leftmost lane: keep target_col straight
                    lanes[node_col] = None;
                    bottom_curves.push((node_col, target_col, node_color));
                } else {
                    // Main/leftmost lane keeps straight down; right-side duplicate lane folds in
                    let target_color = lanes[target_col].as_ref().map(|(_, c)| *c).unwrap_or(node_color);
                    lanes[target_col] = None;
                    lanes[node_col] = Some((p0.clone(), node_color));
                    bottom_lines.retain(|(c, _)| *c != target_col);
                    bottom_lines.push((node_col, node_color));
                    bottom_curves.push((target_col, node_col, target_color));
                }
            } else {
                // Continue straight down on `node_col`
                lanes[node_col] = Some((p0.clone(), node_color));
                bottom_lines.push((node_col, node_color));
            }

            // Secondary parents (merge commits!)
            for parent_oid in commit.parents.iter().skip(1) {
                let existing_p = lanes.iter().enumerate().find_map(|(i, slot)| {
                    slot.as_ref()
                        .and_then(|(o, c)| (o == parent_oid).then_some((i, *c)))
                });

                if let Some((target_col, target_color)) = existing_p {
                    bottom_curves.push((node_col, target_col, target_color));
                } else {
                    let c = next_color;
                    next_color = next_color.wrapping_add(1);
                    let new_col = alloc_lane(&mut lanes, parent_oid.clone(), c);
                    bottom_curves.push((node_col, new_col, c));
                }
            }
        }

        // Trim trailing empty lanes to keep graph compact
        while lanes.last().is_some_and(|s| s.is_none()) {
            lanes.pop();
        }

        let row_width = lanes
            .len()
            .max(node_col + 1)
            .max(
                top_lines
                    .iter()
                    .map(|(c, _)| *c + 1)
                    .max()
                    .unwrap_or(1),
            )
            .max(
                bottom_curves
                    .iter()
                    .map(|(a, b, _)| (*a).max(*b) + 1)
                    .max()
                    .unwrap_or(1),
            );
        if row_width > max_lanes {
            max_lanes = row_width;
        }

        rows.push(GraphRow {
            node_col,
            node_color,
            is_merge: commit.parents.len() > 1,
            top_lines,
            bottom_lines,
            top_curves,
            bottom_curves,
        });
    }

    (rows, max_lanes.min(12))
}

pub fn graph_canvas_width(max_lanes: usize) -> f32 {
    LEFT_PAD * 2.0 + (max_lanes.saturating_sub(1) as f32) * LANE_SPACING + 4.0
}

struct GraphCellProgram {
    row: GraphRow,
    bg_color: Color,
}

impl<Message> canvas::Program<Message> for GraphCellProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let cy = bounds.height / 2.0;
        let lane_x = |col: usize| LEFT_PAD + (col as f32) * LANE_SPACING;

        // 1. Straight vertical lines (top half & bottom half)
        for &(col, color_idx) in &self.row.top_lines {
            let x = lane_x(col);
            let path = Path::line(Point::new(x, 0.0), Point::new(x, cy));
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(lane_color(color_idx))
                    .with_width(2.0),
            );
        }
        for &(col, color_idx) in &self.row.bottom_lines {
            let x = lane_x(col);
            let path = Path::line(Point::new(x, cy), Point::new(x, bounds.height));
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(lane_color(color_idx))
                    .with_width(2.0),
            );
        }

        // 2. Smooth Bézier curves entering from above
        for &(from_col, to_col, color_idx) in &self.row.top_curves {
            let x1 = lane_x(from_col);
            let x2 = lane_x(to_col);
            let path = Path::new(|b| {
                b.move_to(Point::new(x1, 0.0));
                b.bezier_curve_to(
                    Point::new(x1, cy * 0.75),
                    Point::new(x2, cy * 0.25),
                    Point::new(x2, cy),
                );
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(lane_color(color_idx))
                    .with_width(2.0),
            );
        }

        // 3. Smooth Bézier curves leaving downward (merge forks & branch joins)
        for &(from_col, to_col, color_idx) in &self.row.bottom_curves {
            let x1 = lane_x(from_col);
            let x2 = lane_x(to_col);
            let path = Path::new(|b| {
                b.move_to(Point::new(x1, cy));
                b.bezier_curve_to(
                    Point::new(x1, cy + (bounds.height - cy) * 0.25),
                    Point::new(x2, cy + (bounds.height - cy) * 0.75),
                    Point::new(x2, bounds.height),
                );
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(lane_color(color_idx))
                    .with_width(2.0),
            );
        }

        // 4. Commit node circle (hollow ring for merge commits, solid dot for normal commits)
        let nx = lane_x(self.row.node_col);
        let center = Point::new(nx, cy);
        let col = lane_color(self.row.node_color);

        if self.row.is_merge {
            let outer = Path::circle(center, 4.4);
            frame.fill(&outer, self.bg_color);
            frame.stroke(&outer, Stroke::default().with_color(col).with_width(2.0));
        } else {
            let dot = Path::circle(center, 3.7);
            frame.fill(&dot, col);
        }

        vec![frame.into_geometry()]
    }
}

pub fn graph_cell<'a, M: 'a>(row: GraphRow, max_lanes: usize, bg_color: Color) -> Element<'a, M> {
    Canvas::new(GraphCellProgram { row, bg_color })
        .width(Length::Fixed(graph_canvas_width(max_lanes)))
        .height(Length::Fixed(ROW_HEIGHT))
        .into()
}

struct WorkingTreeCellProgram {
    wt_col: usize,
    head_col: usize,
    pass_through: Vec<(usize, usize)>,
    dot_color: Color,
    line_color: Color,
    bg_color: Color,
    has_commits: bool,
}

impl<Message> canvas::Program<Message> for WorkingTreeCellProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let cy = bounds.height / 2.0;
        let lane_x = |col: usize| LEFT_PAD + (col as f32) * LANE_SPACING;

        // 1. Continuous pass-through rails (e.g. incoming origin/develop commits above HEAD)
        for &(col, color_idx) in &self.pass_through {
            let px = lane_x(col);
            let path = Path::line(Point::new(px, 0.0), Point::new(px, bounds.height));
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(lane_color(color_idx))
                    .with_width(2.0),
            );
        }

        let x = lane_x(self.wt_col);
        let hx = lane_x(self.head_col);

        if self.has_commits {
            if self.wt_col == self.head_col {
                let path = Path::line(Point::new(x, cy + 4.0), Point::new(x, bounds.height));
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(self.line_color)
                        .with_width(1.8),
                );
            } else {
                let path = Path::new(|b| {
                    b.move_to(Point::new(x, cy + 3.5));
                    b.bezier_curve_to(
                        Point::new(x, cy + (bounds.height - cy) * 0.55),
                        Point::new(hx, cy + (bounds.height - cy) * 0.45),
                        Point::new(hx, bounds.height),
                    );
                });
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(self.dot_color)
                        .with_width(1.6),
                );
            }
        }

        let center = Point::new(x, cy);
        let outer = Path::circle(center, 4.6);
        frame.fill(&outer, self.bg_color);
        let dot = Path::circle(center, 3.8);
        frame.fill(&dot, self.dot_color);

        vec![frame.into_geometry()]
    }
}

#[allow(clippy::too_many_arguments)]
pub fn working_tree_cell<'a, M: 'a>(
    max_lanes: usize,
    wt_col: usize,
    head_col: usize,
    pass_through: Vec<(usize, usize)>,
    dot_color: Color,
    line_color: Color,
    bg_color: Color,
    has_commits: bool,
) -> Element<'a, M> {
    Canvas::new(WorkingTreeCellProgram {
        wt_col,
        head_col,
        pass_through,
        dot_color,
        line_color,
        bg_color,
        has_commits,
    })
    .width(Length::Fixed(graph_canvas_width(max_lanes)))
    .height(Length::Fixed(ROW_HEIGHT))
    .into()
}
