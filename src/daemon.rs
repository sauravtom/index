use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const DEFAULT_THRESHOLD: usize = 20;
const IDLE_FLUSH_SECS: u64 = 10;
const DEFAULT_POLL_MS: u64 = 1_000;

#[derive(Debug, Serialize, Deserialize, Default)]
struct DaemonConfigFile {
    daemon: Option<DaemonConfigSection>,
    semantic: Option<SemanticConfigSection>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct DaemonConfigSection {
    auto_reindex_threshold: Option<usize>,
    poll_ms: Option<u64>,
    idle_flush_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SemanticConfigSection {
    auto_reindex_threshold: Option<usize>,
    enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct DaemonState {
    version: String,
    project_root: String,
    pid: Option<u32>,
    running: bool,
    threshold: usize,
    poll_ms: Option<u64>,
    idle_flush_secs: Option<u64>,
    dirty_files: usize,
    started_at_epoch: Option<u64>,
    last_seen_at_epoch: Option<u64>,
    last_notify_at_epoch: Option<u64>,
    last_reindex_at_epoch: Option<u64>,
    total_notifies: u64,
    total_reindexes: u64,
    last_error: Option<String>,
}

fn now_epoch_secs() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

fn resolve_project_root(path: Option<String>) -> Result<PathBuf> {
    let raw = match path {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().context("Failed to read current directory")?,
    };
    if !raw.exists() {
        return Err(anyhow!("Project path does not exist: {}", raw.display()));
    }
    if !raw.is_dir() {
        return Err(anyhow!(
            "Project path is not a directory: {}",
            raw.display()
        ));
    }
    raw.canonicalize()
        .with_context(|| format!("Failed to canonicalize project path: {}", raw.display()))
}

fn bakes_dir(root: &Path) -> PathBuf {
    root.join("bakes").join("latest")
}

fn daemon_dir(root: &Path) -> PathBuf {
    bakes_dir(root).join("daemon")
}

fn pid_path(root: &Path) -> PathBuf {
    daemon_dir(root).join("daemon.pid")
}

fn state_path(root: &Path) -> PathBuf {
    daemon_dir(root).join("state.json")
}

fn queue_path(root: &Path) -> PathBuf {
    daemon_dir(root).join("notify.queue")
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".tokenwise").join("config.json")
}

fn default_config() -> DaemonConfigFile {
    DaemonConfigFile {
        daemon: Some(DaemonConfigSection {
            auto_reindex_threshold: Some(DEFAULT_THRESHOLD),
            poll_ms: Some(DEFAULT_POLL_MS),
            idle_flush_secs: Some(IDLE_FLUSH_SECS),
        }),
        semantic: Some(SemanticConfigSection {
            auto_reindex_threshold: None,
            enabled: Some(true),
        }),
    }
}

fn ensure_daemon_dirs(root: &Path) -> Result<()> {
    fs::create_dir_all(daemon_dir(root)).with_context(|| {
        format!(
            "Failed creating daemon dir at {}",
            daemon_dir(root).display()
        )
    })
}

fn ensure_default_config(root: &Path) -> Result<()> {
    let config = config_path(root);
    if config.exists() {
        return Ok(());
    }
    if let Some(parent) = config.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed creating config directory at {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(&default_config())?;
    fs::write(&config, raw)
        .with_context(|| format!("Failed writing default config at {}", config.display()))
}

fn read_config(root: &Path) -> Option<DaemonConfigFile> {
    let raw = fs::read_to_string(config_path(root)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn read_threshold_from_config(root: &Path) -> Option<usize> {
    let cfg = read_config(root)?;
    cfg.daemon
        .as_ref()
        .and_then(|d| d.auto_reindex_threshold)
        .or_else(|| cfg.semantic.as_ref().and_then(|s| s.auto_reindex_threshold))
}

fn read_poll_ms_from_config(root: &Path) -> Option<u64> {
    let cfg = read_config(root)?;
    cfg.daemon.as_ref().and_then(|d| d.poll_ms)
}

fn read_idle_flush_secs_from_config(root: &Path) -> Option<u64> {
    let cfg = read_config(root)?;
    cfg.daemon.as_ref().and_then(|d| d.idle_flush_secs)
}

fn normalize_threshold(root: &Path, threshold: Option<usize>) -> usize {
    let cfg_threshold = read_threshold_from_config(root);
    threshold
        .or(cfg_threshold)
        .unwrap_or(DEFAULT_THRESHOLD)
        .max(1)
}

fn read_pid(root: &Path) -> Option<u32> {
    let content = fs::read_to_string(pid_path(root)).ok()?;
    content.trim().parse::<u32>().ok()
}

fn write_pid(root: &Path, pid: u32) -> Result<()> {
    fs::write(pid_path(root), format!("{pid}\n"))
        .with_context(|| format!("Failed writing pid file at {}", pid_path(root).display()))
}

fn remove_pid(root: &Path) {
    let _ = fs::remove_file(pid_path(root));
}

fn is_pid_running(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn load_state(root: &Path) -> DaemonState {
    let path = state_path(root);
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<DaemonState>(&raw).unwrap_or_default(),
        Err(_) => DaemonState::default(),
    }
}

fn save_state(root: &Path, state: &DaemonState) -> Result<()> {
    let path = state_path(root);
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(&path, raw)
        .with_context(|| format!("Failed writing daemon state at {}", path.display()))
}

fn ensure_bake_exists(root: &Path) -> Result<()> {
    let bake = bakes_dir(root).join("bake.json");
    if !bake.exists() {
        return Err(anyhow!(
            "No bake index found at {}. Run `tokenwise warm --path {}` first.",
            bake.display(),
            root.display()
        ));
    }
    Ok(())
}

fn normalize_changed_file(root: &Path, file: &str) -> String {
    let raw = PathBuf::from(file);
    let rel = if raw.is_absolute() {
        raw.strip_prefix(root).unwrap_or(&raw).to_path_buf()
    } else {
        raw
    };
    rel.to_string_lossy().replace('\\', "/")
}

pub fn start(path: Option<String>, threshold: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    ensure_daemon_dirs(&root)?;

    if let Some(pid) = read_pid(&root) {
        if is_pid_running(pid) {
            let payload = json!({
                "tool": "daemon_start",
                "version": env!("CARGO_PKG_VERSION"),
                "project_root": root,
                "status": "already_running",
                "pid": pid,
                "threshold": normalize_threshold(&root, threshold),
            });
            return Ok(serde_json::to_string_pretty(&payload)?);
        }
        remove_pid(&root);
    }

    let effective_threshold = normalize_threshold(&root, threshold);
    let poll_ms = read_poll_ms_from_config(&root)
        .unwrap_or(DEFAULT_POLL_MS)
        .max(100);

    let exe = std::env::current_exe().context("Failed to resolve current executable path")?;
    let child = Command::new(exe)
        .arg("daemon-run")
        .arg("--path")
        .arg(root.to_string_lossy().to_string())
        .arg("--threshold")
        .arg(effective_threshold.to_string())
        .arg("--poll-ms")
        .arg(poll_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn daemon process")?;

    let pid = child.id();
    write_pid(&root, pid)?;

    // Give the child a moment to boot so status checks are stable.
    thread::sleep(Duration::from_millis(150));

    if !is_pid_running(pid) {
        return Err(anyhow!(
            "Daemon process exited immediately. Check project path and run `tokenwise warm` first."
        ));
    }

    let mut boot_state = load_state(&root);
    boot_state.version = env!("CARGO_PKG_VERSION").to_string();
    boot_state.project_root = root.to_string_lossy().into_owned();
    boot_state.pid = Some(pid);
    boot_state.running = true;
    boot_state.threshold = effective_threshold;
    boot_state.poll_ms = Some(poll_ms);
    boot_state.idle_flush_secs = Some(
        read_idle_flush_secs_from_config(&root)
            .unwrap_or(IDLE_FLUSH_SECS)
            .max(1),
    );
    boot_state.started_at_epoch.get_or_insert(now_epoch_secs());
    boot_state.last_seen_at_epoch = Some(now_epoch_secs());
    save_state(&root, &boot_state)?;

    let payload = json!({
        "tool": "daemon_start",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "status": "started",
        "pid": pid,
        "threshold": effective_threshold,
        "poll_ms": poll_ms,
    });
    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn stop(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    ensure_daemon_dirs(&root)?;

    let pid = match read_pid(&root) {
        Some(p) => p,
        None => {
            let payload = json!({
                "tool": "daemon_stop",
                "version": env!("CARGO_PKG_VERSION"),
                "project_root": root,
                "status": "not_running",
            });
            return Ok(serde_json::to_string_pretty(&payload)?);
        }
    };

    // Cooperative shutdown: remove the pid file and let loop exit on next tick.
    remove_pid(&root);

    let mut stopped = false;
    for _ in 0..30 {
        if !is_pid_running(pid) {
            stopped = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    if !stopped {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
        for _ in 0..20 {
            if !is_pid_running(pid) {
                stopped = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let mut state = load_state(&root);
    state.running = false;
    state.pid = None;
    state.last_seen_at_epoch = Some(now_epoch_secs());
    save_state(&root, &state)?;

    let payload = json!({
        "tool": "daemon_stop",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "status": if stopped { "stopped" } else { "terminate_requested" },
        "pid": pid,
    });
    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn status(path: Option<String>) -> Result<String> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    ensure_daemon_dirs(&root)?;

    let mut state = load_state(&root);
    let pid = read_pid(&root).or(state.pid);
    let running = pid.map(is_pid_running).unwrap_or(false);

    if !running {
        state.running = false;
        state.pid = None;
    } else {
        state.running = true;
        state.pid = pid;
    }

    let mut queue_entries = 0usize;
    let mut pending_unique = BTreeSet::new();
    if let Ok(raw) = fs::read_to_string(queue_path(&root)) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            queue_entries += 1;
            pending_unique.insert(trimmed.to_string());
        }
    }

    state.version = env!("CARGO_PKG_VERSION").to_string();
    state.project_root = root.to_string_lossy().into_owned();
    state.last_seen_at_epoch = Some(now_epoch_secs());
    if state.threshold == 0 {
        state.threshold = normalize_threshold(&root, None);
    }
    state.poll_ms = Some(
        state
            .poll_ms
            .unwrap_or_else(|| read_poll_ms_from_config(&root).unwrap_or(DEFAULT_POLL_MS))
            .max(100),
    );
    state.idle_flush_secs = Some(
        state
            .idle_flush_secs
            .unwrap_or_else(|| read_idle_flush_secs_from_config(&root).unwrap_or(IDLE_FLUSH_SECS))
            .max(1),
    );
    save_state(&root, &state)?;

    let payload = json!({
        "tool": "daemon_status",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "running": running,
        "pid": if running { pid } else { None },
        "threshold": state.threshold,
        "poll_ms": state.poll_ms,
        "idle_flush_secs": state.idle_flush_secs,
        "dirty_files": state.dirty_files,
        "queue_entries": queue_entries,
        "pending_unique_files": pending_unique.len(),
        "started_at_epoch": state.started_at_epoch,
        "last_notify_at_epoch": state.last_notify_at_epoch,
        "last_reindex_at_epoch": state.last_reindex_at_epoch,
        "total_notifies": state.total_notifies,
        "total_reindexes": state.total_reindexes,
        "last_error": state.last_error,
    });

    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn notify(path: Option<String>, file: String) -> Result<String> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    ensure_daemon_dirs(&root)?;
    ensure_bake_exists(&root)?;

    let normalized = normalize_changed_file(&root, &file);
    let queue = queue_path(&root);
    {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&queue)
            .with_context(|| format!("Failed opening queue file at {}", queue.display()))?;
        writeln!(f, "{normalized}")
            .with_context(|| format!("Failed writing queue file at {}", queue.display()))?;
    }

    let mut state = load_state(&root);
    let pid = read_pid(&root);
    let running = pid.map(is_pid_running).unwrap_or(false);

    state.version = env!("CARGO_PKG_VERSION").to_string();
    state.project_root = root.to_string_lossy().into_owned();
    state.last_notify_at_epoch = Some(now_epoch_secs());
    state.total_notifies = state.total_notifies.saturating_add(1);

    let mode = if running {
        "queued"
    } else {
        // Daemon isn't running: process this file inline so notify still works.
        crate::engine::util::reindex_files(&root, &[normalized.as_str()])?;
        if let Err(e) = crate::engine::embed::upsert_embeddings_for_files(
            &bakes_dir(&root),
            &[normalized.as_str()],
        ) {
            state.last_error = Some(e.to_string());
        } else {
            state.last_error = None;
        }
        state.last_reindex_at_epoch = Some(now_epoch_secs());
        state.total_reindexes = state.total_reindexes.saturating_add(1);
        "inline"
    };

    state.running = running;
    state.pid = pid;
    save_state(&root, &state)?;

    let payload = json!({
        "tool": "daemon_notify",
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": root,
        "file": normalized,
        "mode": mode,
        "daemon_running": running,
        "pid": if running { pid } else { None },
    });
    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn run_forever(
    path: Option<String>,
    threshold: Option<usize>,
    poll_ms: Option<u64>,
) -> Result<()> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    ensure_daemon_dirs(&root)?;
    ensure_bake_exists(&root)?;

    let effective_threshold = normalize_threshold(&root, threshold);
    let poll = poll_ms
        .or_else(|| read_poll_ms_from_config(&root))
        .unwrap_or(DEFAULT_POLL_MS)
        .max(100);
    let idle_flush_secs = read_idle_flush_secs_from_config(&root)
        .unwrap_or(IDLE_FLUSH_SECS)
        .max(1);
    let pid = std::process::id();

    write_pid(&root, pid)?;
    let queue = queue_path(&root);
    if !queue.exists() {
        let _ = File::create(&queue)?;
    }

    let mut state = load_state(&root);
    state.version = env!("CARGO_PKG_VERSION").to_string();
    state.project_root = root.to_string_lossy().into_owned();
    state.pid = Some(pid);
    state.running = true;
    state.threshold = effective_threshold;
    state.poll_ms = Some(poll);
    state.idle_flush_secs = Some(idle_flush_secs);
    state.started_at_epoch.get_or_insert(now_epoch_secs());
    state.last_seen_at_epoch = Some(now_epoch_secs());
    save_state(&root, &state)?;

    let mut dirty: BTreeSet<String> = BTreeSet::new();
    let mut offset: u64 = 0;
    let mut last_dirty_at = now_epoch_secs();

    loop {
        match read_pid(&root) {
            Some(current_pid) if current_pid == pid => {}
            _ => break,
        }

        let file = match File::open(&queue) {
            Ok(f) => f,
            Err(_) => {
                thread::sleep(Duration::from_millis(poll));
                continue;
            }
        };

        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if offset > len {
            offset = 0;
        }

        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        let mut new_bytes: u64 = 0;

        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            new_bytes = new_bytes.saturating_add(n as u64);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            dirty.insert(trimmed.to_string());
            last_dirty_at = now_epoch_secs();
        }

        offset = offset.saturating_add(new_bytes);

        let now = now_epoch_secs();
        let flush_due_to_threshold = dirty.len() >= effective_threshold;
        let flush_due_to_idle =
            !dirty.is_empty() && now.saturating_sub(last_dirty_at) >= idle_flush_secs;
        let mut flush_last_error: Option<String> = None;
        let mut flush_happened = false;

        if flush_due_to_threshold || flush_due_to_idle {
            let files: Vec<String> = dirty.iter().cloned().collect();
            let refs: Vec<&str> = files.iter().map(String::as_str).collect();
            flush_happened = true;

            if let Err(e) = crate::engine::util::reindex_files(&root, &refs) {
                flush_last_error = Some(e.to_string());
            } else if let Err(e) =
                crate::engine::embed::upsert_embeddings_for_files(&bakes_dir(&root), &refs)
            {
                flush_last_error = Some(e.to_string());
            }

            dirty.clear();

            if offset >= len {
                let _ = fs::write(&queue, "");
                offset = 0;
            }
        }

        // Reload on every loop to avoid overwriting fields that notify updates concurrently.
        let mut loop_state = load_state(&root);
        loop_state.version = env!("CARGO_PKG_VERSION").to_string();
        loop_state.project_root = root.to_string_lossy().into_owned();
        loop_state.pid = Some(pid);
        loop_state.running = true;
        loop_state.threshold = effective_threshold;
        loop_state.poll_ms = Some(poll);
        loop_state.idle_flush_secs = Some(idle_flush_secs);
        loop_state.started_at_epoch.get_or_insert(now_epoch_secs());
        loop_state.dirty_files = dirty.len();
        loop_state.last_seen_at_epoch = Some(now_epoch_secs());
        if flush_happened {
            loop_state.last_reindex_at_epoch = Some(now_epoch_secs());
            loop_state.total_reindexes = loop_state.total_reindexes.saturating_add(1);
            loop_state.last_error = flush_last_error;
        }
        save_state(&root, &loop_state)?;

        thread::sleep(Duration::from_millis(poll));
    }

    // Graceful cleanup.
    let mut final_state = load_state(&root);
    final_state.running = false;
    final_state.pid = None;
    final_state.dirty_files = 0;
    final_state.last_seen_at_epoch = Some(now_epoch_secs());
    save_state(&root, &final_state)?;
    remove_pid(&root);

    Ok(())
}

pub fn warm(path: Option<String>, no_daemon: bool, threshold: Option<usize>) -> Result<String> {
    let root = resolve_project_root(path)?;
    ensure_default_config(&root)?;
    let root_arg = Some(root.to_string_lossy().into_owned());
    let bake_json = crate::engine::bake(root_arg.clone())?;

    let daemon_json = if no_daemon {
        json!({
            "status": "skipped",
            "reason": "no_daemon flag set"
        })
    } else {
        let daemon_out = start(root_arg, threshold)?;
        serde_json::from_str::<serde_json::Value>(&daemon_out)
            .unwrap_or_else(|_| json!({"status": "unknown", "raw": daemon_out}))
    };

    let bake_value = serde_json::from_str::<serde_json::Value>(&bake_json)
        .unwrap_or_else(|_| json!({"raw": bake_json}));

    let payload = json!({
        "tool": "warm",
        "version": env!("CARGO_PKG_VERSION"),
        "bake": bake_value,
        "daemon": daemon_json,
    });

    Ok(serde_json::to_string_pretty(&payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn project_root(dir: &TempDir) -> Option<String> {
        Some(dir.path().to_string_lossy().into_owned())
    }

    fn write_file(root: &TempDir, rel: &str, content: &str) {
        let p = root.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        write_file(
            &dir,
            "src/lib.rs",
            r#"pub fn add(a: i64, b: i64) -> i64 {
    a + b
}
"#,
        );
        write_file(
            &dir,
            "src/other.rs",
            r#"pub fn mul(a: i64, b: i64) -> i64 {
    a * b
}
"#,
        );
        crate::engine::bake(project_root(&dir)).unwrap();
        dir
    }

    fn parse_json(raw: &str) -> Value {
        serde_json::from_str(raw).unwrap()
    }

    fn wait_until<F>(timeout: Duration, mut cond: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn daemon_notify_uses_inline_mode_when_offline() {
        let dir = setup_project();
        let out = notify(project_root(&dir), "src/lib.rs".into()).unwrap();
        let v = parse_json(&out);

        assert_eq!(v["tool"], "daemon_notify");
        assert_eq!(v["mode"], "inline");
        assert_eq!(v["daemon_running"], false);

        let status_out = status(project_root(&dir)).unwrap();
        let s = parse_json(&status_out);
        assert_eq!(s["running"], false);
        assert!(s["total_notifies"].as_u64().unwrap_or(0) >= 1);
        assert!(s["total_reindexes"].as_u64().unwrap_or(0) >= 1);
    }

    #[test]
    fn daemon_status_reports_deduped_pending_queue_files() {
        let dir = setup_project();
        notify(project_root(&dir), "src/lib.rs".into()).unwrap();
        notify(project_root(&dir), "src/lib.rs".into()).unwrap();

        let status_out = status(project_root(&dir)).unwrap();
        let s = parse_json(&status_out);

        assert_eq!(s["running"], false);
        assert_eq!(s["queue_entries"], 2);
        assert_eq!(s["pending_unique_files"], 1);
        assert_eq!(s["total_notifies"], 2);
    }

    #[test]
    fn run_forever_processes_queue_and_updates_state() {
        let dir = setup_project();
        let root = dir.path().to_path_buf();
        let root_str = root.to_string_lossy().into_owned();

        let worker = std::thread::spawn({
            let root_for_thread = root_str.clone();
            move || run_forever(Some(root_for_thread), Some(2), Some(100))
        });

        assert!(
            wait_until(Duration::from_secs(5), || {
                let out = status(Some(root_str.clone())).unwrap();
                let v = parse_json(&out);
                v["running"].as_bool().unwrap_or(false)
            }),
            "daemon loop did not report running in time"
        );

        let n1 = parse_json(&notify(Some(root_str.clone()), "src/lib.rs".into()).unwrap());
        let n2 = parse_json(&notify(Some(root_str.clone()), "src/other.rs".into()).unwrap());
        assert_eq!(n1["mode"], "queued");
        assert_eq!(n2["mode"], "queued");

        assert!(
            wait_until(Duration::from_secs(8), || {
                let out = status(Some(root_str.clone())).unwrap();
                let v = parse_json(&out);
                v["total_reindexes"].as_u64().unwrap_or(0) >= 1
            }),
            "daemon did not flush queued changes"
        );

        let _ = fs::remove_file(pid_path(&root));
        worker.join().unwrap().unwrap();

        let final_status = parse_json(&status(Some(root_str)).unwrap());
        assert_eq!(final_status["running"], false);
        assert_eq!(final_status["dirty_files"], 0);
    }
}
