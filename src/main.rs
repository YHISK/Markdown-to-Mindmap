use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use std::collections::HashMap;

// ==========================================
// 0. Tipos de Vista, Gráfico y Persistencia
// ==========================================
#[derive(Clone, Copy, PartialEq, Eq)]
enum AppScreen {
    Welcome,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WelcomeTab {
    Recent,
    LocalFiles,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChartType {
    IdeaMap,
    LogicChart,
    TreeChart,
    Timeline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Style,
    Presentation,
    Map,
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
            Direction::Down => "De arriba abajo",
            Direction::Up => "De abajo arriba",
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeStyle {
    Bezier,
    Orthogonal,
}

impl EdgeStyle {
    fn label(self) -> &'static str {
        match self {
            EdgeStyle::Bezier => "Curva orgánica",
            EdgeStyle::Orthogonal => "Línea recta / ángulo",
        }
    }
}

#[derive(Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    ext: String,
    relative_folder: String,
}

/// Relación manual: línea punteada entre dos nodos cualesquiera
#[derive(Clone)]
struct Relation {
    from_path: Vec<usize>,
    to_path: Vec<usize>,
    label: String,
}

/// Resumen: engloba un rango de hijos consecutivos de un padre
/// y los conecta a un nodo resumen (hijo normal del mismo padre)
#[derive(Clone)]
struct Summary {
    parent_path: Vec<usize>,
    start: usize,        // índice del primer hijo cubierto
    end: usize,          // índice del último hijo cubierto (inclusive)
    node_idx: usize,     // índice del nodo resumen dentro del padre
}

// ==========================================
// 1. Estructura de Árbol, Parser MD y Lector .xmind
// ==========================================
#[derive(Clone)]
struct TreeNode {
    title: String,
    children: Vec<TreeNode>,
    collapsed: bool,
    dir_override: NodeDirectionOverride,
    custom_fill: Option<Color32>,
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
            custom_fill: None,
            pos: Pos2::ZERO,
            size: Vec2::ZERO,
            toggle_rect: Rect::NOTHING,
        }
    }

    fn count_nodes(&self) -> usize {
        1 + self.children.iter().map(|c| c.count_nodes()).sum::<usize>()
    }
}

fn strip_md(text: &str) -> String {
    text.replace("**", "")
    .replace("__", "")
    .replace("*", "")
    .replace("_", "")
}

fn parse_markdown(md: &str) -> TreeNode {
    let mut root = TreeNode::new("Tema Central".to_string());
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

/// Importa archivos .xmind reales extrayendo content.json del paquete ZIP
fn import_xmind(path: &Path) -> Result<TreeNode, String> {
    let file = File::open(path).map_err(|e| format!("No se pudo abrir el archivo: {}", e))?;
    let mut archive =
    zip::ZipArchive::new(file).map_err(|e| format!("Archivo .xmind/ZIP inválido: {}", e))?;

    let mut content_str = String::new();
    {
        let mut content_file = archive
        .by_name("content.json")
        .map_err(|_| "Este .xmind no contiene content.json (formato antiguo XML)".to_string())?;
        content_file
        .read_to_string(&mut content_str)
        .map_err(|e| e.to_string())?;
    }

    let json_val: Value =
    serde_json::from_str(&content_str).map_err(|e| format!("JSON inválido: {}", e))?;

    let root_topic = json_val
    .as_array()
    .and_then(|sheets| sheets.first())
    .and_then(|sheet| sheet.get("rootTopic"))
    .ok_or_else(|| "No se encontró rootTopic en content.json".to_string())?;

    Ok(xmind_topic_to_node(root_topic))
}

fn xmind_topic_to_node(topic: &Value) -> TreeNode {
    let title = topic
    .get("title")
    .and_then(|t| t.as_str())
    .unwrap_or("Sin título")
    .to_string();
    let mut node = TreeNode::new(title);

    if let Some(attached) = topic
        .get("children")
        .and_then(|c| c.get("attached"))
        .and_then(|a| a.as_array())
        {
            for child_val in attached {
                node.children.push(xmind_topic_to_node(child_val));
            }
        }
        node
}

fn latex_to_unicode(latex: &str) -> String {
    let mut res = latex.to_string();

    // 1. Extraer el contenido de \text{...} (ej. \text{ mg/dl} ->  mg/dl)
    while let Some(start) = res.find("\\text{") {
        let after_cmd = start + "\\text{".len();
        if let Some(rel_end) = res[after_cmd..].find('}') {
            let end = after_cmd + rel_end;
            let inner = res[after_cmd..end].to_string();
            res.replace_range(start..=end, &inner);
        } else {
            break;
        }
    }

    // 2. Reemplazos de comandos y símbolos frecuentes
    let replacements = [
        ("\\%", "%"),
        ("\\sim", "~"),
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"),
        ("\\epsilon", "ε"), ("\\theta", "θ"), ("\\lambda", "λ"), ("\\mu", "μ"),
        ("\\pi", "π"), ("\\sigma", "σ"), ("\\omega", "ω"), ("\\infty", "∞"),
        ("\\sum", "∑"), ("\\int", "∫"), ("\\approx", "≈"), ("\\neq", "≠"),
        ("\\leq", "≤"), ("\\geq", "≥"), ("\\pm", "±"), ("\\times", "×"),
        ("\\rightarrow", "→"), ("\\leftarrow", "←"), ("\\Rightarrow", "⇒"),
        ("^0", "⁰"), ("^1", "¹"), ("^2", "²"), ("^3", "³"), ("^4", "⁴"),
        ("^5", "⁵"), ("^6", "⁶"), ("^7", "⁷"), ("^8", "⁸"), ("^9", "⁹"),
        ("^n", "ⁿ"), ("^i", "ⁱ"),
        ("_0", "₀"), ("_1", "₁"), ("_2", "₂"), ("_3", "₃"), ("_4", "₄"),
        ("_5", "₅"), ("_6", "₆"), ("_7", "₇"), ("_8", "₈"), ("_9", "₉"),
        ("_n", "ₙ"), ("_i", "ᵢ"),
    ];
    for (from, to) in replacements {
        res = res.replace(from, to);
    }
    res
}

// Parser en línea: detecta Math ($...$) y Markdown (** y *) sin cortar palabras
fn build_inline_layout_job(
    ui: &egui::Ui,
    raw_text: &str,
    font_size: f32,
    text_color: Color32,
    max_text_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_text_width.max(40.0);

    // ¡ESTA ES LA LÍNEA QUE ARREGLA EL CORTE DE PALABRAS!
    job.wrap.break_anywhere = false;

    // 1. Pre-procesar LaTeX ($...$)
    let mut processed_text = String::new();
    let mut in_math = false;
    for part in raw_text.split('$') {
        if in_math {
            processed_text.push_str(&latex_to_unicode(part));
        } else {
            processed_text.push_str(part);
        }
        in_math = !in_math;
    }

    // 2. Procesar Negritas y Cursivas
    let chars: Vec<char> = processed_text.chars().collect();
    let mut i = 0;
    let mut bold = false;
    let mut italic = false;
    let mut buf = String::new();

    let push_segment = |buf: &mut String, bold: bool, italic: bool, job: &mut egui::text::LayoutJob| {
        if buf.is_empty() { return; }

        let mut fmt = egui::TextFormat::simple(FontId::proportional(font_size), text_color);
        fmt.italics = italic;

        if bold {
            // Hacemos la letra ligeramente más grande y con más espaciado para simular negrita
            fmt.font_id.size = font_size * 1.05;
            fmt.extra_letter_spacing = 1.2;
            fmt.line_height = Some(font_size * 1.2);
        }

        job.append(buf, 0.0, fmt);
        buf.clear();
    };

    while i < chars.len() {
        // Detectar ** o __ para negrita
        if (chars[i] == '*' && chars.get(i + 1) == Some(&'*'))
            || (chars[i] == '_' && chars.get(i + 1) == Some(&'_'))
            {
                push_segment(&mut buf, bold, italic, &mut job);
                bold = !bold;
                i += 2;
                continue;
            }

            // Detectar * o _ para cursiva
            if chars[i] == '*' || chars[i] == '_' {
                push_segment(&mut buf, bold, italic, &mut job);
                italic = !italic;
                i += 1;
                continue;
            }

            buf.push(chars[i]);
            i += 1;
    }

    push_segment(&mut buf, bold, italic, &mut job);

    if job.sections.is_empty() {
        job.append("", 0.0, egui::TextFormat::simple(FontId::proportional(font_size), text_color));
    }

    ui.fonts(|f| f.layout_job(job))
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
        "title": "Mind Map",
        "rootTopic": root_json
    }]);

    let metadata = json!({
        "creator": { "name": "MindCraft Rust", "version": "0.3" },
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
// 3. Escáner Recursivo de Carpetas e Historial
// ==========================================
fn scan_directory_recursive(base_dir: &Path, current_dir: &Path, results: &mut Vec<FileEntry>) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            scan_directory_recursive(base_dir, &path, results);
        } else if path.is_file() {
            let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

            if ext == "md" || ext == "xmind" {
                let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Sin nombre")
                .to_string();
                let relative_folder = path
                .parent()
                .and_then(|p| p.strip_prefix(base_dir).ok())
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    if s.is_empty() {
                        "Raíz".to_string()
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| "Raíz".to_string());

                results.push(FileEntry {
                    name,
                    path,
                    ext,
                    relative_folder,
                });
            }
        }
    }
}

fn history_storage_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".config").join("mindcraft");
    let _ = fs::create_dir_all(&dir);
    dir.join("recent_files.json")
}

fn load_recent_history() -> Vec<FileEntry> {
    let path = history_storage_path();
    let Ok(data) = fs::read_to_string(path) else {
        return vec![];
    };
    let Ok(val) = serde_json::from_str::<Value>(&data) else {
        return vec![];
    };
    let mut out = vec![];
    if let Some(arr) = val.as_array() {
        for item in arr {
            if let Some(p_str) = item.as_str() {
                let p = PathBuf::from(p_str);
                if p.exists() {
                    let name = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Mapa")
                    .to_string();
                    let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("md")
                    .to_lowercase();
                    let folder = p
                    .parent()
                    .map(|par| par.to_string_lossy().to_string())
                    .unwrap_or_default();
                    out.push(FileEntry {
                        name,
                        path: p,
                        ext,
                        relative_folder: folder,
                    });
                }
            }
        }
    }
    out
}

fn save_recent_history(recents: &[FileEntry]) {
    let paths: Vec<String> = recents
    .iter()
    .take(20)
    .map(|f| f.path.to_string_lossy().to_string())
    .collect();
    if let Ok(json_str) = serde_json::to_string_pretty(&paths) {
        let _ = fs::write(history_storage_path(), json_str);
    }
}

// ==========================================
// 4. Motor de Layout y Renderizado Estilo XMind
// ==========================================
struct MindMapRenderer {
    zoom: f32,
    pan: Vec2,
    chart_type: ChartType,
    logic_direction: Direction,
    edge_style: EdgeStyle,
    show_arrows: bool,
    branch_colors: bool,
}

impl MindMapRenderer {
    fn to_screen(&self, pos: Pos2, offset: Vec2) -> Pos2 {
        (offset + pos.to_vec2() * self.zoom).to_pos2()
    }

    fn measure_tree(&self, ui: &egui::Ui, node: &mut TreeNode, depth: usize) {
        let base_size = match depth {
            0 => 19.0,
            1 => 14.5,
            2 => 13.0,
            _ => 12.0,
        };
        let has_children = !node.children.is_empty();
        let toggle_extra: f32 = if has_children { 22.0 } else { 0.0 };
        let pad_x: f32 = if depth == 0 { 32.0 } else { 24.0 };
        let pad_y: f32 = if depth == 0 { 20.0 } else { 14.0 };

        let max_box_w: f32 = if depth == 0 { 320.0 } else { 250.0 };
        let wrap_w: f32 = (max_box_w - pad_x - toggle_extra).max(60.0);

        let galley = build_inline_layout_job(ui, &node.title, base_size, Color32::BLACK, wrap_w);
        let text_w = galley.size().x.min(wrap_w);
        let text_h = galley.size().y.max(base_size * 1.2);

        node.size = Vec2::new(
            (text_w + pad_x + toggle_extra).clamp(70.0, max_box_w),
                              (text_h + pad_y).max(30.0),
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
                let root_dir = node.dir_override.resolve(self.logic_direction);
                Self::layout_children_from(node, root_dir);
            }
            ChartType::TreeChart => {
                let root_dir = node.dir_override.resolve(Direction::Down);
                Self::layout_children_from(node, root_dir);
            }
            ChartType::Timeline => self.layout_timeline(node),
        }
    }

    fn layout_idea_map(&self, node: &mut TreeNode) {
        let v_gap = 35.0; // <-- Aumentado de 22.0 a 35.0 para más espacio vertical
        let h_gap = 100.0; // <-- Aumentado de 75.0 a 100.0 para más espacio horizontal

        let mid = (node.children.len() + 1) / 2;
        let (right_kids, left_kids) = node.children.split_at_mut(mid);

        // Lado Derecho
        let mut right_h = 0.0;
        let mut right_heights = Vec::with_capacity(right_kids.len());
        for child in right_kids.iter() {
            let h = Self::subtree_cross_span(child, Direction::Right, v_gap);
            right_heights.push(h);
            right_h += h;
        }
        if !right_kids.is_empty() {
            right_h += v_gap * (right_kids.len() as f32 - 1.0);
        }
        let mut cursor_y = -right_h / 2.0;
        for (child, &ch_h) in right_kids.iter_mut().zip(right_heights.iter()) {
            Self::place_node_in_direction(
                child,
                node.pos.x + node.size.x / 2.0 + h_gap,
                cursor_y,
                ch_h,
                Direction::Right,
                h_gap,
                v_gap,
            );
            cursor_y += ch_h + v_gap;
        }

        // Lado Izquierdo
        let mut left_h = 0.0;
        let mut left_heights = Vec::with_capacity(left_kids.len());
        for child in left_kids.iter() {
            let h = Self::subtree_cross_span(child, Direction::Left, v_gap);
            left_heights.push(h);
            left_h += h;
        }
        if !left_kids.is_empty() {
            left_h += v_gap * (left_kids.len() as f32 - 1.0);
        }
        let mut cursor_y = -left_h / 2.0;
        for (child, &ch_h) in left_kids.iter_mut().zip(left_heights.iter()) {
            Self::place_node_in_direction(
                child,
                node.pos.x - node.size.x / 2.0 - h_gap,
                cursor_y,
                ch_h,
                Direction::Left,
                h_gap,
                v_gap,
            );
            cursor_y += ch_h + v_gap;
        }
    }

    /// Distribuye los hijos de `parent` respetando `parent_dir` (y la dirección propia de cada nodo)
    fn layout_children_from(parent: &mut TreeNode, parent_dir: Direction) {
        if parent.children.is_empty() || parent.collapsed {
            return;
        }

        let gap_main = 100.0; // <-- Aumentado de 65.0 a 100.0
        let gap_cross = 32.0; // <-- Aumentado de 24.0 a 32.0

        let spans: Vec<f32> = parent
        .children
        .iter()
        .map(|c| Self::subtree_cross_span(c, parent_dir, gap_cross))
        .collect();
        let total_span =
        spans.iter().sum::<f32>() + gap_cross * (parent.children.len().saturating_sub(1) as f32);

        let anchor_cross = if parent_dir.is_horizontal() {
            parent.pos.y
        } else {
            parent.pos.x
        };
        let mut cursor = anchor_cross - total_span / 2.0;

        let edge_coord = match parent_dir {
            Direction::Right => parent.pos.x + parent.size.x / 2.0 + gap_main,
            Direction::Left => parent.pos.x - parent.size.x / 2.0 - gap_main,
            Direction::Down => parent.pos.y + parent.size.y / 2.0 + gap_main,
            Direction::Up => parent.pos.y - parent.size.y / 2.0 - gap_main,
        };

        for (child, span) in parent.children.iter_mut().zip(spans.into_iter()) {
            Self::place_node_in_direction(
                child,
                edge_coord,
                cursor,
                span,
                parent_dir,
                gap_main,
                gap_cross,
            );
            cursor += span + gap_cross;
        }
    }

    /// Espacio perpendicular a `incoming_dir` que ocupa `node` junto con sus descendientes
    fn subtree_cross_span(node: &TreeNode, incoming_dir: Direction, gap_cross: f32) -> f32 {
        let own_cross = if incoming_dir.is_horizontal() {
            node.size.y
        } else {
            node.size.x
        };

        if node.children.is_empty() || node.collapsed {
            return own_cross;
        }

        let out_dir = node.dir_override.resolve(incoming_dir);

        // Si los hijos salen en el mismo eje (horizontal->horizontal o vertical->vertical), suman su ancho perpendicular
        if out_dir.is_horizontal() == incoming_dir.is_horizontal() {
            let sum: f32 = node
            .children
            .iter()
            .map(|c| Self::subtree_cross_span(c, out_dir, gap_cross))
            .sum();
            let gaps = gap_cross * (node.children.len().saturating_sub(1) as f32);
            (sum + gaps).max(own_cross)
        } else {
            // Si el nodo gira 90° respecto a su padre (ej. padre va a la derecha y este nodo baja sus hijos),
            // su extensión perpendicular al padre es la profundidad total en `out_dir`
            let branch_depth = Self::subtree_main_depth(node, out_dir, 65.0, gap_cross);
            branch_depth.max(own_cross)
        }
    }

    fn subtree_main_depth(node: &TreeNode, dir: Direction, gap_main: f32, gap_cross: f32) -> f32 {
        let own_main = if dir.is_horizontal() {
            node.size.x
        } else {
            node.size.y
        };
        if node.children.is_empty() || node.collapsed {
            return own_main;
        }
        let max_child = node
        .children
        .iter()
        .map(|c| {
            let c_dir = c.dir_override.resolve(dir);
            if c_dir.is_horizontal() == dir.is_horizontal() {
                Self::subtree_main_depth(c, c_dir, gap_main, gap_cross)
            } else {
                Self::subtree_cross_span(c, dir, gap_cross)
            }
        })
        .fold(0.0_f32, f32::max);
        own_main + gap_main + max_child
    }

    fn place_node_in_direction(
        node: &mut TreeNode,
        edge_coord: f32,
        cross_start: f32,
        span: f32,
        incoming_dir: Direction,
        _gap_main: f32,
        _gap_cross: f32,
    ) {
        let out_dir = node.dir_override.resolve(incoming_dir);

        // Si el nodo gira 90° respecto al padre, alineamos su inicio cerca del padre para que no invada hermanos
        if out_dir.is_horizontal() != incoming_dir.is_horizontal() {
            let node_cross_size = if incoming_dir.is_horizontal() {
                node.size.y
            } else {
                node.size.x
            };
            let cross_pos = match out_dir {
                Direction::Down | Direction::Right => cross_start + node_cross_size / 2.0,
                Direction::Up | Direction::Left => cross_start + span - node_cross_size / 2.0,
            };
            node.pos = match incoming_dir {
                Direction::Right => Pos2::new(edge_coord + node.size.x / 2.0, cross_pos),
                Direction::Left => Pos2::new(edge_coord - node.size.x / 2.0, cross_pos),
                Direction::Down => Pos2::new(cross_pos, edge_coord + node.size.y / 2.0),
                Direction::Up => Pos2::new(cross_pos, edge_coord - node.size.y / 2.0),
            };
        } else {
            let cross_center = cross_start + span / 2.0;
            node.pos = match incoming_dir {
                Direction::Right => Pos2::new(edge_coord + node.size.x / 2.0, cross_center),
                Direction::Left => Pos2::new(edge_coord - node.size.x / 2.0, cross_center),
                Direction::Down => Pos2::new(cross_center, edge_coord + node.size.y / 2.0),
                Direction::Up => Pos2::new(cross_center, edge_coord - node.size.y / 2.0),
            };
        }

        Self::layout_children_from(node, out_dir);
    }

    fn layout_timeline(&self, root: &mut TreeNode) {
        let step_x = 95.0;
        let branch_gap_y = 55.0;
        let sub_gap_y = 18.0;
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
                    let sub_h = Self::subtree_cross_span(sub, Direction::Right, sub_gap_y);
                    let sub_center_y = if above {
                        y_acc - sub_h / 2.0
                    } else {
                        y_acc + sub_h / 2.0
                    };
                    let sub_x = milestone.pos.x + milestone.size.x / 2.0 + sub_gap_x + sub.size.x / 2.0;
                    sub.pos = Pos2::new(sub_x, sub_center_y);
                    Self::layout_children_from(sub, Direction::Right);

                    if above {
                        y_acc -= sub_h + sub_gap_y;
                    } else {
                        y_acc += sub_h + sub_gap_y;
                    }
                }
            }

            let max_right =
            Self::max_right_extent(milestone).max(milestone.pos.x + milestone.size.x / 2.0);
            cursor_x = max_right + step_x;
        }
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

    fn branch_palette(branch_idx: usize) -> Color32 {
        const PALETTE: [Color32; 6] = [
            Color32::from_rgb(59, 130, 246), // Azul
            Color32::from_rgb(16, 185, 129), // Esmeralda
            Color32::from_rgb(245, 158, 11), // Ámbar
            Color32::from_rgb(139, 92, 246), // Violeta
            Color32::from_rgb(236, 72, 153), // Rosa
            Color32::from_rgb(20, 184, 166), // Turquesa
        ];
        PALETTE[branch_idx % PALETTE.len()]
    }

    fn draw_edges(
        &self,
        painter: &egui::Painter,
        node: &TreeNode,
        offset: Vec2,
        is_root: bool,
        branch_idx: usize,
    ) {
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
            .map(|m| m.pos.x + m.size.x / 2.0 + 36.0)
            .unwrap_or(node.pos.x + node.size.x / 2.0);
            let axis_end = self.to_screen(Pos2::new(last_x, node.pos.y), offset);

            painter.line_segment(
                [root_right, axis_end],
                Stroke::new(3.0 * self.zoom, Color32::from_rgb(45, 55, 72)),
            );

            for (i, milestone) in node.children.iter().enumerate() {
                let color = if self.branch_colors {
                    Self::branch_palette(i)
                } else {
                    Color32::from_rgb(75, 85, 99)
                };
                let m_center = self.to_screen(milestone.pos, offset);
                let above = i % 2 == 0;
                if !milestone.children.is_empty() && !milestone.collapsed {
                    for sub in &milestone.children {
                        let start = if above {
                            Pos2::new(m_center.x, m_center.y - milestone.size.y * self.zoom / 2.0)
                        } else {
                            Pos2::new(m_center.x, m_center.y + milestone.size.y * self.zoom / 2.0)
                        };
                        let sub_center = self.to_screen(sub.pos, offset);
                        let end =
                        Pos2::new(sub_center.x - sub.size.x * self.zoom / 2.0, sub_center.y);
                        self.draw_connector(painter, start, end, true, color);
                        self.draw_edges(painter, sub, offset, false, i);
                    }
                }
            }
            return;
        }

        let n_center = self.to_screen(node.pos, offset);
        let n_w = node.size.x * self.zoom;
        let n_h = node.size.y * self.zoom;

        for (i, child) in node.children.iter().enumerate() {
            let active_branch = if is_root { i } else { branch_idx };
            let edge_color = if self.branch_colors {
                Self::branch_palette(active_branch)
            } else {
                Color32::from_rgb(75, 85, 99)
            };

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

            self.draw_connector(painter, start, end, horizontal, edge_color);
            self.draw_edges(painter, child, offset, false, active_branch);
        }
    }

    fn draw_connector(
        &self,
        painter: &egui::Painter,
        start: Pos2,
        end: Pos2,
        horizontal: bool,
        color: Color32,
    ) {
        let stroke = Stroke::new(2.0 * self.zoom, color);

        match self.edge_style {
            EdgeStyle::Bezier => {
                let (c1, c2) = if horizontal {
                    let ctrl = (end.x - start.x) * 0.5;
                    (start + Vec2::new(ctrl, 0.0), end - Vec2::new(ctrl, 0.0))
                } else {
                    let ctrl = (end.y - start.y) * 0.5;
                    (start + Vec2::new(0.0, ctrl), end - Vec2::new(0.0, ctrl))
                };
                let bezier = egui::epaint::CubicBezierShape::from_points_stroke(
                    [start, c1, c2, end],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                );
                painter.add(bezier);
            }
            EdgeStyle::Orthogonal => {
                if horizontal {
                    // Usamos un offset del 30% en lugar del 50% (punto medio) para que las curvas se vean más naturales
                    let offset_x = (end.x - start.x) * 0.3;
                    let p1 = Pos2::new(start.x + offset_x, start.y);
                    let p2 = Pos2::new(end.x - offset_x, end.y);
                    painter.line_segment([start, p1], stroke);
                    painter.line_segment([p1, p2], stroke);
                    painter.line_segment([p2, end], stroke);
                } else {
                    let offset_y = (end.y - start.y) * 0.3;
                    let p1 = Pos2::new(start.x, start.y + offset_y);
                    let p2 = Pos2::new(end.x, end.y - offset_y);
                    painter.line_segment([start, p1], stroke);
                    painter.line_segment([p1, p2], stroke);
                    painter.line_segment([p2, end], stroke);
                }
            }
        }

        if self.show_arrows {
            let dir_vec = if self.edge_style == EdgeStyle::Orthogonal {
                if horizontal {
                    Vec2::new((end.x - start.x).signum(), 0.0)
                } else {
                    Vec2::new(0.0, (end.y - start.y).signum())
                }
            } else {
                (end - start).normalized()
            };
            let perp = Vec2::new(-dir_vec.y, dir_vec.x);
            let arrow_len = 7.0 * self.zoom;
            let arrow_w = 4.0 * self.zoom;
            let tip = end;
            let left = tip - dir_vec * arrow_len + perp * arrow_w;
            let right = tip - dir_vec * arrow_len - perp * arrow_w;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, left, right],
                color,
                Stroke::NONE,
            ));
        }
    }

    fn draw_nodes(
        &self,
        painter: &egui::Painter,
        ui: &egui::Ui,
        node: &mut TreeNode,
        offset: Vec2,
        depth: usize,
        branch_idx: usize,
        selected_path: &[usize],
        current_path: &mut Vec<usize>,
    ) {
        let center = self.to_screen(node.pos, offset);
        let scaled_size = node.size * self.zoom;
        let rect = Rect::from_center_size(center, scaled_size);
        let is_selected = current_path.as_slice() == selected_path;

        // Paleta limpia estilo XMind (Tema central oscuro, ramas principales acentuadas, subtemas claros)
        let (default_bg, default_border, text_color) = match depth {
            0 => (
                Color32::from_rgb(24, 31, 44),
                  Color32::from_rgb(24, 31, 44),
                  Color32::WHITE,
            ),
            1 => {
                if self.branch_colors {
                    let c = Self::branch_palette(branch_idx);
                    (c, c, Color32::WHITE)
                } else {
                    (
                        Color32::from_rgb(107, 114, 128),
                     Color32::from_rgb(107, 114, 128),
                     Color32::WHITE,
                    )
                }
            }
            2 => (
                Color32::from_rgb(243, 244, 246),
                  Color32::from_rgb(209, 213, 219),
                  Color32::from_rgb(31, 41, 55),
            ),
            _ => (
                Color32::WHITE,
                Color32::from_rgb(209, 213, 219),
                  Color32::from_rgb(55, 65, 81),
            ),
        };

        let bg = node.custom_fill.unwrap_or(default_bg);
        let final_text = if node.custom_fill.is_some() {
            // Contraste automático si el usuario eligió color personalizado
            let lum = 0.299 * (bg.r() as f32) + 0.587 * (bg.g() as f32) + 0.114 * (bg.b() as f32);
            if lum < 140.0 {
                Color32::WHITE
            } else {
                Color32::from_rgb(17, 24, 39)
            }
        } else {
            text_color
        };

        let rounding = if depth == 0 {
            10.0 * self.zoom
        } else {
            7.0 * self.zoom
        };

        // Sombra sutil estilo XMind
        let shadow_rect = rect.translate(Vec2::new(0.0, 2.0 * self.zoom));
        painter.rect_filled(shadow_rect, rounding, Color32::from_black_alpha(18));

        painter.rect_filled(rect, rounding, bg);

        let stroke = if is_selected {
            Stroke::new(2.5 * self.zoom, Color32::from_rgb(37, 99, 235))
        } else {
            Stroke::new(1.2 * self.zoom, default_border)
        };
        painter.rect_stroke(rect, rounding, stroke);

        let has_children = !node.children.is_empty();
        let btn_size = 15.0 * self.zoom;

        if has_children {
            node.toggle_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 12.0 * self.zoom, rect.center().y),
                                                      Vec2::splat(btn_size),
            );
            painter.rect_filled(
                node.toggle_rect,
                4.0 * self.zoom,
                Color32::from_rgba_unmultiplied(255, 255, 255, 220),
            );
            painter.rect_stroke(
                node.toggle_rect,
                4.0 * self.zoom,
                Stroke::new(1.0 * self.zoom, Color32::from_gray(140)),
            );

            let symbol = if node.collapsed { "+" } else { "-" };
            painter.text(
                node.toggle_rect.center(),
                         Align2::CENTER_CENTER,
                         symbol,
                         FontId::proportional(11.0 * self.zoom),
                         Color32::from_rgb(31, 41, 55),
            );
        } else {
            node.toggle_rect = Rect::NOTHING;
        }

        let base_size = match depth {
            0 => 19.0,
            1 => 14.5,
            2 => 13.0,
            _ => 12.0,
        };

        let base_pad_x: f32 = if depth == 0 { 32.0 } else { 24.0 };
        let base_toggle_reserve: f32 = if has_children { 24.0 } else { 0.0 };
        let base_max_box_w: f32 = if depth == 0 { 320.0 } else { 250.0 };
        let base_wrap_w: f32 = (base_max_box_w - base_pad_x - base_toggle_reserve).max(70.0);

        let galley = build_inline_layout_job(
            ui,
            &node.title,
            base_size * self.zoom,
            final_text,
            base_wrap_w * self.zoom,
        );

        // Tu nueva fórmula para centrar el texto usando las variables correctas
        let avail_w = rect.width() - (base_pad_x * self.zoom) - (base_toggle_reserve * self.zoom);
        let text_origin = Pos2::new(
            rect.left() + (base_pad_x * 0.5 * self.zoom) + ((avail_w - galley.size().x).max(0.0) * 0.5),
                                    rect.center().y - galley.size().y * 0.5,
        );

        painter.galley(text_origin, galley, final_text);

        if has_children && !node.collapsed {
            for (idx, child) in node.children.iter_mut().enumerate() {
                let active_branch = if depth == 0 { idx } else { branch_idx };
                current_path.push(idx);
                self.draw_nodes(
                    painter,
                    ui,
                    child,
                    offset,
                    depth + 1,
                    active_branch,
                    selected_path,
                    current_path,
                );
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

// ==========================================
// 5. Aplicación Principal (Bienvenida + Editor XMind UI)
// ==========================================
struct MindMapApp {
    screen: AppScreen,
    welcome_tab: WelcomeTab,
    recent_files: Vec<FileEntry>,
    scanned_folder: Option<PathBuf>,
    local_files: Vec<FileEntry>,
    search_query: String,
    status_error: Option<String>,

    current_file_path: Option<PathBuf>,
    document_title: String,
    md_text: String,
    root_tree: TreeNode,
    renderer: MindMapRenderer,
    selected_path: Vec<usize>,
    edit_buffer: String,
    editing_node: bool,
    show_md_editor: bool,
    show_inspector: bool,
    inspector_tab: InspectorTab,

    // Relaciones y resúmenes
    relations: Vec<Relation>,
    summaries: Vec<Summary>,
    relation_pending: bool,
    relation_source: Option<Vec<usize>>,
    summary_pending: bool,
    summary_source: Option<Vec<usize>>,
    selected_relation: Option<usize>,

}

impl Default for MindMapApp {
    fn default() -> Self {
        let default_md = "# Tema Central\n\n## Idea Principal 1\n- Subtema A\n- **Subtema B**\n  - Detalle 1\n  - Detalle 2\n\n## Idea Principal 2\n- Concepto clave\n- Siguiente paso".to_string();
        let root_tree = parse_markdown(&default_md);
        let edit_buffer = root_tree.title.clone();
        Self {
            screen: AppScreen::Welcome,
            welcome_tab: WelcomeTab::Recent,
            recent_files: load_recent_history(),
            scanned_folder: None,
            local_files: vec![],
            search_query: String::new(),
            status_error: None,

            current_file_path: None,
            document_title: "Nuevo mapa".to_string(),
            md_text: default_md,
            root_tree,
            renderer: MindMapRenderer {
                zoom: 1.0,
                pan: Vec2::ZERO,
                chart_type: ChartType::IdeaMap,
                logic_direction: Direction::Right,
                edge_style: EdgeStyle::Bezier,
                show_arrows: false,
                branch_colors: true,
            },
            selected_path: vec![],
            edit_buffer,
            editing_node: false,
            show_md_editor: true,
            show_inspector: true,
            inspector_tab: InspectorTab::Style,

            relations: vec![],
            summaries: vec![],
            relation_pending: false,
            relation_source: None,
            summary_pending: false,
            summary_source: None,
            selected_relation: None,
        }
    }
}

impl MindMapApp {
    fn open_new_map(&mut self) {
        let default_md = "# Tema Central\n\n## Rama Principal 1\n- Subtema 1\n- Subtema 2\n\n## Rama Principal 2\n- Subtema 3\n- Subtema 4".to_string();
        self.md_text = default_md;
        self.root_tree = parse_markdown(&self.md_text);
        self.current_file_path = None;
        self.document_title = "Nuevo mapa".to_string();
        self.selected_path.clear();
        self.sync_edit_buffer();
        self.renderer.zoom = 1.0;
        self.renderer.pan = Vec2::ZERO;
        self.status_error = None;
        self.screen = AppScreen::Editor;
    }

    fn open_file_entry(&mut self, entry: &FileEntry) {
        let path = &entry.path;
        let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

        let load_res = if ext == "xmind" {
            import_xmind(path).map(|tree| {
                let md = tree_to_markdown(&tree);
                (tree, md)
            })
        } else {
            fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .map(|md| {
                let tree = parse_markdown(&md);
                (tree, md)
            })
        };

        match load_res {
            Ok((tree, md)) => {
                self.root_tree = tree;
                self.md_text = md;
                self.current_file_path = Some(path.clone());
                self.document_title = entry.name.clone();
                self.selected_path.clear();
                self.sync_edit_buffer();
                self.renderer.zoom = 1.0;
                self.renderer.pan = Vec2::ZERO;
                self.status_error = None;
                self.record_recent(entry.clone());
                self.screen = AppScreen::Editor;
            }
            Err(err) => {
                self.status_error = Some(err);
            }
        }
    }

    fn record_recent(&mut self, entry: FileEntry) {
        self.recent_files.retain(|f| f.path != entry.path);
        self.recent_files.insert(0, entry);
        save_recent_history(&self.recent_files);
    }

    fn select_and_scan_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            let mut found = Vec::new();
            scan_directory_recursive(&folder, &folder, &mut found);
            found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.scanned_folder = Some(folder);
            self.local_files = found;
        }
    }

    fn sync_edit_buffer(&mut self) {
        if let Some(r) = self.selected_relation {
            if let Some(rel) = self.relations.get(r) {
                self.edit_buffer = rel.label.clone();
                return;
            }
        }
        if let Some(node) = get_node(&self.root_tree, &self.selected_path) {
            self.edit_buffer = node.title.clone();
        }
    }

    fn apply_edit_buffer(&mut self) {
        if let Some(r) = self.selected_relation {
            if let Some(rel) = self.relations.get_mut(r) {
                rel.label = self.edit_buffer.trim().to_string();
            }
            self.editing_node = false;
            return;
        }
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
            parent.children.push(TreeNode::new("Subtema".to_string()));
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
            .insert(clamped_idx, TreeNode::new("Tema hermano".to_string()));
            let mut new_path = parent_path.to_vec();
            new_path.push(clamped_idx);
            self.selected_path = new_path;
            self.sync_edit_buffer();
            self.editing_node = true;
            self.md_text = tree_to_markdown(&self.root_tree);
        }
    }

    fn delete_selected_node(&mut self) {
        if self.selected_path.is_empty() {
            return;
        }
        let parent_path = &self.selected_path[..self.selected_path.len() - 1];
        let remove_idx = *self.selected_path.last().unwrap();
        if let Some(parent) = Self::get_node_mut_safe(&mut self.root_tree, parent_path) {
            if remove_idx < parent.children.len() {
                parent.children.remove(remove_idx);
                self.selected_path = parent_path.to_vec();
                self.sync_edit_buffer();
                self.md_text = tree_to_markdown(&self.root_tree);
            }
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

    /// Elimina relaciones/resúmenes que apuntan a nodos que ya no existen
    fn sanitize_links(&mut self) {
        self.relations.retain(|r| {
            get_node(&self.root_tree, &r.from_path).is_some()
            && get_node(&self.root_tree, &r.to_path).is_some()
        });
        self.summaries.retain(|s| {
            get_node(&self.root_tree, &s.parent_path)
            .map_or(false, |p| s.end < p.children.len() && s.node_idx < p.children.len())
        });
        if let Some(r) = self.selected_relation {
            if r >= self.relations.len() { self.selected_relation = None; }
        }
    }

    /// Zoom anclado al cursor: el punto bajo el puntero no se mueve
    fn zoom_at(&mut self, canvas_rect: Rect, pointer: Pos2, factor: f32) {
        let old_zoom = self.renderer.zoom;
        let new_zoom = (old_zoom * factor).clamp(0.2, 3.0);
        if (new_zoom - old_zoom).abs() < f32::EPSILON { return; }
        let old_offset = canvas_rect.center().to_vec2() + self.renderer.pan;
        let world = (pointer.to_vec2() - old_offset) / old_zoom;
        let new_offset = pointer.to_vec2() - world * new_zoom;
        self.renderer.pan = new_offset - canvas_rect.center().to_vec2();
        self.renderer.zoom = new_zoom;
    }

    // ==========================================
    // Pantalla 1: Bienvenida Minimalista (XMind Style)
    // ==========================================
    fn render_welcome_screen(&mut self, ctx: &egui::Context) {
        // Barra lateral izquierda limpia con SOLO "Archivos recientes" y "Archivos locales"
        egui::SidePanel::left("welcome_sidebar")
        .resizable(false)
        .exact_width(230.0)
        .frame(
            egui::Frame::none()
            .fill(Color32::from_rgb(245, 246, 248))
            .inner_margin(egui::Margin::symmetric(16.0, 22.0))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(229, 231, 235))),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (badge_rect, _) =
                ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    badge_rect,
                    Rounding::same(6.0),
                                         Color32::from_rgb(239, 68, 68),
                );
                ui.painter().text(
                    badge_rect.center(),
                                  Align2::CENTER_CENTER,
                                  "M",
                                  FontId::proportional(15.0),
                                  Color32::WHITE,
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("MindCraft")
                    .size(18.0)
                    .strong()
                    .color(Color32::from_rgb(17, 24, 39)),
                );
            });

            ui.add_space(28.0);

            let nav_items = [
                (WelcomeTab::Recent, "🕒  Archivos recientes"),
              (WelcomeTab::LocalFiles, "📁  Archivos locales"),
            ];

            for (tab, label) in nav_items {
                let active = self.welcome_tab == tab;
                let fill = if active {
                    Color32::from_rgb(226, 230, 236)
                } else {
                    Color32::TRANSPARENT
                };
                let text_col = if active {
                    Color32::from_rgb(17, 24, 39)
                } else {
                    Color32::from_rgb(75, 85, 99)
                };

                let btn = egui::Button::new(
                    egui::RichText::new(label).size(14.0).color(text_col),
                )
                .fill(fill)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(ui.available_width(), 38.0));

                if ui.add(btn).clicked() {
                    self.welcome_tab = tab;
                }
                ui.add_space(6.0);
            }
        });

        // Área principal de la pantalla de bienvenida
        egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
            .fill(Color32::from_rgb(252, 252, 253))
            .inner_margin(egui::Margin::symmetric(36.0, 28.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let heading = match self.welcome_tab {
                    WelcomeTab::Recent => "Archivos recientes",
                    WelcomeTab::LocalFiles => "Archivos locales",
                };
                ui.label(
                    egui::RichText::new(heading)
                    .size(24.0)
                    .strong()
                    .color(Color32::from_rgb(17, 24, 39)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("🔍 Buscar mapa...")
                        .desired_width(220.0),
                    );

                    if self.welcome_tab == WelcomeTab::LocalFiles {
                        ui.add_space(10.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("📂 Elegir carpeta a escanear")
                                    .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(37, 99, 235))
                                .rounding(Rounding::same(7.0)),
                            )
                            .clicked()
                            {
                                self.select_and_scan_folder();
                            }
                    }
                });
            });

            if let Some(err) = &self.status_error {
                ui.add_space(10.0);
                ui.colored_label(Color32::from_rgb(220, 38, 38), format!("⚠ {}", err));
            }

            ui.add_space(22.0);

            match self.welcome_tab {
                WelcomeTab::Recent => {
                    let recents_snapshot = self.recent_files.clone();
                    let query = self.search_query.to_lowercase();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(20.0, 24.0);

                            // Tarjeta "+ Nuevo mapa" estilo XMind
                            if self.render_new_map_card(ui) {
                                self.open_new_map();
                            }

                            for entry in &recents_snapshot {
                                if !query.is_empty()
                                    && !entry.name.to_lowercase().contains(&query)
                                    {
                                        continue;
                                    }
                                    if self.render_file_card(ui, entry) {
                                        self.open_file_entry(entry);
                                    }
                            }
                        });
                    });
                }
                WelcomeTab::LocalFiles => {
                    if let Some(folder) = &self.scanned_folder {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Carpeta escaneada: {}  ({} archivos .md / .xmind)",
                                                            folder.to_string_lossy(),
                                                            self.local_files.len()
                                ))
                                .size(13.0)
                                .color(Color32::from_rgb(107, 114, 128)),
                            );
                        });
                        ui.add_space(14.0);

                        let files_snapshot = self.local_files.clone();
                        let query = self.search_query.to_lowercase();

                        if files_snapshot.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No se encontraron archivos .md ni .xmind en esta carpeta o sus subcarpetas.",
                                )
                                .color(Color32::from_rgb(107, 114, 128)),
                            );
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(20.0, 24.0);
                                    for entry in &files_snapshot {
                                        if !query.is_empty()
                                            && !entry.name.to_lowercase().contains(&query)
                                            && !entry
                                            .relative_folder
                                            .to_lowercase()
                                            .contains(&query)
                                            {
                                                continue;
                                            }
                                            if self.render_file_card(ui, entry) {
                                                self.open_file_entry(entry);
                                            }
                                    }
                                });
                            });
                        }
                    } else {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    "Selecciona una carpeta de estudios para escanear automáticamente todos tus archivos .md y .xmind (incluyendo subcarpetas).",
                                )
                                .size(15.0)
                                .color(Color32::from_rgb(107, 114, 128)),
                            );
                            ui.add_space(16.0);
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("📂 Seleccionar carpeta")
                                        .size(15.0)
                                        .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(37, 99, 235))
                                    .rounding(Rounding::same(8.0))
                                    .min_size(Vec2::new(200.0, 40.0)),
                                )
                                .clicked()
                                {
                                    self.select_and_scan_folder();
                                }
                        });
                    }
                }
            }
        });
    }

    fn render_new_map_card(&self, ui: &mut egui::Ui) -> bool {
        let mut clicked = false;
        ui.vertical(|ui| {
            let card_size = Vec2::new(190.0, 120.0);
            let (rect, resp) = ui.allocate_exact_size(card_size, egui::Sense::click());
            let bg = if resp.hovered() {
                Color32::from_rgb(243, 244, 246)
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(rect, Rounding::same(10.0), bg);
            ui.painter().rect_stroke(
                rect,
                Rounding::same(10.0),
                                     Stroke::new(1.2_f32, Color32::from_rgb(209, 213, 219)),
            );
            ui.painter().text(
                rect.center(),
                              Align2::CENTER_CENTER,
                              "+",
                              FontId::proportional(38.0),
                              Color32::from_rgb(156, 163, 175),
            );
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                Vec2::new(190.0, 22.0),
                                       egui::Layout::top_down(egui::Align::Center),
                                       |ui| {
                                           ui.label(
                                               egui::RichText::new("Nuevo mapa")
                                               .size(13.5)
                                               .color(Color32::from_rgb(31, 41, 55)),
                                           );
                                       },
            );
            if resp.clicked() {
                clicked = true;
            }
        });
        clicked
    }

    fn render_file_card(&self, ui: &mut egui::Ui, entry: &FileEntry) -> bool {
        let mut clicked = false;
        ui.vertical(|ui| {
            let card_size = Vec2::new(190.0, 120.0);
            let (rect, resp) = ui.allocate_exact_size(card_size, egui::Sense::click());
            let border = if resp.hovered() {
                Stroke::new(2.0_f32, Color32::from_rgb(17, 24, 39))
            } else {
                Stroke::new(1.2_f32, Color32::from_rgb(229, 231, 235))
            };
            ui.painter()
            .rect_filled(rect, Rounding::same(10.0), Color32::WHITE);
            ui.painter().rect_stroke(rect, Rounding::same(10.0), border);

            // Mini vista previa esquemática dentro de la tarjeta
            let c = rect.center();
            let root_box = Rect::from_center_size(c, Vec2::new(56.0, 18.0));
            ui.painter().rect_filled(
                root_box,
                Rounding::same(4.0),
                                     Color32::from_rgb(24, 31, 44),
            );
            for dy in [-24.0_f32, 0.0, 24.0] {
                let left_b =
                Rect::from_center_size(Pos2::new(c.x - 56.0, c.y + dy), Vec2::new(34.0, 10.0));
                let right_b =
                Rect::from_center_size(Pos2::new(c.x + 56.0, c.y + dy), Vec2::new(34.0, 10.0));
                ui.painter().line_segment(
                    [Pos2::new(root_box.left(), c.y), Pos2::new(left_b.right(), c.y + dy)],
                                          Stroke::new(1.0_f32, Color32::from_rgb(156, 163, 175)),
                );
                ui.painter().line_segment(
                    [Pos2::new(root_box.right(), c.y), Pos2::new(right_b.left(), c.y + dy)],
                                          Stroke::new(1.0_f32, Color32::from_rgb(156, 163, 175)),
                );
                ui.painter().rect_filled(
                    left_b,
                    Rounding::same(3.0),
                                         Color32::from_rgb(229, 231, 235),
                );
                ui.painter().rect_filled(
                    right_b,
                    Rounding::same(3.0),
                                         Color32::from_rgb(229, 231, 235),
                );
            }

            // Etiqueta de formato (.MD / .XMIND) en la esquina superior derecha
            let badge_col = if entry.ext == "xmind" {
                Color32::from_rgb(239, 68, 68)
            } else {
                Color32::from_rgb(37, 99, 235)
            };
            let badge_rect = Rect::from_min_size(
                Pos2::new(rect.right() - 46.0, rect.top() + 8.0),
                                                 Vec2::new(38.0, 16.0),
            );
            ui.painter()
            .rect_filled(badge_rect, Rounding::same(4.0), badge_col);
            ui.painter().text(
                badge_rect.center(),
                              Align2::CENTER_CENTER,
                              entry.ext.to_uppercase(),
                              FontId::proportional(9.5),
                              Color32::WHITE,
            );

            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                Vec2::new(190.0, 34.0),
                                       egui::Layout::top_down(egui::Align::Center),
                                       |ui| {
                                           let short_name = if entry.name.chars().count() > 24 {
                                               format!("{}...", entry.name.chars().take(21).collect::<String>())
                                           } else {
                                               entry.name.clone()
                                           };
                                           ui.label(
                                               egui::RichText::new(short_name)
                                               .size(13.0)
                                               .color(Color32::from_rgb(31, 41, 55)),
                                           );
                                           if !entry.relative_folder.is_empty() {
                                               ui.label(
                                                   egui::RichText::new(&entry.relative_folder)
                                                   .size(10.5)
                                                   .color(Color32::from_rgb(156, 163, 175)),
                                               );
                                           }
                                       },
            );

            if resp.clicked() {
                clicked = true;
            }
        });
        clicked
    }

    // --- FUNCIONES DE LAS PESTAÑAS DEL INSPECTOR ---

    fn render_tab_style(&mut self, ui: &mut egui::Ui) {
        let has_selection = !self.selected_path.is_empty() || get_node(&self.root_tree, &self.selected_path).is_some();

        ui.add_space(4.0);

        // Sección: Estructura
        ui.label(egui::RichText::new("▼ Estructura").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
        ui.add_space(6.0);
        egui::ComboBox::from_id_source("style_chart_type")
        .width(ui.available_width())
        .selected_text(self.renderer.chart_type.label())
        .show_ui(ui, |ui| {
            for ct in [ChartType::IdeaMap, ChartType::LogicChart, ChartType::TreeChart, ChartType::Timeline] {
                ui.selectable_value(&mut self.renderer.chart_type, ct, ct.label());
            }
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // Sección: Forma y Texto
        // 1. Creamos una bandera para saber si hubo cambios sin leer el árbol
        let mut changed = false;

        ui.add_enabled_ui(has_selection, |ui| {
            ui.label(egui::RichText::new("▼ Forma y Texto").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
            ui.add_space(8.0);

            if let Some(node) = Self::get_node_mut_safe(&mut self.root_tree, &self.selected_path) {
                // Relleno
                ui.horizontal(|ui| {
                    ui.label("Relleno:");
                    let mut color = node.custom_fill.unwrap_or(Color32::from_rgb(60, 60, 60));
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        node.custom_fill = Some(color);
                        changed = true; // Marcamos que hubo cambio
                    }
                });

                ui.add_space(8.0);

                // Texto
                ui.horizontal(|ui| {
                    ui.label("Tamaño:");
                    ui.add(egui::DragValue::new(&mut 14.0).speed(1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Estilo:");
                    let is_bold = node.title.starts_with("**") && node.title.ends_with("**");
                    let is_italic = node.title.starts_with("*") && node.title.ends_with("*") && !is_bold;

                    if ui.selectable_label(is_bold, egui::RichText::new("B").strong()).clicked() {
                        if is_bold {
                            node.title = node.title[2..node.title.len()-2].to_string();
                        } else {
                            node.title = format!("**{}**", node.title);
                        }
                        changed = true;
                    }
                    if ui.selectable_label(is_italic, egui::RichText::new("I").italics()).clicked() {
                        if is_italic {
                            node.title = node.title[1..node.title.len()-1].to_string();
                        } else {
                            node.title = format!("*{}*", node.title);
                        }
                        changed = true;
                    }
                    let _ = ui.selectable_label(false, egui::RichText::new("S").strikethrough());
                    let _ = ui.selectable_label(false, egui::RichText::new("U").underline());
                });

                ui.add_space(12.0);
                if ui.button("Restablecer estilo").clicked() {
                    node.custom_fill = None;
                    node.dir_override = NodeDirectionOverride::Auto;
                    changed = true;
                }
            }
        });

        // 2. Actualizamos el Markdown aquí, cuando el 'node' ya liberó el árbol
        if changed {
            self.md_text = tree_to_markdown(&self.root_tree);
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // Sección: Ramificación
        ui.label(egui::RichText::new("▼ Ramificación").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
        ui.add_space(8.0);

        egui::ComboBox::from_id_source("style_edge_style")
        .width(ui.available_width())
        .selected_text(self.renderer.edge_style.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut self.renderer.edge_style, EdgeStyle::Bezier, EdgeStyle::Bezier.label());
            ui.selectable_value(&mut self.renderer.edge_style, EdgeStyle::Orthogonal, EdgeStyle::Orthogonal.label());
        });

        ui.add_space(6.0);
        ui.checkbox(&mut self.renderer.show_arrows, "Punta de flecha");
        ui.checkbox(&mut self.renderer.branch_colors, "Ramificación con color");
    }

    fn render_tab_presentation(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Mostrar en modo Presentación").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
        ui.add_space(10.0);

        // Simulador de miniatura
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, egui::Rounding::same(8.0), Color32::BLACK);
        ui.painter().text(
            rect.center(),
                          Align2::CENTER_CENTER,
                          &self.document_title,
                          FontId::proportional(18.0),
                          Color32::WHITE
        );

        ui.add_space(10.0);
        if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Cambiar tema")).clicked() {
            // Lógica futura
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Mantener relación");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_source("pres_rel").selected_text("Automático").show_ui(ui, |ui| { ui.label("Opciones"); });
            });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Diapositiva de temas");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_source("pres_slide").selected_text("Automático").show_ui(ui, |ui| { ui.label("Opciones"); });
            });
        });

        ui.add_space(8.0);
        ui.label("Entrega");
        egui::ComboBox::from_id_source("pres_delivery")
        .width(ui.available_width())
        .selected_text("Una a una: práctica")
        .show_ui(ui, |ui| { ui.label("Opciones"); });
    }

    fn render_tab_map(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);

        // Preview de la estructura (Simplificado)
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, egui::Rounding::same(6.0), Color32::from_rgb(50, 50, 50));
        ui.painter().text(
            rect.center(),
                          Align2::CENTER_CENTER,
                          self.renderer.chart_type.label(),
                          FontId::proportional(14.0),
                          Color32::WHITE
        );

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Tema de colores").size(12.0).color(Color32::from_rgb(180, 180, 180)));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_source("map_theme")
            .width(ui.available_width() - 30.0)
            .selected_text("Zen")
            .show_ui(ui, |ui| { ui.label("Opciones"); });
            if ui.button("+").clicked() {}
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // Opciones globales
        ui.label(egui::RichText::new("Fuente general").size(12.0).color(Color32::from_rgb(180, 180, 180)));
        egui::ComboBox::from_id_source("map_font").width(ui.available_width()).selected_text("Predeterminado").show_ui(ui, |ui| { ui.label("Opciones"); });

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Ancho de línea de ramificación").size(12.0).color(Color32::from_rgb(180, 180, 180)));
        egui::ComboBox::from_id_source("map_line_width").width(ui.available_width()).selected_text("Predeterminado").show_ui(ui, |ui| { ui.label("Opciones"); });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        ui.label(egui::RichText::new("▼ Estilo de mapa").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
        ui.add_space(6.0);
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Equilibrio automático de mapa"));
        ui.horizontal(|ui| {
            let mut c = false;
            ui.checkbox(&mut c, "Mapa compacto");
            ui.label(egui::RichText::new("PRO").size(9.0).color(Color32::BLACK).background_color(Color32::from_rgb(200, 200, 200)));
        });
        ui.checkbox(&mut true, "Justificar alineación del tema");
        ui.checkbox(&mut false, "Mostrar todas las notas");

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        ui.label(egui::RichText::new("▼ Diseño avanzado").size(12.5).strong().color(Color32::from_rgb(180, 180, 180)));
        ui.add_space(6.0);
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Posición libre de ramificación"));
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Tema flotante flexible"));
        ui.checkbox(&mut true, "Superposición de tema");
    }

    // ==========================================
    // Pantalla 2: Editor de Mapa Mental Estilo XMind
    // ==========================================
    fn render_editor_screen(&mut self, ctx: &egui::Context) {
        // 1. Barra superior minimalista estilo XMind
        egui::TopBottomPanel::top("xmind_top_bar")
        .exact_height(42.0)
        .frame(
            egui::Frame::none()
            .fill(Color32::WHITE)
            .inner_margin(egui::Margin::symmetric(12.0, 6.0))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(229, 231, 235))),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("🏠 Inicio").color(Color32::from_rgb(31, 41, 55)))
                    .clicked()
                    {
                        self.screen = AppScreen::Welcome;
                    }

                    ui.separator();
                ui.label(
                    egui::RichText::new(&self.document_title)
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(17, 24, 39)),
                );

                ui.separator();

                if ui.button("+ Subtema").clicked() {
                    self.add_child_to_selected();
                }
                if ui.button("+ Hermano").clicked() {
                    self.add_sibling_to_selected();
                }
                if !self.selected_path.is_empty() && ui.button("🗑 Eliminar").clicked() {
                    self.delete_selected_node();
                }

                ui.separator();
                let rel_armed = self.relation_pending;
                if ui.selectable_label(rel_armed, "⤿ Relación")
                    .on_hover_text("Modo relación: clic en nodo origen, luego en nodo destino")
                    .clicked()
                    {
                        self.relation_pending = !self.relation_pending;
                        self.relation_source = None;
                        self.summary_pending = false;
                        self.summary_source = None;
                    }
                    let sum_armed = self.summary_pending;
                if ui.selectable_label(sum_armed, "⊞ Resumen")
                    .on_hover_text("Modo resumen: clic en primer nodo, luego en último (mismo padre)")
                    .clicked()
                    {
                        self.summary_pending = !self.summary_pending;
                        self.summary_source = None;
                        self.relation_pending = false;
                        self.relation_source = None;
                    }

                ui.separator();
                ui.label(
                    egui::RichText::new("Texto:")
                    .size(12.5)
                    .color(Color32::from_rgb(107, 114, 128)),
                );
                let text_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.edit_buffer).desired_width(210.0),
                );
                if text_resp.gained_focus() {
                    self.editing_node = true;
                }
                if (text_resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    && self.editing_node
                    {
                        self.apply_edit_buffer();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let insp_label = if self.show_inspector {
                            "Ocultar Formato"
                        } else {
                            "Formato"
                        };
                        if ui.button(insp_label).clicked() {
                            self.show_inspector = !self.show_inspector;
                        }

                        let md_label = if self.show_md_editor {
                            "Ocultar Markdown"
                        } else {
                            "Markdown"
                        };
                        if ui.button(md_label).clicked() {
                            self.show_md_editor = !self.show_md_editor;
                        }

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Exportar .xmind").color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(239, 68, 68))
                                .rounding(Rounding::same(6.0)),
                            )
                            .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("XMind", &["xmind"])
                                    .set_file_name(&format!("{}.xmind", self.document_title))
                                    .save_file()
                                    {
                                        let mut path_str = path.to_string_lossy().to_string();
                                        if !path_str.ends_with(".xmind") {
                                            path_str.push_str(".xmind");
                                        }
                                        let _ = export_xmind(&self.root_tree, &path_str);
                                    }
                            }
                    });
            });
        });

        // 2. Barra inferior estilo XMind (Conteo de temas y Zoom)
        egui::TopBottomPanel::bottom("xmind_bottom_bar")
        .exact_height(28.0)
        .frame(
            egui::Frame::none()
            .fill(Color32::WHITE)
            .inner_margin(egui::Margin::symmetric(14.0, 4.0))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(229, 231, 235))),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Mind Map")
                    .size(12.0)
                    .strong()
                    .color(Color32::from_rgb(55, 65, 81)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Centrar").clicked() {
                        self.renderer.zoom = 1.0;
                        self.renderer.pan = Vec2::ZERO;
                    }
                    if ui.small_button("+").clicked() {
                        self.renderer.zoom = (self.renderer.zoom * 1.1).min(3.0);
                    }
                    ui.label(
                        egui::RichText::new(format!("{:.0}%", self.renderer.zoom * 100.0))
                        .size(12.0)
                        .color(Color32::from_rgb(55, 65, 81)),
                    );
                    if ui.small_button("-").clicked() {
                        self.renderer.zoom = (self.renderer.zoom * 0.9).max(0.2);
                    }

                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Temas: {}", self.root_tree.count_nodes()))
                        .size(12.0)
                        .color(Color32::from_rgb(107, 114, 128)),
                    );
                });
            });
        });

        // 3. Panel lateral izquierdo: Editor Markdown
        if self.show_md_editor {
            egui::SidePanel::left("xmind_md_panel")
            .resizable(true)
            .default_width(310.0)
            .frame(
                egui::Frame::none()
                .fill(Color32::from_rgb(249, 250, 251))
                .inner_margin(egui::Margin::same(10.0))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(229, 231, 235))),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Esquema Markdown")
                    .size(12.5)
                    .strong()
                    .color(Color32::from_rgb(75, 85, 99)),
                );
                ui.add_space(6.0);
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
        }

        // 4. Panel Inspector Derecho Oscuro (estilo Xmind)
        if self.show_inspector {
            egui::SidePanel::right("xmind_inspector")
            .resizable(false)
            .exact_width(260.0)
            .frame(
                egui::Frame::none()
                .fill(Color32::from_rgb(35, 35, 35)) // Fondo oscuro profundo de Xmind
                .inner_margin(egui::Margin::same(0.0)), // Sin margen para que las pestañas peguen al borde
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(220, 220, 220));
                ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_rgb(55, 55, 55);
                ui.visuals_mut().widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 70);
                ui.visuals_mut().widgets.active.bg_fill = Color32::from_rgb(80, 80, 80);

                // --- Pestañas Superiores (Tabs) ---
                egui::Frame::none()
                .fill(Color32::from_rgb(45, 45, 45))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        let tabs = [
                            (InspectorTab::Style, "Estilo"),
                                  (InspectorTab::Presentation, "Presentación"),
                                  (InspectorTab::Map, "Mapa"),
                        ];

                        for (tab, label) in tabs {
                            let is_active = self.inspector_tab == tab;
                            let bg = if is_active { Color32::from_rgb(75, 75, 75) } else { Color32::TRANSPARENT };
                            let text_col = if is_active { Color32::WHITE } else { Color32::from_rgb(150, 150, 150) };

                            // Botón custom para simular pestaña
                            let btn = egui::Button::new(egui::RichText::new(label).size(12.5).color(text_col))
                            .fill(bg)
                            .rounding(egui::Rounding::same(6.0))
                            .min_size(egui::vec2(70.0, 28.0));

                            if ui.add(btn).clicked() {
                                self.inspector_tab = tab;
                            }
                        }
                    });
                });

                // --- Contenido de la Pestaña ---
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Frame::none()
                    .inner_margin(egui::Margin::same(16.0))
                    .show(ui, |ui| {
                        match self.inspector_tab {
                            InspectorTab::Style => self.render_tab_style(ui),
                          InspectorTab::Presentation => self.render_tab_presentation(ui),
                          InspectorTab::Map => self.render_tab_map(ui),
                        }
                    });
                });
            });
        }

        // 5. Lienzo Central Claro estilo XMind
        egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(Color32::from_rgb(248, 249, 251)))
        .show(ctx, |ui| {
            let canvas_rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());

            // Pan con clic sostenido (mouse)
            if response.dragged() {
                self.renderer.pan += response.drag_delta();
            }

            let pointer_pos = response.hover_pos().or_else(|| response.interact_pointer_pos());

            // Pellizco táctil / zoom con 2 dedos del touchpad
            if let Some(mt) = ui.input(|i| i.multi_touch()) {
                if (mt.zoom_delta - 1.0).abs() > 0.002 {
                    if let Some(p) = pointer_pos {
                        self.zoom_at(canvas_rect, p, mt.zoom_delta);
                    }
                }
            }

            let smooth = ui.input(|i| i.smooth_scroll_delta);
            if ui.input(|i| i.modifiers.ctrl) {
                // Ctrl + rueda (o pellizco sintetizado por el touchpad) = zoom al cursor
                if smooth.y != 0.0 {
                    if let Some(p) = pointer_pos {
                        self.zoom_at(canvas_rect, p, if smooth.y > 0.0 { 1.1 } else { 0.9 });
                    }
                }
            } else if smooth != Vec2::ZERO {
                // Dos dedos en touchpad = paneo en TODAS direcciones, solo lienzo
                self.renderer.pan += smooth;
            }

            // Tecla Delete: borra relación seleccionada o nodo seleccionado
            if !ctx.wants_keyboard_input()
                && ui.input(|i| i.key_pressed(egui::Key::Delete))
                {
                    if let Some(r) = self.selected_relation.take() {
                        self.relations.remove(r);
                    } else {
                        self.delete_selected_node();
                    }
                    self.sanitize_links();
                }

            self.renderer.measure_tree(ui, &mut self.root_tree, 0);
            self.renderer.layout_tree(&mut self.root_tree);

            let offset = canvas_rect.center().to_vec2() + self.renderer.pan;

            // Mapa path -> rect en pantalla (para relaciones, resúmenes y selección)
            let mut rect_map: HashMap<Vec<usize>, Rect> = HashMap::new();
            let mut rect_path = Vec::new();
            collect_rects(&self.root_tree, offset, self.renderer.zoom, &mut rect_path, &mut rect_map);

            if response.clicked() || response.double_clicked() {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    let mut path_buf = Vec::new();
                    let hit = MindMapRenderer::hit_test(
                        &mut self.root_tree,
                        offset,
                        self.renderer.zoom,
                        pointer_pos,
                        &mut path_buf,
                    );

                    // --- Modo RELACIÓN ---
                    if self.relation_pending {
                        if let Some((target, false)) = &hit {
                            match &self.relation_source {
                                None => self.relation_source = Some(target.clone()),
              Some(src) => {
                  if src != target {
                      self.relations.push(Relation {
                          from_path: src.clone(),
                                          to_path: target.clone(),
                                          label: "Relación".to_string(),
                      });
                      self.selected_relation = Some(self.relations.len() - 1);
                      self.selected_path.clear();
                  }
                  self.relation_pending = false;
                  self.relation_source = None;
              }
                            }
                        } else {
                            self.relation_pending = false;
                            self.relation_source = None;
                        }
                        // --- Modo RESUMEN ---
                    } else if self.summary_pending {
                        if let Some((target, false)) = &hit {
                            match &self.summary_source {
                                None => self.summary_source = Some(target.clone()),
              Some(src) => {
                  let same_parent = src.len() == target.len()
                  && src.len() >= 1
                  && src[..src.len() - 1] == target[..target.len() - 1];
                  if same_parent {
                      let parent_path = &src[..src.len() - 1];
                      let a = *src.last().unwrap();
                      let b = *target.last().unwrap();
                      let (start, end) = if a < b { (a, b) } else { (b, a) };
                      if let Some(parent) =
                          Self::get_node_mut_safe(&mut self.root_tree, parent_path)
                          {
                              let node_idx = parent.children.len();
                              parent.children.push(TreeNode::new("Resumen".to_string()));
                              self.summaries.push(Summary {
                                  parent_path: parent_path.to_vec(),
                                                  start,
                                                  end,
                                                  node_idx,
                              });
                              let mut p = parent_path.to_vec();
                              p.push(node_idx);
                              self.selected_path = p;
                              self.sync_edit_buffer();
                              self.editing_node = true;
                              self.md_text = tree_to_markdown(&self.root_tree);
                          }
                  }
                  self.summary_pending = false;
                  self.summary_source = None;
              }
                            }
                        } else {
                            self.summary_pending = false;
                            self.summary_source = None;
                        }
                        // --- Selección normal ---
                    } else if let Some((hit_path, is_toggle)) = hit {
                        if is_toggle {
                            if let Some(node) =
                                Self::get_node_mut_safe(&mut self.root_tree, &hit_path)
                                {
                                    node.collapsed = !node.collapsed;
                                }
                        } else {
                            self.selected_path = hit_path;
                            self.selected_relation = None;
                            self.sync_edit_buffer();
                            if response.double_clicked() {
                                self.editing_node = true;
                            }
                        }
                    } else {
                        // ¿Clic sobre una relación? (distancia a su línea punteada)
                        let mut found = None;
                        for (i, rel) in self.relations.iter().enumerate() {
                            if let (Some(ra), Some(rb)) =
                                (rect_map.get(&rel.from_path), rect_map.get(&rel.to_path))
                                {
                                    let s = rect_edge_toward(*ra, rb.center());
                                    let e = rect_edge_toward(*rb, ra.center());
                                    if dist_point_segment(pointer_pos, s, e) < 8.0 {
                                        found = Some(i);
                                        break;
                                    }
                                }
                        }
                        self.selected_relation = found;
                        if found.is_none() {
                            self.selected_path.clear();
                            self.sync_edit_buffer();
                        }
                    }
                }
            }

            let painter = ui.painter_at(canvas_rect);
            self.renderer.draw_edges(&painter, &self.root_tree, offset, true, 0);
            draw_relations(&painter, &self.relations, self.selected_relation, &rect_map);

            let mut draw_path = Vec::new();
            self.renderer.draw_nodes(
                &painter,
                ui,
                &mut self.root_tree,
                offset,
                0,
                0,
                &self.selected_path,
                &mut draw_path,
            );

            // Los resúmenes se dibujan ENCIMA (bracket visible)
            draw_summaries(&painter, &self.summaries, &rect_map, self.renderer.zoom);

            // Highlight del origen pendiente (modo relación / resumen)
            let pending = if self.relation_pending {
                &self.relation_source
            } else if self.summary_pending {
                &self.summary_source
            } else {
                &None
            };
            if let Some(src) = pending {
                if let Some(r) = rect_map.get(src) {
                    let col = if self.relation_pending {
                        Color32::from_rgb(56, 189, 248)
                    } else {
                        Color32::from_rgb(245, 158, 11)
                    };
                    painter.rect_stroke(r.expand(4.0), 6.0, Stroke::new(2.5_f32, col));
                }
            }
        });
    }
}

impl eframe::App for MindMapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.screen {
            AppScreen::Welcome => self.render_welcome_screen(ctx),
            AppScreen::Editor => self.render_editor_screen(ctx),
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 840.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MindCraft",
        options,
        Box::new(|_| Box::new(MindMapApp::default())),
    )
}

fn rect_edge_toward(rect: Rect, toward: Pos2) -> Pos2 {
    let c = rect.center();
    let d = toward - c;
    if d.x.abs() >= d.y.abs() {
        if d.x >= 0.0 {
            egui::pos2(rect.right(), c.y)
        } else {
            egui::pos2(rect.left(), c.y)
        }
    } else if d.y >= 0.0 {
        egui::pos2(c.x, rect.bottom())
    } else {
        egui::pos2(c.x, rect.top())
    }
}

fn draw_dashed(
    painter: &egui::Painter,
    p0: Pos2,
    p1: Pos2,
    stroke: Stroke,
    dash: f32,
    gap: f32,
) {
    let v = p1 - p0;
    let len = v.length();
    if len < 1.0 {
        return;
    }
    let dir = v / len;
    let mut d = 0.0;
    while d < len {
        let a = p0 + dir * d;
        let b = p0 + dir * (d + dash).min(len);
        painter.line_segment([a, b], stroke);
        d += dash + gap;
    }
}

fn dist_point_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-6 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn collect_rects(
    node: &TreeNode,
    offset: Vec2,
    zoom: f32,
    path: &mut Vec<usize>,
    map: &mut HashMap<Vec<usize>, Rect>,
) {
    let center = (offset + node.pos.to_vec2() * zoom).to_pos2();
    map.insert(path.clone(), Rect::from_center_size(center, node.size * zoom));
    if !node.collapsed {
        for (i, c) in node.children.iter().enumerate() {
            path.push(i);
            collect_rects(c, offset, zoom, path, map);
            path.pop();
        }
    }
}

fn draw_relations(
    painter: &egui::Painter,
    relations: &[Relation],
    selected: Option<usize>,
    rects: &HashMap<Vec<usize>, Rect>,
) {
    let color = Color32::from_rgb(56, 189, 248); // celeste
    for (i, rel) in relations.iter().enumerate() {
        let (ra, rb) = match (rects.get(&rel.from_path), rects.get(&rel.to_path)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => continue,
        };
        let start = rect_edge_toward(ra, rb.center());
        let end = rect_edge_toward(rb, ra.center());

        let stroke = Stroke::new(
            if selected == Some(i) { 3.0 } else { 2.0_f32 },
                if selected == Some(i) { Color32::from_rgb(2, 132, 199) } else { color },
        );
        draw_dashed(painter, start, end, stroke, 8.0, 5.0);

        // Etiqueta en el punto medio
        let mid = start + (end - start) * 0.5;
        let galley = painter.layout_no_wrap(
            rel.label.clone(),
                                            FontId::proportional(12.0),
                                            Color32::from_rgb(2, 132, 199),
        );
        let label_rect = Rect::from_center_size(mid - Vec2::new(0.0, 14.0), galley.size() + Vec2::splat(8.0));
        painter.rect_filled(label_rect, 4.0, Color32::WHITE);
        painter.rect_stroke(label_rect, 4.0, Stroke::new(1.0_f32, color));
        painter.galley(label_rect.left_top() + Vec2::splat(4.0), galley, Color32::from_rgb(2, 132, 199));
    }
}

fn draw_summaries(
    painter: &egui::Painter,
    summaries: &[Summary],
    rects: &HashMap<Vec<usize>, Rect>,
    zoom: f32,
) {
    let stroke = Stroke::new(1.5_f32, Color32::from_rgb(107, 114, 128));
    for s in summaries {
        let mut bmin = Pos2::new(f32::MAX, f32::MAX);
        let mut bmax = Pos2::new(f32::MIN, f32::MIN);
        let mut ok = true;

        for i in s.start..=s.end {
            let mut key = s.parent_path.clone();
            key.push(i);
            match rects.get(&key) {
                Some(r) => {
                    bmin = bmin.min(r.min);
                    bmax = bmax.max(r.max);
                }
                None => {
                    ok = false;
                    break;
                }
            }
        } // <-- Cierra el ciclo interno 'for i'

        if !ok {
            continue;
        }

        let mut key = s.parent_path.clone();
        key.push(s.node_idx);

        let Some(sn_rect) = rects.get(&key) else {
            continue;
        };

        let bracket = Rect::from_min_max(bmin, bmax).expand(12.0 * zoom);
        painter.rect_stroke(bracket, 8.0 * zoom, stroke);

        let from = rect_edge_toward(bracket, sn_rect.center());
        let to = rect_edge_toward(*sn_rect, bracket.center());
        painter.line_segment([from, to], stroke);
    } // <-- Cierra el ciclo 'for s in summaries'
} // <-- Cierra la función draw_summaries
