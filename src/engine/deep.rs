use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use super::types::{
    CfgEdge, CfgNode, CfgPayload, DfgEdge, DfgNode, DfgPayload, ProgramSliceLine,
    ProgramSlicePayload,
};
use super::util::{load_bake_index, resolve_project_root};

#[derive(Clone, Debug)]
struct LineInfo {
    line: u32,
    depth: u32,
    kind: String,
    text: String,
    cleaned: String,
    defs: Vec<String>,
    uses: Vec<String>,
}

struct DfgBuild {
    nodes: Vec<DfgNode>,
    edges: Vec<DfgEdge>,
    unresolved_inputs: Vec<String>,
}

/// Public entrypoint for the `cfg` tool: initial control-flow graph for Rust/Go.
pub fn cfg(path: Option<String>, file: String, function: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake =
        load_bake_index(&root)?.ok_or_else(|| anyhow!("No bake index found. Run `bake` first."))?;

    let target = resolve_function(&bake, &root, &file, &function)?;
    let language = target.language.to_lowercase();

    if !is_supported_language(&language) {
        let payload = CfgPayload {
            tool: "cfg",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            file: target.file.clone(),
            symbol: target.name.clone(),
            language: target.language.clone(),
            supported: false,
            reason: Some(
                "cfg currently supports Rust and Go only. Other languages need richer statement-level indexing."
                    .to_string(),
            ),
            alternatives: vec![
                "supersearch with context=identifiers and pattern=call".to_string(),
                "flow for endpoint-rooted call-chain exploration".to_string(),
                "symbol + include_source=true for manual tracing".to_string(),
            ],
            entry_line: target.start_line,
            exit_lines: vec![target.end_line],
            nodes: vec![],
            edges: vec![],
            summary: format!(
                "CFG is unavailable for {} in {}.",
                target.language, target.file
            ),
        };
        return Ok(serde_json::to_string_pretty(&payload)?);
    }

    let line_infos = build_line_infos(&root, target)?;
    let (nodes, edges, entry_line, exit_lines) = build_cfg(&line_infos);
    let node_count = nodes.len();
    let edge_count = edges.len();

    let payload = CfgPayload {
        tool: "cfg",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file: target.file.clone(),
        symbol: target.name.clone(),
        language: target.language.clone(),
        supported: true,
        reason: None,
        alternatives: vec![],
        entry_line,
        exit_lines,
        nodes,
        edges,
        summary: format!(
            "Built CFG for {} ({}): {} node(s), {} edge(s).",
            target.name, target.file, node_count, edge_count
        ),
    };

    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Public entrypoint for the `dfg` tool: initial data-flow graph for Rust/Go.
pub fn dfg(path: Option<String>, file: String, function: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake =
        load_bake_index(&root)?.ok_or_else(|| anyhow!("No bake index found. Run `bake` first."))?;

    let target = resolve_function(&bake, &root, &file, &function)?;
    let language = target.language.to_lowercase();

    if !is_supported_language(&language) {
        let payload = DfgPayload {
            tool: "dfg",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            file: target.file.clone(),
            symbol: target.name.clone(),
            language: target.language.clone(),
            supported: false,
            reason: Some(
                "dfg currently supports Rust and Go only. Other languages need richer assignment/use extraction."
                    .to_string(),
            ),
            alternatives: vec![
                "program_slice once language support lands".to_string(),
                "symbol + include_source=true for manual dependency tracing".to_string(),
            ],
            nodes: vec![],
            edges: vec![],
            unresolved_inputs: vec![],
            summary: format!(
                "DFG is unavailable for {} in {}.",
                target.language, target.file
            ),
        };
        return Ok(serde_json::to_string_pretty(&payload)?);
    }

    let line_infos = build_line_infos(&root, target)?;
    let built = build_dfg(&line_infos, &language);
    let node_count = built.nodes.len();
    let edge_count = built.edges.len();

    let payload = DfgPayload {
        tool: "dfg",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file: target.file.clone(),
        symbol: target.name.clone(),
        language: target.language.clone(),
        supported: true,
        reason: None,
        alternatives: vec![],
        nodes: built.nodes,
        edges: built.edges,
        unresolved_inputs: built.unresolved_inputs,
        summary: format!(
            "Built DFG for {} ({}): {} node(s), {} edge(s).",
            target.name, target.file, node_count, edge_count
        ),
    };

    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Public entrypoint for the `program_slice` tool: dependency-aware backward slice.
pub fn program_slice(
    path: Option<String>,
    file: String,
    function: String,
    line: u32,
) -> Result<String> {
    let root = resolve_project_root(path)?;
    let bake =
        load_bake_index(&root)?.ok_or_else(|| anyhow!("No bake index found. Run `bake` first."))?;

    let target = resolve_function(&bake, &root, &file, &function)?;
    let language = target.language.to_lowercase();

    if !is_supported_language(&language) {
        let payload = ProgramSlicePayload {
            tool: "program_slice",
            version: env!("CARGO_PKG_VERSION"),
            project_root: root,
            file: target.file.clone(),
            symbol: target.name.clone(),
            language: target.language.clone(),
            line,
            supported: false,
            reason: Some("program_slice currently supports Rust and Go only.".to_string()),
            alternatives: vec![
                "dfg for line-level variable dependencies".to_string(),
                "cfg for control-flow shape".to_string(),
                "symbol + slice for manual tracing".to_string(),
            ],
            seed_variables: vec![],
            control_dependencies: vec![],
            data_dependencies: vec![],
            lines: vec![],
            summary: format!(
                "Program slice is unavailable for {} in {}.",
                target.language, target.file
            ),
        };
        return Ok(serde_json::to_string_pretty(&payload)?);
    }

    if line < target.start_line || line > target.end_line {
        return Err(anyhow!(
            "Line {} is outside function '{}' range ({}..={}).",
            line,
            target.name,
            target.start_line,
            target.end_line
        ));
    }

    let line_infos = build_line_infos(&root, target)?;
    let line_lookup: HashMap<u32, &LineInfo> = line_infos.iter().map(|l| (l.line, l)).collect();
    if !line_lookup.contains_key(&line) {
        return Err(anyhow!(
            "Line {} is not part of parsed lines for function '{}' in {}.",
            line,
            target.name,
            target.file
        ));
    }

    let built = build_dfg(&line_infos, &language);
    let incoming = incoming_edges(&built.edges);
    let seed_variables = line_lookup
        .get(&line)
        .map(|l| {
            let mut s = BTreeSet::new();
            for v in &l.uses {
                s.insert(v.clone());
            }
            for v in &l.defs {
                s.insert(v.clone());
            }
            s.into_iter().collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut selected_lines: HashSet<u32> = HashSet::from([line]);
    let mut selected_edges: Vec<DfgEdge> = Vec::new();
    let mut seen_edges: HashSet<(String, u32, u32)> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::from([line]);

    while let Some(current) = queue.pop_front() {
        if let Some(edges) = incoming.get(&current) {
            for edge in edges {
                let edge_key = (edge.variable.clone(), edge.from_line, edge.to_line);
                if seen_edges.insert(edge_key) {
                    selected_edges.push((*edge).clone());
                }
                if selected_lines.insert(edge.from_line) {
                    queue.push_back(edge.from_line);
                }
            }
        }
    }

    let mut control_dependencies: BTreeSet<u32> = BTreeSet::new();
    for selected in selected_lines.clone() {
        if let Some(ctrl) = nearest_control_dependency(&line_infos, selected) {
            control_dependencies.insert(ctrl);
            selected_lines.insert(ctrl);
        }
    }

    if let Some(entry_line) = line_infos.first().map(|l| l.line) {
        selected_lines.insert(entry_line);
    }

    let selected_edge_lines: HashSet<u32> = selected_edges
        .iter()
        .flat_map(|e| [e.from_line, e.to_line])
        .collect();

    let lines = line_infos
        .iter()
        .filter(|l| selected_lines.contains(&l.line))
        .map(|l| {
            let kind = if l.line == line {
                "target"
            } else if control_dependencies.contains(&l.line) {
                "control"
            } else if selected_edge_lines.contains(&l.line) {
                "data"
            } else {
                "context"
            };
            ProgramSliceLine {
                line: l.line,
                kind: kind.to_string(),
                code: l.text.trim().to_string(),
            }
        })
        .collect::<Vec<_>>();
    let control_dependency_count = control_dependencies.len();

    let payload = ProgramSlicePayload {
        tool: "program_slice",
        version: env!("CARGO_PKG_VERSION"),
        project_root: root,
        file: target.file.clone(),
        symbol: target.name.clone(),
        language: target.language.clone(),
        line,
        supported: true,
        reason: None,
        alternatives: vec![],
        seed_variables,
        control_dependencies: control_dependencies.into_iter().collect(),
        data_dependencies: selected_edges.clone(),
        lines,
        summary: format!(
            "Program slice for {}:{} in {} captured {} line(s), {} data edge(s), {} control dependency line(s).",
            target.name,
            line,
            target.file,
            selected_lines.len(),
            selected_edges.len(),
            control_dependency_count
        ),
    };

    Ok(serde_json::to_string_pretty(&payload)?)
}

fn resolve_function<'a>(
    bake: &'a super::types::BakeIndex,
    root: &Path,
    file: &str,
    function: &str,
) -> Result<&'a crate::lang::IndexedFunction> {
    let file_norm = normalize_file_arg(root, file).to_lowercase();
    let needle = function.to_lowercase();

    let mut candidates: Vec<&crate::lang::IndexedFunction> = bake
        .functions
        .iter()
        .filter(|f| f.name.to_lowercase() == needle)
        .filter(|f| {
            let fpath = f.file.to_lowercase();
            fpath == file_norm || fpath.ends_with(&file_norm) || file_norm.ends_with(&fpath)
        })
        .collect();

    if candidates.is_empty() {
        return Err(anyhow!(
            "Function '{}' not found in file '{}'. Run `bake` and verify the exact function/file.",
            function,
            file
        ));
    }

    candidates.sort_by(|a, b| a.start_line.cmp(&b.start_line));
    Ok(candidates[0])
}

fn normalize_file_arg(root: &Path, file: &str) -> String {
    let raw = PathBuf::from(file);
    let rel = if raw.is_absolute() {
        raw.strip_prefix(root).unwrap_or(&raw).to_path_buf()
    } else {
        raw
    };
    rel.to_string_lossy().replace('\\', "/")
}

fn is_supported_language(lang: &str) -> bool {
    matches!(lang, "rust" | "go")
}

fn build_line_infos(root: &Path, func: &crate::lang::IndexedFunction) -> Result<Vec<LineInfo>> {
    let full_path = root.join(&func.file);
    let content = fs::read_to_string(&full_path)
        .map_err(|e| anyhow!("Failed to read {}: {}", full_path.display(), e))?;
    let all_lines: Vec<&str> = content.lines().collect();
    if all_lines.is_empty() {
        return Ok(vec![]);
    }

    let start = func.start_line.saturating_sub(1) as usize;
    let end = func.end_line.min(all_lines.len() as u32) as usize;
    if start >= end || start >= all_lines.len() {
        return Ok(vec![]);
    }

    let language = func.language.to_lowercase();
    let mut depth = 0i32;
    let mut out: Vec<LineInfo> = Vec::new();

    for idx in start..end {
        let line_no = idx as u32 + 1;
        let raw = all_lines[idx].to_string();
        let cleaned = sanitize_for_analysis(&raw);
        let trimmed = raw.trim();
        let depth_before = depth.max(0) as u32;
        let opens = cleaned.matches('{').count() as i32;
        let closes = cleaned.matches('}').count() as i32;
        depth += opens - closes;
        if depth < 0 {
            depth = 0;
        }

        let kind = classify_line_kind(trimmed, &language).to_string();
        let (defs, uses) = extract_defs_uses(&cleaned, &language);

        out.push(LineInfo {
            line: line_no,
            depth: depth_before,
            kind,
            text: raw,
            cleaned,
            defs,
            uses,
        });
    }

    Ok(out)
}

fn build_cfg(line_infos: &[LineInfo]) -> (Vec<CfgNode>, Vec<CfgEdge>, u32, Vec<u32>) {
    let mut nodes: Vec<CfgNode> = Vec::new();
    for li in line_infos {
        let trimmed = li.text.trim();
        if trimmed.is_empty() || is_trivial_brace_line(trimmed) {
            continue;
        }
        nodes.push(CfgNode {
            id: nodes.len() as u32 + 1,
            line: li.line,
            kind: li.kind.clone(),
            text: trimmed.to_string(),
        });
    }

    if nodes.is_empty() {
        return (vec![], vec![], 0, vec![]);
    }

    let depth_map: HashMap<u32, u32> = line_infos.iter().map(|l| (l.line, l.depth)).collect();

    let mut edges: Vec<CfgEdge> = Vec::new();
    let mut seen: HashSet<(u32, u32, String)> = HashSet::new();

    for pair in nodes.windows(2) {
        let from = &pair[0];
        let to = &pair[1];
        if from.kind == "return" || from.kind == "jump" {
            continue;
        }
        push_cfg_edge(
            &mut edges,
            &mut seen,
            from.id,
            to.id,
            "fallthrough".to_string(),
        );
    }

    for i in 0..nodes.len() {
        let kind = nodes[i].kind.as_str();
        if kind != "branch" && kind != "loop" {
            continue;
        }

        if i + 1 < nodes.len() {
            let edge_kind = if kind == "loop" { "loop_body" } else { "true" };
            push_cfg_edge(
                &mut edges,
                &mut seen,
                nodes[i].id,
                nodes[i + 1].id,
                edge_kind.to_string(),
            );
        }

        let depth = depth_map.get(&nodes[i].line).copied().unwrap_or(0);
        if let Some(exit_idx) = find_exit_index(&nodes, i, depth, &depth_map) {
            if exit_idx > i {
                let edge_kind = if kind == "loop" { "loop_exit" } else { "false" };
                push_cfg_edge(
                    &mut edges,
                    &mut seen,
                    nodes[i].id,
                    nodes[exit_idx].id,
                    edge_kind.to_string(),
                );

                if kind == "loop" && exit_idx > i + 1 {
                    if let Some(src_idx) =
                        find_loop_back_source(&nodes, i + 1, exit_idx, depth, &depth_map)
                    {
                        push_cfg_edge(
                            &mut edges,
                            &mut seen,
                            nodes[src_idx].id,
                            nodes[i].id,
                            "loop_back".to_string(),
                        );
                    }
                }
            }
        }
    }

    let mut exits: BTreeSet<u32> = BTreeSet::new();
    for node in &nodes {
        if node.kind == "return" || node.kind == "jump" {
            exits.insert(node.line);
        }
    }
    exits.insert(nodes.last().map(|n| n.line).unwrap_or(0));

    (
        nodes.clone(),
        edges,
        nodes.first().map(|n| n.line).unwrap_or(0),
        exits.into_iter().collect(),
    )
}

fn find_exit_index(
    nodes: &[CfgNode],
    start: usize,
    depth: u32,
    depth_map: &HashMap<u32, u32>,
) -> Option<usize> {
    for idx in start + 1..nodes.len() {
        let node = &nodes[idx];
        let node_depth = depth_map.get(&node.line).copied().unwrap_or(0);
        let text = node.text.trim_start();
        if node_depth == depth && text.starts_with("else") {
            return Some(idx);
        }
        if node_depth <= depth && !text.starts_with("else") {
            return Some(idx);
        }
    }
    None
}

fn find_loop_back_source(
    nodes: &[CfgNode],
    block_start: usize,
    block_end: usize,
    depth: u32,
    depth_map: &HashMap<u32, u32>,
) -> Option<usize> {
    for idx in (block_start..block_end).rev() {
        let node_depth = depth_map.get(&nodes[idx].line).copied().unwrap_or(0);
        if node_depth > depth {
            return Some(idx);
        }
    }
    None
}

fn push_cfg_edge(
    edges: &mut Vec<CfgEdge>,
    seen: &mut HashSet<(u32, u32, String)>,
    from: u32,
    to: u32,
    kind: String,
) {
    if from == to {
        return;
    }
    let key = (from, to, kind.clone());
    if seen.insert(key) {
        edges.push(CfgEdge { from, to, kind });
    }
}

fn build_dfg(line_infos: &[LineInfo], language: &str) -> DfgBuild {
    let params = extract_params(line_infos, language);
    let mut params_set: HashSet<String> = HashSet::new();
    for p in &params {
        params_set.insert(p.clone());
    }

    let entry_line = line_infos.first().map(|l| l.line).unwrap_or(0);
    let mut last_def: HashMap<String, u32> = HashMap::new();
    for p in &params {
        last_def.insert(p.clone(), entry_line);
    }

    let mut nodes: Vec<DfgNode> = Vec::new();
    let mut edges: Vec<DfgEdge> = Vec::new();
    let mut unresolved: BTreeSet<String> = BTreeSet::new();
    let mut seen_edges: HashSet<(String, u32, u32)> = HashSet::new();

    for li in line_infos {
        let trimmed = li.text.trim();
        if trimmed.is_empty() {
            continue;
        }

        nodes.push(DfgNode {
            line: li.line,
            kind: li.kind.clone(),
            code: trimmed.to_string(),
            defs: li.defs.clone(),
            uses: li.uses.clone(),
        });

        for used in &li.uses {
            if let Some(from_line) = last_def.get(used) {
                if *from_line != li.line {
                    let edge = DfgEdge {
                        variable: used.clone(),
                        from_line: *from_line,
                        to_line: li.line,
                    };
                    let key = (edge.variable.clone(), edge.from_line, edge.to_line);
                    if seen_edges.insert(key) {
                        edges.push(edge);
                    }
                }
            } else if !params_set.contains(used) {
                unresolved.insert(used.clone());
            }
        }

        for def in &li.defs {
            last_def.insert(def.clone(), li.line);
        }
    }

    DfgBuild {
        nodes,
        edges,
        unresolved_inputs: unresolved.into_iter().collect(),
    }
}

fn incoming_edges(edges: &[DfgEdge]) -> HashMap<u32, Vec<&DfgEdge>> {
    let mut incoming: HashMap<u32, Vec<&DfgEdge>> = HashMap::new();
    for edge in edges {
        incoming.entry(edge.to_line).or_default().push(edge);
    }
    incoming
}

fn nearest_control_dependency(line_infos: &[LineInfo], target_line: u32) -> Option<u32> {
    let target = line_infos.iter().find(|l| l.line == target_line)?;
    let mut candidate: Option<u32> = None;
    for li in line_infos {
        if li.line >= target_line {
            break;
        }
        if (li.kind == "branch" || li.kind == "loop") && li.depth < target.depth {
            candidate = Some(li.line);
        }
    }
    candidate
}

fn extract_params(line_infos: &[LineInfo], language: &str) -> Vec<String> {
    let mut signature = String::new();
    for li in line_infos.iter().take(8) {
        signature.push_str(&li.cleaned);
        signature.push(' ');
        if li.cleaned.contains('{') {
            break;
        }
    }

    let Some(open_idx) = signature.find('(') else {
        return vec![];
    };
    let Some(close_rel) = signature[open_idx + 1..].find(')') else {
        return vec![];
    };
    let close_idx = open_idx + 1 + close_rel;
    let params = &signature[open_idx + 1..close_idx];
    if params.trim().is_empty() {
        return vec![];
    }

    let mut out: BTreeSet<String> = BTreeSet::new();
    match language {
        "rust" => {
            for segment in params.split(',') {
                let left = segment.split(':').next().unwrap_or("").trim();
                for id in extract_idents(left, language) {
                    out.insert(id);
                }
            }
        }
        "go" => {
            for segment in params.split(',') {
                let segment = segment.trim();
                if segment.is_empty() {
                    continue;
                }
                let mut tokens: Vec<&str> = segment.split_whitespace().collect();
                if tokens.len() >= 2 {
                    tokens.pop();
                    for tok in tokens {
                        for id in extract_idents(tok, language) {
                            out.insert(id);
                        }
                    }
                } else {
                    for id in extract_idents(segment, language) {
                        out.insert(id);
                    }
                }
            }
        }
        _ => {}
    }
    out.into_iter().collect()
}

fn classify_line_kind(trimmed: &str, language: &str) -> &'static str {
    let t = trimmed.trim_start();
    if t.is_empty() {
        return "empty";
    }

    if t.starts_with("fn ")
        || t.starts_with("pub fn ")
        || t.starts_with("pub(crate) fn ")
        || t.starts_with("func ")
    {
        return "entry";
    }

    if t.starts_with("return ") || t == "return" {
        return "return";
    }

    if t.starts_with("break")
        || t.starts_with("continue")
        || (language == "go" && t.starts_with("goto "))
    {
        return "jump";
    }

    if is_loop_line(t, language) {
        return "loop";
    }

    if is_branch_line(t, language) {
        return "branch";
    }

    "stmt"
}

fn is_loop_line(trimmed: &str, language: &str) -> bool {
    match language {
        "rust" => {
            trimmed.starts_with("for ")
                || trimmed.starts_with("while ")
                || trimmed.starts_with("loop")
        }
        "go" => trimmed.starts_with("for "),
        _ => false,
    }
}

fn is_branch_line(trimmed: &str, language: &str) -> bool {
    match language {
        "rust" => {
            trimmed.starts_with("if ")
                || trimmed.starts_with("if(")
                || trimmed.starts_with("else if")
                || trimmed.starts_with("else {")
                || trimmed.starts_with("match ")
        }
        "go" => {
            trimmed.starts_with("if ")
                || trimmed.starts_with("if(")
                || trimmed.starts_with("else if")
                || trimmed.starts_with("else {")
                || trimmed.starts_with("switch ")
                || trimmed.starts_with("select ")
                || trimmed.starts_with("case ")
                || trimmed.starts_with("default:")
        }
        _ => false,
    }
}

fn is_trivial_brace_line(trimmed: &str) -> bool {
    let t = trimmed.trim();
    matches!(t, "{" | "}" | "};" | "},")
}

fn sanitize_for_analysis(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    while let Some(ch) = chars.next() {
        if in_backtick {
            if ch == '`' {
                in_backtick = false;
            }
            out.push(' ');
            continue;
        }

        if in_double {
            if escape {
                escape = false;
                out.push(' ');
                continue;
            }
            if ch == '\\' {
                escape = true;
                out.push(' ');
                continue;
            }
            if ch == '"' {
                in_double = false;
            }
            out.push(' ');
            continue;
        }

        if ch == '"' {
            in_double = true;
            out.push(' ');
            continue;
        }
        if ch == '`' {
            in_backtick = true;
            out.push(' ');
            continue;
        }

        if ch == '/' {
            if let Some('/') = chars.peek().copied() {
                break;
            }
        }

        out.push(ch);
    }

    out
}

fn extract_defs_uses(cleaned: &str, language: &str) -> (Vec<String>, Vec<String>) {
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return (vec![], vec![]);
    }

    let mut defs: BTreeSet<String> = BTreeSet::new();
    let mut uses: BTreeSet<String> = BTreeSet::new();

    if language == "rust" && trimmed.starts_with("let ") {
        let mut rest = trimmed.trim_start_matches("let ").trim_start();
        if let Some(stripped) = rest.strip_prefix("mut ") {
            rest = stripped.trim_start();
        }
        if let Some(eq_idx) = find_assignment_index(rest) {
            let lhs = &rest[..eq_idx];
            let rhs = &rest[eq_idx + 1..];
            for id in extract_idents(lhs, language) {
                defs.insert(id);
            }
            for id in extract_idents(rhs, language) {
                uses.insert(id);
            }
        } else {
            for id in extract_idents(rest, language) {
                defs.insert(id);
            }
        }
        return finalize_defs_uses(defs, uses);
    }

    if language == "go" && trimmed.starts_with("var ") {
        let rest = trimmed.trim_start_matches("var ").trim_start();
        if let Some(eq_idx) = find_assignment_index(rest) {
            let lhs = &rest[..eq_idx];
            let rhs = &rest[eq_idx + 1..];
            for id in extract_idents(lhs, language) {
                defs.insert(id);
            }
            for id in extract_idents(rhs, language) {
                uses.insert(id);
            }
        } else {
            let mut tokens = rest.split_whitespace();
            if let Some(name) = tokens.next() {
                for id in extract_idents(name, language) {
                    defs.insert(id);
                }
            }
        }
        return finalize_defs_uses(defs, uses);
    }

    if language == "go" {
        if let Some(idx) = trimmed.find(":=") {
            let lhs = &trimmed[..idx];
            let rhs = &trimmed[idx + 2..];
            for id in extract_idents(lhs, language) {
                defs.insert(id);
            }
            for id in extract_idents(rhs, language) {
                uses.insert(id);
            }
            return finalize_defs_uses(defs, uses);
        }
    }

    if let Some(eq_idx) = find_assignment_index(trimmed) {
        let lhs = &trimmed[..eq_idx];
        let rhs = &trimmed[eq_idx + 1..];
        for id in extract_idents(lhs, language) {
            defs.insert(id);
        }
        for id in extract_idents(rhs, language) {
            uses.insert(id);
        }
        return finalize_defs_uses(defs, uses);
    }

    for id in extract_idents(trimmed, language) {
        uses.insert(id);
    }
    finalize_defs_uses(defs, uses)
}

fn finalize_defs_uses(
    defs: BTreeSet<String>,
    mut uses: BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    for d in &defs {
        uses.remove(d);
    }
    (defs.into_iter().collect(), uses.into_iter().collect())
}

fn find_assignment_index(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'=' {
            continue;
        }
        if i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'<' | b'>' | b':') {
            continue;
        }
        if i + 1 < bytes.len() && matches!(bytes[i + 1], b'=' | b'>') {
            continue;
        }
        return Some(i);
    }
    None
}

fn extract_idents(s: &str, language: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for token in tokenize_identifiers(s) {
        if token == "_" || token == "self" || token == "this" {
            continue;
        }
        if is_keyword(&token, language) {
            continue;
        }
        out.insert(token);
    }
    out.into_iter().collect()
}

fn tokenize_identifiers(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            if current
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false)
            {
                tokens.push(current.to_lowercase());
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && current
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
    {
        tokens.push(current.to_lowercase());
    }
    tokens
}

fn is_keyword(token: &str, language: &str) -> bool {
    const COMMON: &[&str] = &[
        "fn",
        "func",
        "pub",
        "crate",
        "super",
        "self",
        "mut",
        "const",
        "static",
        "return",
        "if",
        "else",
        "match",
        "for",
        "while",
        "loop",
        "switch",
        "select",
        "case",
        "default",
        "break",
        "continue",
        "goto",
        "in",
        "as",
        "let",
        "var",
        "type",
        "struct",
        "enum",
        "trait",
        "impl",
        "interface",
        "package",
        "import",
        "use",
        "where",
        "async",
        "await",
        "defer",
        "go",
        "true",
        "false",
        "nil",
        "none",
        "some",
        "ok",
        "err",
        "result",
        "option",
        "string",
        "bool",
        "int",
        "i64",
        "i32",
        "u64",
        "u32",
        "f64",
        "f32",
        "usize",
        "isize",
    ];
    if COMMON.contains(&token) {
        return true;
    }
    match language {
        "rust" => matches!(
            token,
            "mod"
                | "move"
                | "unsafe"
                | "dyn"
                | "ref"
                | "where"
                | "extern"
                | "union"
                | "type"
                | "macro_rules"
        ),
        "go" => matches!(
            token,
            "chan"
                | "map"
                | "range"
                | "fallthrough"
                | "iota"
                | "uintptr"
                | "byte"
                | "rune"
                | "error"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_index_skips_comparisons() {
        assert_eq!(find_assignment_index("if a == b {"), None);
        assert_eq!(find_assignment_index("let x = a + b"), Some(6));
        assert_eq!(find_assignment_index("value := compute()"), None);
        assert_eq!(find_assignment_index("value = compute()"), Some(6));
    }

    #[test]
    fn rust_defs_uses_extracts_expected_symbols() {
        let (defs, uses) = extract_defs_uses("let total = a + b + adjust;", "rust");
        assert_eq!(defs, vec!["total".to_string()]);
        assert_eq!(
            uses,
            vec!["a".to_string(), "adjust".to_string(), "b".to_string()]
        );
    }
}
