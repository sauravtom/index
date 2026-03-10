use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CURRENT_NAMESPACE: &str = "tokenwise";
const LEGACY_NAMESPACE: &str = "yoyo";

pub(crate) fn run_slash_command(args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        bail!("No slash command provided. Try /tw:propose <change-name>.");
    }

    let raw_command = args[0].trim();
    if raw_command.starts_with("/yoyo:") {
        eprintln!(
            "[deprecated] `/yoyo:*` is deprecated. Use `/tw:*` (for example, `/tw:propose`)."
        );
    }
    let command = normalize_slash_command(raw_command);
    let rest = &args[1..];
    let root = std::env::current_dir().context("Failed to determine current directory")?;

    match command.as_str() {
        "/tw:propose" => propose_change(&root, rest),
        "/tw:apply" => apply_change(&root, rest),
        "/tw:archive" => archive_change(&root, rest),
        "/tw:status" | "/tw:show" => show_status(&root),
        _ => bail!(
            "Unknown slash command: {command}. Supported: /tw:propose, /tw:apply, /tw:archive, /tw:status"
        ),
    }
}

fn normalize_slash_command(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("/yoyo:") {
        return format!("/tw:{rest}");
    }
    raw.to_string()
}

fn current_namespace_root(root: &Path) -> PathBuf {
    root.join(CURRENT_NAMESPACE)
}

fn legacy_namespace_root(root: &Path) -> PathBuf {
    root.join(LEGACY_NAMESPACE)
}

fn current_changes_root(root: &Path) -> PathBuf {
    current_namespace_root(root).join("changes")
}

fn legacy_changes_root(root: &Path) -> PathBuf {
    legacy_namespace_root(root).join("changes")
}

fn current_specs_root(root: &Path) -> PathBuf {
    current_namespace_root(root).join("specs")
}

fn legacy_specs_root(root: &Path) -> PathBuf {
    legacy_namespace_root(root).join("specs")
}

fn resolve_change_dir(root: &Path, slug: &str) -> Option<PathBuf> {
    let current = current_changes_root(root).join(slug);
    if current.is_dir() {
        return Some(current);
    }
    let legacy = legacy_changes_root(root).join(slug);
    if legacy.is_dir() {
        return Some(legacy);
    }
    None
}

fn list_change_dirs_under(changes_root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = list_dirs(changes_root)?
        .into_iter()
        .filter(|p| p.file_name().and_then(|v| v.to_str()) != Some("archive"))
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn list_archived_changes(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    dirs.extend(list_dirs(&current_changes_root(root).join("archive"))?);
    dirs.extend(list_dirs(&legacy_changes_root(root).join("archive"))?);
    Ok(dirs)
}

fn propose_change(root: &Path, rest: &[String]) -> Result<()> {
    let raw_name = rest
        .first()
        .ok_or_else(|| anyhow!("Usage: /tw:propose <change-name>"))?;
    let slug = slugify(raw_name);
    if slug.is_empty() {
        bail!(
            "Change name is empty after normalization. Use letters/numbers (e.g. add-dark-mode)."
        );
    }

    let changes_root = current_changes_root(root);
    let change_dir = changes_root.join(&slug);
    let specs_dir = change_dir.join("specs");

    if resolve_change_dir(root, &slug).is_some() {
        bail!("Change already exists: {}", rel(root, &change_dir));
    }

    fs::create_dir_all(&specs_dir).context("Failed creating change directories")?;

    let proposal = format!(
        "# Proposal: {slug}\n\n## Why\n- Describe the user/problem this change solves.\n\n## What Changes\n- Summarize behavior and affected components.\n\n## Success Criteria\n- Clear outcomes and verification signal(s).\n"
    );
    let design = format!(
        "# Design: {slug}\n\n## Approach\n- High-level architecture and key tradeoffs.\n\n## Implementation Notes\n- Data flow, interfaces, and migration details if needed.\n\n## Risks\n- Edge cases and rollback plan.\n"
    );
    let tasks = "# Tasks\n\n## 1. Implementation\n- [ ] 1.1 Add core behavior\n- [ ] 1.2 Integrate with existing flow\n\n## 2. Validation\n- [ ] 2.1 Add/adjust tests\n- [ ] 2.2 Update docs\n";
    let requirements = format!(
        "# Requirements: {slug}\n\n## Requirements\n- The system SHALL ...\n\n## Scenarios\n- Given <context>, when <action>, then <outcome>.\n"
    );

    write_file(change_dir.join("proposal.md"), &proposal)?;
    write_file(change_dir.join("design.md"), &design)?;
    write_file(change_dir.join("tasks.md"), tasks)?;
    write_file(specs_dir.join("requirements.md"), &requirements)?;

    println!("Created {}/", rel(root, &change_dir));
    println!("  ✓ proposal.md — why we're doing this, what's changing");
    println!("  ✓ specs/       — requirements and scenarios");
    println!("  ✓ design.md    — technical approach");
    println!("  ✓ tasks.md     — implementation checklist");
    println!("  Ready for implementation!");
    Ok(())
}

fn apply_change(root: &Path, rest: &[String]) -> Result<()> {
    let active = list_active_changes(root)?;
    if active.is_empty() {
        bail!(
            "No active changes found under tokenwise/changes/ (or legacy yoyo/changes/). Create one with /tw:propose."
        );
    }

    let target = if let Some(name) = rest.first() {
        let slug = slugify(name);
        resolve_change_dir(root, &slug).ok_or_else(|| {
            anyhow!(
                "Active change not found: {} or {}",
                rel(root, &current_changes_root(root).join(&slug)),
                rel(root, &legacy_changes_root(root).join(&slug)),
            )
        })?
    } else {
        active[0].clone()
    };

    let tasks_path = target.join("tasks.md");
    if !tasks_path.exists() {
        bail!("Missing tasks file: {}", rel(root, &tasks_path));
    }

    let content = fs::read_to_string(&tasks_path)
        .with_context(|| format!("Failed reading {}", rel(root, &tasks_path)))?;
    let (updated, completed_now) = mark_pending_tasks_done(&content);

    fs::write(&tasks_path, updated)
        .with_context(|| format!("Failed writing {}", rel(root, &tasks_path)))?;

    println!("Implementing tasks...");
    if completed_now.is_empty() {
        println!("  No pending tasks found in {}.", rel(root, &tasks_path));
    } else {
        for item in &completed_now {
            println!("  ✓ {item}");
        }
    }
    println!("All tasks complete!");
    Ok(())
}

fn archive_change(root: &Path, rest: &[String]) -> Result<()> {
    let active = list_active_changes(root)?;
    if active.is_empty() {
        bail!("No active changes found under tokenwise/changes/ (or legacy yoyo/changes/).");
    }

    let target = if let Some(name) = rest.first() {
        let slug = slugify(name);
        resolve_change_dir(root, &slug).ok_or_else(|| {
            anyhow!(
                "Active change not found: {} or {}",
                rel(root, &current_changes_root(root).join(&slug)),
                rel(root, &legacy_changes_root(root).join(&slug)),
            )
        })?
    } else {
        active[0].clone()
    };

    let slug = target
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| anyhow!("Invalid change folder name"))?;
    let date = current_date_yyyy_mm_dd();
    let archive_root = current_changes_root(root).join("archive");
    fs::create_dir_all(&archive_root).context("Failed creating archive directory")?;
    let archived_dir = archive_root.join(format!("{date}-{slug}"));

    if archived_dir.exists() {
        bail!("Archive path already exists: {}", rel(root, &archived_dir));
    }

    fs::rename(&target, &archived_dir).with_context(|| {
        format!(
            "Failed moving {} to {}",
            rel(root, &target),
            rel(root, &archived_dir)
        )
    })?;

    sync_specs_into_catalog(root, &archived_dir, slug)?;

    println!("Archived to {}/", rel(root, &archived_dir));
    println!("Specs updated. Ready for the next feature.");
    Ok(())
}

fn show_status(root: &Path) -> Result<()> {
    let active = list_active_changes(root)?;
    let mut archived = list_archived_changes(root)?;
    archived.sort();
    let completed_count = archived.len();
    let spec_stats = collect_spec_stats(root, &active)?;
    let (done, total) = collect_task_progress(&active)?;
    let pct = if total == 0 {
        0
    } else {
        ((done as f64 / total as f64) * 100.0).round() as u32
    };

    println!("Summary:");
    println!(
        "  - Specifications: {} specs, {} requirements",
        spec_stats.spec_files, spec_stats.requirements
    );
    println!("  - Active Changes: {} in progress", active.len());
    println!("  - Completed Changes: {}", completed_count);
    println!("  - Task Progress: {done}/{total} ({pct}% complete)");
    println!();
    println!("Active Changes");
    println!("----------------------------------------");
    if active.is_empty() {
        println!("  (none)");
    } else {
        for change in &active {
            let name = change
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("unknown");
            let (d, t) = task_progress_for_change(change)?;
            let p = if t == 0 {
                0
            } else {
                ((d as f64 / t as f64) * 100.0).round() as u32
            };
            println!("  - {:<32} [{}] {}%", name, progress_bar(p, 24), p);
        }
    }

    println!();
    println!("Completed Changes");
    println!("----------------------------------------");
    if archived.is_empty() {
        println!("  (none)");
    } else {
        for item in archived.iter().rev().take(10) {
            let name = item
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("unknown");
            println!("  ✓ {name}");
        }
    }

    println!();
    println!("Specifications");
    println!("----------------------------------------");
    let specs = list_spec_files_with_counts(root, &active)?;
    if specs.is_empty() {
        println!("  (none)");
    } else {
        for (name, reqs) in specs {
            println!("  - {:<32} {} requirements", name, reqs);
        }
    }
    Ok(())
}

fn list_active_changes(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(list_change_dirs_under(&current_changes_root(root))?);
    dirs.extend(list_change_dirs_under(&legacy_changes_root(root))?);
    Ok(dirs)
}

fn list_dirs(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path).with_context(|| format!("Failed listing {}", path.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            dirs.push(p);
        }
    }
    Ok(dirs)
}

fn collect_task_progress(changes: &[PathBuf]) -> Result<(usize, usize)> {
    let mut done = 0usize;
    let mut total = 0usize;
    for change in changes {
        let (d, t) = task_progress_for_change(change)?;
        done += d;
        total += t;
    }
    Ok((done, total))
}

fn task_progress_for_change(change: &Path) -> Result<(usize, usize)> {
    let tasks_path = change.join("tasks.md");
    if !tasks_path.exists() {
        return Ok((0, 0));
    }
    let content = fs::read_to_string(&tasks_path)?;
    let mut done = 0usize;
    let mut total = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ] ")
            || trimmed.starts_with("- [x] ")
            || trimmed.starts_with("- [X] ")
        {
            total += 1;
            if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
                done += 1;
            }
        }
    }
    Ok((done, total))
}

fn collect_spec_stats(root: &Path, active_changes: &[PathBuf]) -> Result<SpecStats> {
    let mut stats = SpecStats::default();
    for root_specs in [current_specs_root(root), legacy_specs_root(root)] {
        if root_specs.exists() {
            for path in list_markdown_files(&root_specs)? {
                stats.spec_files += 1;
                stats.requirements += count_requirement_lines(&path)?;
            }
        }
    }

    for change in active_changes {
        let spec_dir = change.join("specs");
        if spec_dir.exists() {
            for path in list_markdown_files(&spec_dir)? {
                stats.spec_files += 1;
                stats.requirements += count_requirement_lines(&path)?;
            }
        }
    }

    Ok(stats)
}

fn list_spec_files_with_counts(
    root: &Path,
    active_changes: &[PathBuf],
) -> Result<Vec<(String, usize)>> {
    let mut specs = Vec::new();

    for root_specs in [current_specs_root(root), legacy_specs_root(root)] {
        if root_specs.exists() {
            for path in list_markdown_files(&root_specs)? {
                let name = path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                specs.push((name, count_requirement_lines(&path)?));
            }
        }
    }

    for change in active_changes {
        let change_name = change
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("change");
        let spec_dir = change.join("specs");
        if spec_dir.exists() {
            for path in list_markdown_files(&spec_dir)? {
                let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("spec");
                let label = format!("{change_name}/{stem}");
                specs.push((label, count_requirement_lines(&path)?));
            }
        }
    }

    specs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(specs)
}

#[derive(Default)]
struct SpecStats {
    spec_files: usize,
    requirements: usize,
}

fn list_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn count_requirement_lines(path: &Path) -> Result<usize> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter(|line| line.trim_start().starts_with("- "))
        .count())
}

fn sync_specs_into_catalog(root: &Path, archived_dir: &Path, slug: &str) -> Result<()> {
    let source = archived_dir.join("specs").join("requirements.md");
    if !source.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&source)?;
    let target_dir = current_specs_root(root);
    fs::create_dir_all(&target_dir)?;
    let target_file = target_dir.join(format!("{slug}.md"));
    fs::write(&target_file, content)?;
    Ok(())
}

fn write_file(path: PathBuf, content: &str) -> Result<()> {
    fs::write(&path, content).with_context(|| format!("Failed writing {}", path.display()))
}

fn mark_pending_tasks_done(content: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(content.len() + 32);
    let mut completed_now = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [ ] ") {
            let indent_len = line.len() - trimmed.len();
            let indent = &line[..indent_len];
            let task = trimmed.trim_start_matches("- [ ] ").trim().to_string();
            completed_now.push(task.clone());
            out.push_str(indent);
            out.push_str("- [x] ");
            out.push_str(&task);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    (out, completed_now)
}

fn progress_bar(percent: u32, width: usize) -> String {
    let filled = ((percent as f64 / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("{}{}", "#".repeat(filled), ".".repeat(empty))
}

fn rel(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .unwrap_or(target)
        .to_string_lossy()
        .replace('\\', "/")
}

fn current_date_yyyy_mm_dd() -> String {
    let output = Command::new("date").arg("+%F").output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "unknown-date".to_string(),
    }
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch == '-' || ch == '_' || ch == ' ') && !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::{mark_pending_tasks_done, slugify};

    #[test]
    fn slugify_normalizes_text() {
        assert_eq!(slugify("Add Dark Mode"), "add-dark-mode");
        assert_eq!(slugify("add_dark_mode"), "add-dark-mode");
        assert_eq!(slugify("  Add---Dark___Mode  "), "add-dark-mode");
    }

    #[test]
    fn task_marking_checks_pending_only() {
        let input = "- [ ] 1.1 First task\n- [x] 1.2 Done already\n";
        let (updated, completed) = mark_pending_tasks_done(input);
        assert!(updated.contains("- [x] 1.1 First task"));
        assert!(updated.contains("- [x] 1.2 Done already"));
        assert_eq!(completed, vec!["1.1 First task"]);
    }
}
