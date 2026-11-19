use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use serde_json::json;
use std::fs::File;
use std::io::Write;
use uuid::Uuid;

// ==========================================
// Tipos de Gráfico y Direcciones
// ==========================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChartType {
    IdeaMap,
    LogicChart,
    TreeChart,
    Timeline,
}

impl ChartType {
    fn label(self) -> &'static str {
        match self {
            ChartType::IdeaMap => "Mapa de ideas",
            ChartType::LogicChart => "Gráfico lógico",
            ChartType::TreeChart => "Gráfico de árbol",
            ChartType::Timeline => "Línea temporal",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Right,
    Left,
    Down,
    Up,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Direction::Right => "Derecha",
            Direction::Left => "Izquierda",
            Direction::Down => "Abajo",
            Direction::Up => "Arriba",
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Direction::Right | Direction::Left)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeDirectionOverride {
    Auto,
    Right,
    Left,
    Down,
    Up,
}

impl NodeDirectionOverride {
    fn label(self) -> &'static str {
        match self {
            NodeDirectionOverride::Auto => "Auto (global)",
            NodeDirectionOverride::Right => "Derecha",
            NodeDirectionOverride::Left => "Izquierda",
            NodeDirectionOverride::Down => "Abajo",
            NodeDirectionOverride::Up => "Arriba",
        }
    }

    fn resolve(self, fallback: Direction) -> Direction {
        match self {
            NodeDirectionOverride::Auto => fallback,
            NodeDirectionOverride::Right => Direction::Right,
            NodeDirectionOverride::Left => Direction::Left,
            NodeDirectionOverride::Down => Direction::Down,
            NodeDirectionOverride::Up => Direction::Up,
        }
    }
}

// ==========================================
// 1. Parser de Markdown a Árbol
// ==========================================
#[derive(Clone)]
struct TreeNode {
    title: String,
    children: Vec<TreeNode>,
    collapsed: bool,
    dir_override: NodeDirectionOverride,
    pos: Pos2,
    size: Vec2,
    toggle_rect: Rect,
}

impl TreeNode {
    fn new(title: String) -> Self {
        Self {
            title,
            children: vec![],
            collapsed: false,
            dir_override: NodeDirectionOverride::Auto,
            pos: Pos2::ZERO,
            size: Vec2::ZERO,
            toggle_rect: Rect::NOTHING,
        }
    }
}

fn strip_md(text: &str) -> String {
    text.replace("**", "")
    .replace("__", "")
    .replace("*", "")
    .replace("_", "")
}

fn parse_markdown(md: &str) -> TreeNode {
    let mut root = TreeNode::new("Mapa mental".to_string());
    let mut heading_stack: Vec<(usize, Vec<usize>)> = vec![(0, vec![])];
    let mut list_chains: std::collections::HashMap<usize, Vec<(usize, Vec<usize>)>> =
    std::collections::HashMap::new();
    let mut lines = md.lines().peekable();
    let mut found_h1 = false;

    if let Some(first_line) = lines.peek() {
        if first_line.starts_with("# ") {
            root.title = first_line[2..].trim().to_string();
            lines.next();
            found_h1 = true;
        }
    }

    for line in lines {
        if line.trim().is_empty() {
            list_chains.clear();
            continue;
        }

        if line.starts_with('#') {
            let level = line.chars().take_while(|&c| c == '#').count();
            let title = line[level..].trim().to_string();
            if title.is_empty() {
                continue;
            }

            let adjusted_level = if found_h1 { level + 1 } else { level };
            let node = TreeNode::new(title);

            while let Some((lvl, _)) = heading_stack.last() {
                if *lvl >= adjusted_level {
                    heading_stack.pop();
                } else {
                    break;
                }
            }

            let parent_path = &heading_stack.last().unwrap().1;
            let parent = get_node_mut(&mut root, parent_path);
            parent.children.push(node);

            let mut new_path = parent_path.clone();
            new_path.push(parent.children.len() - 1);
            heading_stack.push((adjusted_level, new_path));
            list_chains.clear();
            continue;
        }

        let trimmed = line.trim_start();
        let is_list = trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || (trimmed.chars().next().unwrap_or(' ').is_ascii_digit() && trimmed.contains(". "));

        if is_list {
            let indent = line.len() - trimmed.len();
            let title_start = if trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.starts_with('+')
            {
                2
            } else {
                trimmed.find(". ").unwrap_or(0) + 2
            };
            let title = trimmed[title_start..].trim().to_string();
            if title.is_empty() {
                continue;
            }

            let ctx = heading_stack.last().unwrap().0;
            let chain = list_chains.entry(ctx).or_insert_with(Vec::new);

            while let Some((ind, _)) = chain.last() {
                if *ind >= indent {
                    chain.pop();
                } else {
                    break;
                }
            }

            let parent_path = if let Some((_, path)) = chain.last() {
                path.clone()
            } else {
                heading_stack.last().unwrap().1.clone()
            };

            let parent = get_node_mut(&mut root, &parent_path);
            parent.children.push(TreeNode::new(title));

            let mut new_path = parent_path.clone();
            new_path.push(parent.children.len() - 1);
            chain.push((indent, new_path));
        }
    }
    root
}

fn tree_to_markdown(root: &TreeNode) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", root.title));
    for branch in &root.children {
        out.push_str(&format!("## {}\n", branch.title));
        write_list_children(&branch.children, 0, &mut out);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn write_list_children(children: &[TreeNode], indent_spaces: usize, out: &mut String) {
    let indent = "  ".repeat(indent_spaces);
    for child in children {
        out.push_str(&format!("{}- {}\n", indent, child.title));
        if !child.children.is_empty() {
            write_list_children(&child.children, indent_spaces + 1, out);
        }
    }
}

fn get_node_mut<'a>(node: &'a mut TreeNode, path: &[usize]) -> &'a mut TreeNode {
    if path.is_empty() {
        return node;
    }
    get_node_mut(&mut node.children[path[0]], &path[1..])
}

fn get_node<'a>(node: &'a TreeNode, path: &[usize]) -> Option<&'a TreeNode> {
    if path.is_empty() {
        return Some(node);
    }
    node.children.get(path[0]).and_then(|c| get_node(c, &path[1..]))
}

// ==========================================
// 2. Exportador a .xmind
// ==========================================
fn to_xmind_json(node: &TreeNode) -> serde_json::Value {
    let mut topic = json!({
        "id": Uuid::new_v4().to_string(),
                          "class": "topic",
                          "title": strip_md(&node.title)
    });

    if !node.children.is_empty() {
        let children: Vec<_> = node.children.iter().map(to_xmind_json).collect();
        topic["children"] = json!({ "attached": children });
    }
    topic
}

fn export_xmind(root: &TreeNode, path: &str) -> Result<(), String> {
    let mut root_json = to_xmind_json(root);
    root_json["structureClass"] = json!("org.xmind.ui.map.unbalanced");

    let sheet_id = Uuid::new_v4().to_string();
    let content = json!([{
        "id": sheet_id,
        "class": "sheet",
        "title": "Sheet 1",
        "rootTopic": root_json
    }]);

    let metadata = json!({
        "creator": { "name": "MindCraft Rust", "version": "1.0" },
        "activeSheetId": sheet_id
    });

    let manifest = json!({
        "file-entries": {
            "content.json": {},
            "metadata.json": {},
            "manifest.json": {}
        }
    });

    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
    zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("content.json", options)
    .map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&content).unwrap().as_bytes())
    .map_err(|e| e.to_string())?;

    zip.start_file("metadata.json", options)
    .map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&metadata).unwrap().as_bytes())
    .map_err(|e| e.to_string())?;

    zip.start_file("manifest.json", options)
    .map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
    .map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

// ==========================================
// 3. Lógica de Layout y Renderizado
// ==========================================
struct MindMapRenderer {
    zoom: f32,
    pan: Vec2,
    chart_type: ChartType,
    logic_direction: Direction,
}

impl MindMapRenderer {
    fn to_screen(&self, pos: Pos2, offset: Vec2) -> Pos2 {
        (offset + pos.to_vec2() * self.zoom).to_pos2()
    }

    fn measure_tree(&self, ui: &egui::Ui, node: &mut TreeNode, depth: usize) {
        let base_size = (16.0 - (depth as f32 * 1.5)).max(11.0);
        let font_id = FontId::proportional(base_size);
        let clean_title = strip_md(&node.title);

        let galley = ui.fonts(|f| f.layout(clean_title, font_id, Color32::BLACK, 260.0));
        let toggle_extra = if !node.children.is_empty() { 20.0 } else { 0.0 };

        node.size = Vec2::new(
            galley.size().x + 28.0 + toggle_extra,
                              (galley.size().y + 18.0).max(24.0),
        );

        for child in &mut node.children {
            self.measure_tree(ui, child, depth + 1);
        }
    }

    fn layout_tree(&self, node: &mut TreeNode) {
        node.pos = Pos2::ZERO;
        if node.children.is_empty() || node.collapsed {
            return;
        }

        match self.chart_type {
            ChartType::IdeaMap => self.layout_idea_map(node),
            ChartType::LogicChart => {
                self.layout_directed_children(node, self.logic_direction);
            }
            ChartType::TreeChart => {
                self.layout_directed_children(node, Direction::Down);
            }
            ChartType::Timeline => self.layout_timeline(node),
        }
    }

    /// Mapa de ideas (balanceado izquierda / derecha)
    fn layout_idea_map(&self, node: &mut TreeNode) {
        let v_gap = 20.0;
        let h_gap = 70.0;

        let mid = (node.children.len() + 1) / 2;
        let (right_kids, left_kids) = node.children.split_at_mut(mid);

        // Derecha
        let mut right_h = 0.0;
        let mut right_heights = Vec::with_capacity(right_kids.len());
        for child in right_kids.iter() {
            let h = Self::span_along(child, Direction::Right, v_gap, Direction::Right);
            right_heights.push(h);
            right_h += h;
        }
        if !right_kids.is_empty() {
            right_h += v_gap * (right_kids.len() as f32 - 1.0);
        }
        let mut current_y = -right_h / 2.0;
        for (child, &ch_h) in right_kids.iter_mut().zip(right_heights.iter()) {
            Self::place_directed(
                child,
                node.pos.x + node.size.x / 2.0 + h_gap,
                current_y,
                ch_h,
                Direction::Right,
                h_gap,
                v_gap,
            );
            current_y += ch_h + v_gap;
        }

        // Izquierda
        let mut left_h = 0.0;
        let mut left_heights = Vec::with_capacity(left_kids.len());
        for child in left_kids.iter() {
            let h = Self::span_along(child, Direction::Left, v_gap, Direction::Left);
            left_heights.push(h);
            left_h += h;
        }
        if !left_kids.is_empty() {
            left_h += v_gap * (left_kids.len() as f32 - 1.0);
        }
        let mut current_y = -left_h / 2.0;
        for (child, &ch_h) in left_kids.iter_mut().zip(left_heights.iter()) {
            Self::place_directed(
                child,
                node.pos.x - node.size.x / 2.0 - h_gap,
                current_y,
                ch_h,
                Direction::Left,
                h_gap,
                v_gap,
            );
            current_y += ch_h + v_gap;
        }
    }

    /// Gráfico lógico (con dirección global y por nodo) y Gráfico de árbol
    fn layout_directed_children(&self, node: &mut TreeNode, root_dir: Direction) {
        let gap_main = 70.0;
        let gap_cross = 22.0;

        let mut spans = Vec::with_capacity(node.children.len());
        let mut total_span = 0.0;
        for child in &node.children {
            let dir = child.dir_override.resolve(root_dir);
            let s = Self::span_along(child, dir, gap_cross, root_dir);
            spans.push((dir, s));
            total_span += s;
        }
        if !node.children.is_empty() {
            total_span += gap_cross * (node.children.len() as f32 - 1.0);
        }

        let mut cursor = -total_span / 2.0;
        for (child, (dir, span)) in node.children.iter_mut().zip(spans.into_iter()) {
            let edge_start = match dir {
                Direction::Right => node.pos.x + node.size.x / 2.0 + gap_main,
                Direction::Left => node.pos.x - node.size.x / 2.0 - gap_main,
                Direction::Down => node.pos.y + node.size.y / 2.0 + gap_main,
                Direction::Up => node.pos.y - node.size.y / 2.0 - gap_main,
            };
            Self::place_directed(child, edge_start, cursor, span, dir, gap_main, gap_cross);
            cursor += span + gap_cross;
        }
    }

    /// Calcula el ancho/alto perpendicular a `dir` que ocupa un subárbol
    fn span_along(node: &TreeNode, dir: Direction, gap_cross: f32, fallback_dir: Direction) -> f32 {
        let own_cross = if dir.is_horizontal() {
            node.size.y
        } else {
            node.size.x
        };

        if node.children.is_empty() || node.collapsed {
            return own_cross;
        }

        let children_cross: f32 = node
        .children
        .iter()
        .map(|c| {
            let c_dir = c.dir_override.resolve(dir.min_fallback(fallback_dir));
            Self::span_along(c, c_dir, gap_cross, c_dir)
        })
        .sum();
        let gaps = gap_cross * (node.children.len().saturating_sub(1) as f32);
        (children_cross + gaps).max(own_cross)
    }

    /// Coloca un nodo y sus descendientes en `dir`
    /// Para Right/Left: `edge_coord` es el borde X más cercano al padre, `cross_start` es Y inicial.
    /// Para Down/Up: `edge_coord` es el borde Y más cercano al padre, `cross_start` es X inicial.
    fn place_directed(
        node: &mut TreeNode,
        edge_coord: f32,
        cross_start: f32,
        span: f32,
        dir: Direction,
        gap_main: f32,
        gap_cross: f32,
    ) {
        let cross_center = cross_start + span / 2.0;

        match dir {
            Direction::Right => {
                node.pos = Pos2::new(edge_coord + node.size.x / 2.0, cross_center);
            }
            Direction::Left => {
                node.pos = Pos2::new(edge_coord - node.size.x / 2.0, cross_center);
            }
            Direction::Down => {
                node.pos = Pos2::new(cross_center, edge_coord + node.size.y / 2.0);
            }
            Direction::Up => {
                node.pos = Pos2::new(cross_center, edge_coord - node.size.y / 2.0);
            }
        }

        if node.children.is_empty() || node.collapsed {
            return;
        }

        let mut child_spans = Vec::with_capacity(node.children.len());
        let mut total_child_span = 0.0;
        for child in &node.children {
            let c_dir = child.dir_override.resolve(dir);
            let s = Self::span_along(child, c_dir, gap_cross, dir);
            child_spans.push((c_dir, s));
            total_child_span += s;
        }
        if !node.children.is_empty() {
            total_child_span += gap_cross * (node.children.len().saturating_sub(1) as f32);
        }

        let mut child_cursor = cross_center - total_child_span / 2.0;
        for (child, (c_dir, c_span)) in node.children.iter_mut().zip(child_spans.into_iter()) {
            let next_edge = match c_dir {
                Direction::Right => node.pos.x + node.size.x / 2.0 + gap_main,
                Direction::Left => node.pos.x - node.size.x / 2.0 - gap_main,
                Direction::Down => node.pos.y + node.size.y / 2.0 + gap_main,
                Direction::Up => node.pos.y - node.size.y / 2.0 - gap_main,
            };
            Self::place_directed(
                child,
                next_edge,
                child_cursor,
                c_span,
                c_dir,
                gap_main,
                gap_cross,
            );
            child_cursor += c_span + gap_cross;
        }
    }

    /// Línea temporal horizontal: hitos principales sobre un eje X con sub-ramas arriba/abajo
    fn layout_timeline(&self, root: &mut TreeNode) {
        let step_x = 90.0;
        let branch_gap_y = 55.0;
        let sub_gap_y = 16.0;
        let sub_gap_x = 50.0;

        root.pos = Pos2::new(0.0, 0.0);
        let mut cursor_x = root.size.x / 2.0 + step_x;

        for (i, milestone) in root.children.iter_mut().enumerate() {
            let above = i % 2 == 0;
            milestone.pos = Pos2::new(cursor_x + milestone.size.x / 2.0, 0.0);

            if !milestone.children.is_empty() && !milestone.collapsed {
                let mut y_acc = if above {
                    -milestone.size.y / 2.0 - branch_gap_y
                } else {
                    milestone.size.y / 2.0 + branch_gap_y
                };

                for sub in &mut milestone.children {
                    let sub_h = Self::timeline_sub_height(sub, sub_gap_y);
                    let sub_center_y = if above {
                        y_acc - sub_h / 2.0
                    } else {
                        y_acc + sub_h / 2.0
                    };
                    let sub_x = milestone.pos.x + milestone.size.x / 2.0 + sub_gap_x + sub.size.x / 2.0;
                    sub.pos = Pos2::new(sub_x, sub_center_y);

                    Self::place_directed(
                        sub,
                        sub.pos.x + sub.size.x / 2.0 + sub_gap_x,
                        sub_center_y - sub_h / 2.0,
                        sub_h,
                        Direction::Right,
                        sub_gap_x,
                        sub_gap_y,
                    );

                    if above {
                        y_acc -= sub_h + sub_gap_y;
                    } else {
                        y_acc += sub_h + sub_gap_y;
                    }
                }
            }

            let max_child_right = Self::max_right_extent(milestone).max(milestone.pos.x + milestone.size.x / 2.0);
            cursor_x = max_child_right + step_x;
        }
    }

    fn timeline_sub_height(node: &TreeNode, v_gap: f32) -> f32 {
        if node.children.is_empty() || node.collapsed {
            return node.size.y;
        }
        let sum: f32 = node
        .children
        .iter()
        .map(|c| Self::timeline_sub_height(c, v_gap))
        .sum();
        let gaps = v_gap * (node.children.len().saturating_sub(1) as f32);
        (sum + gaps).max(node.size.y)
    }

    fn max_right_extent(node: &TreeNode) -> f32 {
        let mut max_x = node.pos.x + node.size.x / 2.0;
        if !node.collapsed {
            for c in &node.children {
                max_x = max_x.max(Self::max_right_extent(c));
            }
        }
        max_x
    }

    /// Pasada 3A: Dibuja conexiones según el tipo de gráfico
    fn draw_edges(&self, painter: &egui::Painter, node: &TreeNode, offset: Vec2, is_root: bool) {
        if node.children.is_empty() || node.collapsed {
            return;
        }

        if self.chart_type == ChartType::Timeline && is_root {
            let root_right = self.to_screen(
                Pos2::new(node.pos.x + node.size.x / 2.0, node.pos.y),
                                            offset,
            );
            let last_x = node
            .children
            .last()
            .map(|m| m.pos.x + m.size.x / 2.0 + 30.0)
            .unwrap_or(node.pos.x + node.size.x / 2.0);
            let axis_end = self.to_screen(Pos2::new(last_x, node.pos.y), offset);

            painter.line_segment(
                [root_right, axis_end],
                Stroke::new(3.0 * self.zoom, Color32::from_rgb(90, 130, 200)),
            );

            for (i, milestone) in node.children.iter().enumerate() {
                let m_center = self.to_screen(milestone.pos, offset);
                painter.circle_filled(m_center, 5.0 * self.zoom, Color32::from_rgb(51, 89, 166));
                let above = i % 2 == 0;
                if !milestone.children.is_empty() && !milestone.collapsed {
                    for sub in &milestone.children {
                        let start = if above {
                            Pos2::new(m_center.x, m_center.y - milestone.size.y * self.zoom / 2.0)
                        } else {
                            Pos2::new(m_center.x, m_center.y + milestone.size.y * self.zoom / 2.0)
                        };
                        let sub_center = self.to_screen(sub.pos, offset);
                        let end = Pos2::new(sub_center.x - sub.size.x * self.zoom / 2.0, sub_center.y);
                        self.draw_bezier_pair(painter, start, end, true);
                        self.draw_edges(painter, sub, offset, false);
                    }
                }
            }
            return;
        }

        let n_center = self.to_screen(node.pos, offset);
        let n_w = node.size.x * self.zoom;
        let n_h = node.size.y * self.zoom;

        for child in &node.children {
            let c_center = self.to_screen(child.pos, offset);
            let c_w = child.size.x * self.zoom;
            let c_h = child.size.y * self.zoom;

            let dx = child.pos.x - node.pos.x;
            let dy = child.pos.y - node.pos.y;

            let (start, end, horizontal) = if dx.abs() >= dy.abs() {
                if dx >= 0.0 {
                    (
                        Pos2::new(n_center.x + n_w / 2.0, n_center.y),
                     Pos2::new(c_center.x - c_w / 2.0, c_center.y),
                     true,
                    )
                } else {
                    (
                        Pos2::new(n_center.x - n_w / 2.0, n_center.y),
                     Pos2::new(c_center.x + c_w / 2.0, c_center.y),
                     true,
                    )
                }
            } else if dy >= 0.0 {
                (
                    Pos2::new(n_center.x, n_center.y + n_h / 2.0),
                 Pos2::new(c_center.x, c_center.y - c_h / 2.0),
                 false,
                )
            } else {
                (
                    Pos2::new(n_center.x, n_center.y - n_h / 2.0),
                 Pos2::new(c_center.x, c_center.y + c_h / 2.0),
                 false,
                )
            };

            self.draw_bezier_pair(painter, start, end, horizontal);
            self.draw_edges(painter, child, offset, false);
        }
    }

    fn draw_bezier_pair(&self, painter: &egui::Painter, start: Pos2, end: Pos2, horizontal: bool) {
        let (c1, c2) = if horizontal {
            let ctrl = (end.x - start.x) * 0.5;
            (
                start + Vec2::new(ctrl, 0.0),
             end - Vec2::new(ctrl, 0.0),
            )
        } else {
            let ctrl = (end.y - start.y) * 0.5;
            (
                start + Vec2::new(0.0, ctrl),
             end - Vec2::new(0.0, ctrl),
            )
        };

        let bezier = egui::epaint::CubicBezierShape::from_points_stroke(
            [start, c1, c2, end],
            false,
            Color32::TRANSPARENT,
            Stroke::new(2.0 * self.zoom, Color32::from_gray(130)),
        );
        painter.add(bezier);
    }

    fn draw_nodes(
        &self,
        painter: &egui::Painter,
        node: &mut TreeNode,
        offset: Vec2,
        depth: usize,
        selected_path: &[usize],
        current_path: &mut Vec<usize>,
    ) {
        let center = self.to_screen(node.pos, offset);
        let scaled_size = node.size * self.zoom;
        let rect = Rect::from_center_size(center, scaled_size);
        let is_selected = current_path.as_slice() == selected_path;

        let (bg, text_color) = match depth {
            0 => (Color32::from_rgb(51, 89, 166), Color32::WHITE),
            1 => (Color32::from_rgb(191, 217, 255), Color32::BLACK),
            _ => (Color32::from_rgb(242, 242, 250), Color32::BLACK),
        };

        let rounding = 8.0 * self.zoom;
        painter.rect_filled(rect, rounding, bg);

        let stroke = if is_selected {
            Stroke::new(2.5 * self.zoom, Color32::from_rgb(255, 180, 0))
        } else {
            Stroke::new(1.0 * self.zoom, Color32::GRAY)
        };
        painter.rect_stroke(rect, rounding, stroke);

        let has_children = !node.children.is_empty();
        let btn_size = 14.0 * self.zoom;

        let text_center = if has_children {
            node.toggle_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 12.0 * self.zoom, rect.center().y),
                                                      Vec2::splat(btn_size),
            );
            painter.rect_filled(node.toggle_rect, 3.0 * self.zoom, Color32::from_rgb(230, 235, 245));
            painter.rect_stroke(
                node.toggle_rect,
                3.0 * self.zoom,
                Stroke::new(1.0 * self.zoom, Color32::from_gray(90)),
            );

            let symbol = if node.collapsed { "+" } else { "-" };
            painter.text(
                node.toggle_rect.center(),
                         Align2::CENTER_CENTER,
                         symbol,
                         FontId::proportional(11.0 * self.zoom),
                         Color32::BLACK,
            );
            Pos2::new(rect.center().x - 8.0 * self.zoom, rect.center().y)
        } else {
            node.toggle_rect = Rect::NOTHING;
            center
        };

        let base_size = (16.0 - (depth as f32 * 1.5)).max(11.0);
        let clean_title = strip_md(&node.title);

        painter.text(
            text_center,
            Align2::CENTER_CENTER,
            clean_title,
            FontId::proportional(base_size * self.zoom),
                     text_color,
        );

        if has_children && !node.collapsed {
            for (idx, child) in node.children.iter_mut().enumerate() {
                current_path.push(idx);
                self.draw_nodes(painter, child, offset, depth + 1, selected_path, current_path);
                current_path.pop();
            }
        }
    }

    fn hit_test(
        node: &mut TreeNode,
        offset: Vec2,
        zoom: f32,
        pointer_pos: Pos2,
        current_path: &mut Vec<usize>,
    ) -> Option<(Vec<usize>, bool)> {
        if !node.children.is_empty() && node.toggle_rect.contains(pointer_pos) {
            return Some((current_path.clone(), true));
        }

        let screen_pos = (offset + node.pos.to_vec2() * zoom).to_pos2();
        let screen_size = node.size * zoom;
        let rect = Rect::from_center_size(screen_pos, screen_size);

        if rect.contains(pointer_pos) {
            return Some((current_path.clone(), false));
        }

        if !node.children.is_empty() && !node.collapsed {
            for (idx, child) in node.children.iter_mut().enumerate() {
                current_path.push(idx);
                if let Some(hit) = Self::hit_test(child, offset, zoom, pointer_pos, current_path) {
                    return Some(hit);
                }
                current_path.pop();
            }
        }
        None
    }
}

trait DirectionHelper {
    fn min_fallback(self, fallback: Direction) -> Direction;
}

impl DirectionHelper for Direction {
    fn min_fallback(self, fallback: Direction) -> Direction {
        if self == fallback {
            self
        } else {
            self
        }
    }
}

// ==========================================
// 4. Aplicación Principal (eframe)
// ==========================================
struct MindMapApp {
    md_text: String,
    root_tree: TreeNode,
    renderer: MindMapRenderer,
    selected_path: Vec<usize>,
    edit_buffer: String,
    editing_node: bool,
}

impl Default for MindMapApp {
    fn default() -> Self {
        let default_md = "# Mi Proyecto\n\n## Objetivos\n- Investigación\n- **Desarrollo**\n  - Frontend\n  - Backend\n- Pruebas\n\n## Cronograma\n- Q1: Diseño\n- Q2: Implementación\n- Q3: Lanzamiento".to_string();
        let root_tree = parse_markdown(&default_md);
        let edit_buffer = root_tree.title.clone();
        Self {
            md_text: default_md,
            root_tree,
            renderer: MindMapRenderer {
                zoom: 1.0,
                pan: Vec2::ZERO,
                chart_type: ChartType::IdeaMap,
                logic_direction: Direction::Right,
            },
            selected_path: vec![],
            edit_buffer,
            editing_node: false,
        }
    }
}

impl MindMapApp {
    fn sync_edit_buffer(&mut self) {
        if let Some(node) = get_node(&self.root_tree, &self.selected_path) {
            self.edit_buffer = node.title.clone();
        }
    }

    fn apply_edit_buffer(&mut self) {
        let trimmed = self.edit_buffer.trim().to_string();
        if !trimmed.is_empty() {
            if let Some(node) = Self::get_node_mut_safe(&mut self.root_tree, &self.selected_path) {
                node.title = trimmed;
                self.md_text = tree_to_markdown(&self.root_tree);
            }
        }
        self.editing_node = false;
    }

    fn add_child_to_selected(&mut self) {
        if let Some(parent) = Self::get_node_mut_safe(&mut self.root_tree, &self.selected_path) {
            parent.collapsed = false;
            parent.children.push(TreeNode::new("Nuevo tema".to_string()));
            let new_idx = parent.children.len() - 1;
            let mut new_path = self.selected_path.clone();
            new_path.push(new_idx);
            self.selected_path = new_path;
            self.sync_edit_buffer();
            self.editing_node = true;
            self.md_text = tree_to_markdown(&self.root_tree);
        }
    }

    fn add_sibling_to_selected(&mut self) {
        if self.selected_path.is_empty() {
            self.add_child_to_selected();
            return;
        }
        let parent_path = &self.selected_path[..self.selected_path.len() - 1];
        let insert_idx = *self.selected_path.last().unwrap() + 1;

        if let Some(parent) = Self::get_node_mut_safe(&mut self.root_tree, parent_path) {
            let clamped_idx = insert_idx.min(parent.children.len());
            parent
            .children
            .insert(clamped_idx, TreeNode::new("Nuevo tema".to_string()));
            let mut new_path = parent_path.to_vec();
            new_path.push(clamped_idx);
            self.selected_path = new_path;
            self.sync_edit_buffer();
            self.editing_node = true;
            self.md_text = tree_to_markdown(&self.root_tree);
        }
    }

    fn get_node_mut_safe<'a>(node: &'a mut TreeNode, path: &[usize]) -> Option<&'a mut TreeNode> {
        if path.is_empty() {
            return Some(node);
        }
        node.children
        .get_mut(path[0])
        .and_then(|c| Self::get_node_mut_safe(c, &path[1..]))
    }
}

impl eframe::App for MindMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Abrir .md").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Markdown", &["md"])
                        .pick_file()
                        {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                self.md_text = content;
                                self.root_tree = parse_markdown(&self.md_text);
                                self.selected_path.clear();
                                self.sync_edit_buffer();
                            }
                        }
                }
                if ui.button("Exportar .xmind").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("XMind", &["xmind"])
                        .save_file()
                        {
                            let mut path_str = path.to_string_lossy().to_string();
                            if !path_str.ends_with(".xmind") {
                                path_str.push_str(".xmind");
                            }
                            if let Err(e) = export_xmind(&self.root_tree, &path_str) {
                                eprintln!("Error exportando: {}", e);
                            } else {
                                println!("Exportado correctamente a {}", path_str);
                            }
                        }
                }
                if ui.button("Centrar vista").clicked() {
                    self.renderer.zoom = 1.0;
                    self.renderer.pan = Vec2::ZERO;
                }

                ui.separator();

                ui.label("Gráfico:");
                egui::ComboBox::from_id_source("chart_type_combo")
                .selected_text(self.renderer.chart_type.label())
                .show_ui(ui, |ui| {
                    for ct in [
                        ChartType::IdeaMap,
                        ChartType::LogicChart,
                        ChartType::TreeChart,
                        ChartType::Timeline,
                    ] {
                        ui.selectable_value(&mut self.renderer.chart_type, ct, ct.label());
                    }
                });

                if self.renderer.chart_type == ChartType::LogicChart {
                    ui.label("Dirección:");
                    egui::ComboBox::from_id_source("logic_dir_combo")
                    .selected_text(self.renderer.logic_direction.label())
                    .show_ui(ui, |ui| {
                        for dir in [
                            Direction::Right,
                            Direction::Left,
                            Direction::Down,
                            Direction::Up,
                        ] {
                            ui.selectable_value(
                                &mut self.renderer.logic_direction,
                                dir,
                                dir.label(),
                            );
                        }
                    });

                    if let Some(node) = Self::get_node_mut_safe(&mut self.root_tree, &self.selected_path)
                    {
                        ui.label("Dir. nodo:");
                        egui::ComboBox::from_id_source("node_dir_combo")
                        .selected_text(node.dir_override.label())
                        .show_ui(ui, |ui| {
                            for ov in [
                                NodeDirectionOverride::Auto,
                                NodeDirectionOverride::Right,
                                NodeDirectionOverride::Left,
                                NodeDirectionOverride::Down,
                                NodeDirectionOverride::Up,
                            ] {
                                ui.selectable_value(
                                    &mut node.dir_override,
                                    ov,
                                    ov.label(),
                                );
                            }
                        });
                    }
                }

                ui.separator();

                if ui.button("+ Hijo").clicked() {
                    self.add_child_to_selected();
                }
                if ui.button("+ Hermano").clicked() {
                    self.add_sibling_to_selected();
                }

                ui.separator();
                ui.label("Nodo:");
                let text_resp = ui.text_edit_singleline(&mut self.edit_buffer);
                if text_resp.gained_focus() {
                    self.editing_node = true;
                }
                if (text_resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    && self.editing_node
                    {
                        self.apply_edit_buffer();
                    }
            });
        });

        egui::SidePanel::left("editor")
        .resizable(true)
        .default_width(360.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let response = ui.add_sized(
                    ui.available_size(),
                                            egui::TextEdit::multiline(&mut self.md_text).code_editor(),
                );
                if response.changed() {
                    self.root_tree = parse_markdown(&self.md_text);
                    if get_node(&self.root_tree, &self.selected_path).is_none() {
                        self.selected_path.clear();
                    }
                    self.sync_edit_buffer();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let canvas_rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());

            if response.dragged() {
                self.renderer.pan += response.drag_delta();
            }

            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if ui.input(|i| i.modifiers.ctrl) && scroll != 0.0 {
                self.renderer.zoom *= if scroll > 0.0 { 1.1 } else { 0.9 };
                self.renderer.zoom = self.renderer.zoom.clamp(0.2, 3.0);
            }

            self.renderer.measure_tree(ui, &mut self.root_tree, 0);
            self.renderer.layout_tree(&mut self.root_tree);

            let offset = canvas_rect.center().to_vec2() + self.renderer.pan;

            if response.clicked() || response.double_clicked() {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    let mut path_buf = Vec::new();
                    if let Some((hit_path, is_toggle)) = MindMapRenderer::hit_test(
                        &mut self.root_tree,
                        offset,
                        self.renderer.zoom,
                        pointer_pos,
                        &mut path_buf,
                    ) {
                        if is_toggle {
                            if let Some(node) =
                                Self::get_node_mut_safe(&mut self.root_tree, &hit_path)
                                {
                                    node.collapsed = !node.collapsed;
                                }
                        } else {
                            self.selected_path = hit_path;
                            self.sync_edit_buffer();
                            if response.double_clicked() {
                                self.editing_node = true;
                            }
                        }
                    } else {
                        self.selected_path.clear();
                        self.sync_edit_buffer();
                    }
                }
            }

            let painter = ui.painter_at(canvas_rect);
            self.renderer.draw_edges(&painter, &self.root_tree, offset, true);

            let mut draw_path = Vec::new();
            self.renderer.draw_nodes(
                &painter,
                &mut self.root_tree,
                offset,
                0,
                &self.selected_path,
                &mut draw_path,
            );
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MindCraft - Rust Edition",
        options,
        Box::new(|_| Box::new(MindMapApp::default())),
    )
}
