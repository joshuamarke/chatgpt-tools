//! Backup utilities for provider live configs (Codex / Grok).
//! Handles deduplication and automated rotation/pruning.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const MAX_BACKUPS_PER_CATEGORY: usize = 1;
pub const MAX_BACKUP_AGE_SECS: u64 = 7 * 24 * 3600; // 7 days

/// Backup directory for live provider configs.
pub fn live_backup_dir() -> std::path::PathBuf {
    crate::sessions::paths::app_state_dir().join("provider-live-backups")
}

/// Backup a live config file if its content has changed compared to the latest backup.
/// `file_prefix`: e.g. "grok-config", "codex-auth", "codex-config"
/// `file_ext`: e.g. "toml", "json"
pub fn save_live_backup(file_prefix: &str, file_ext: &str, content: &[u8]) -> Result<(), String> {
    if content.is_empty() {
        return Ok(());
    }
    let dir = live_backup_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Check if the latest existing backup for this prefix has identical content
    if let Some(latest_path) = get_latest_backup_file(&dir, file_prefix, file_ext) {
        if let Ok(existing_bytes) = fs::read(&latest_path) {
            if existing_bytes == content {
                // Deduplicate: skip writing identical backup
                let _ = prune_backups_for_category(&dir, file_prefix, file_ext);
                return Ok(());
            }
        }
    }

    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("{file_prefix}-{stamp}.{file_ext}");
    let path = dir.join(file_name);
    fs::write(&path, content).map_err(|e| e.to_string())?;

    // Prune backups for this category after writing
    let _ = prune_backups_for_category(&dir, file_prefix, file_ext);
    Ok(())
}

fn get_latest_backup_file(dir: &Path, file_prefix: &str, file_ext: &str) -> Option<std::path::PathBuf> {
    let mut matching = list_category_backups(dir, file_prefix, file_ext);
    matching.sort_by_key(|(mtime, _path)| *mtime);
    matching.pop().map(|(_mtime, path)| path)
}

fn list_category_backups(dir: &Path, file_prefix: &str, file_ext: &str) -> Vec<(SystemTime, std::path::PathBuf)> {
    let prefix = format!("{file_prefix}-");
    let ext = format!(".{file_ext}");
    let mut files = Vec::new();

    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with(&prefix) && name.ends_with(&ext) {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((mtime, path));
        }
    }
    files
}

/// Prune backup files for a specific category, enforcing max count and max age.
pub fn prune_backups_for_category(dir: &Path, file_prefix: &str, file_ext: &str) -> Result<(), String> {
    let mut files = list_category_backups(dir, file_prefix, file_ext);
    // Sort oldest first
    files.sort_by_key(|(mtime, _path)| *mtime);

    let now = SystemTime::now();
    let max_age = Duration::from_secs(MAX_BACKUP_AGE_SECS);

    let total = files.len();
    for (i, (mtime, path)) in files.into_iter().enumerate() {
        let is_over_limit = total.saturating_sub(i) > MAX_BACKUPS_PER_CATEGORY;
        let is_expired = now.duration_since(mtime).unwrap_or_default() > max_age;

        if is_over_limit || is_expired {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

/// Run full pruning pass over all categories in provider-live-backups.
pub fn prune_all_provider_backups() {
    let dir = live_backup_dir();
    if !dir.is_dir() {
        return;
    }
    let _ = prune_backups_for_category(&dir, "grok-config", "toml");
    let _ = prune_backups_for_category(&dir, "codex-auth", "json");
    let _ = prune_backups_for_category(&dir, "codex-config", "toml");
}
