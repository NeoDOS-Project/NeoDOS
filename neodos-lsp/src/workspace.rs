use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;

use crate::config::NeodosLspConfig;

/// File change event from the watcher.
#[derive(Debug, Clone)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    FullRescan,
}

/// Tracks workspace files and their modification timestamps.
pub struct WorkspaceManager {
    config: Arc<NeodosLspConfig>,
    /// Known files and their last modification time for polling.
    known_files: RwLock<HashMap<PathBuf, FileMeta>>,
    /// Version counter for file changes.
    file_versions: RwLock<HashMap<PathBuf, u64>>,
    /// Whether a full rescan was requested.
    rescan_requested: RwLock<bool>,
}

#[derive(Debug, Clone)]
struct FileMeta {
    modified: SystemTime,
    size: u64,
}

impl WorkspaceManager {
    pub fn new(config: Arc<NeodosLspConfig>) -> Self {
        Self {
            config,
            known_files: RwLock::new(HashMap::new()),
            file_versions: RwLock::new(HashMap::new()),
            rescan_requested: RwLock::new(false),
        }
    }

    /// Register a set of files as the initial workspace.
    pub fn register_files(&self, files: &[PathBuf]) {
        let mut known = self.known_files.write();
        let mut versions = self.file_versions.write();
        for f in files {
            let meta = std::fs::metadata(f).ok();
            known.insert(
                f.clone(),
                FileMeta {
                    modified: meta.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH),
                    size: meta.map(|m| m.len()).unwrap_or(0),
                },
            );
            versions.entry(f.clone()).or_insert(0);
        }
    }

    /// Poll the filesystem for changes. Returns list of changed files.
    pub fn poll_for_changes(&self) -> Vec<(PathBuf, FileEvent)> {
        let mut events: Vec<(PathBuf, FileEvent)> = Vec::new();

        // Check for full rescan request.
        if *self.rescan_requested.read() {
            *self.rescan_requested.write() = false;
            events.push((PathBuf::new(), FileEvent::FullRescan));
            return events;
        }

        let known = self.known_files.read();
        for (path, meta) in known.iter() {
            if let Ok(current_meta) = std::fs::metadata(path) {
                let modified = current_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let size = current_meta.len();
                if modified != meta.modified || size != meta.size {
                    events.push((path.clone(), FileEvent::Modified(path.clone())));
                }
            }
        }
        drop(known);

        // Also check for new files (compare known set with filesystem scan).
        // Only do this periodically to avoid overhead.
        let known_paths: Vec<PathBuf> = self.known_files.read().keys().cloned().collect();
        let discovered = self.discover_current_files();
        for f in &discovered {
            if !known_paths.contains(f) {
                let meta = std::fs::metadata(f)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                self.known_files.write().insert(
                    f.clone(),
                    FileMeta {
                        modified: meta,
                        size: std::fs::metadata(f).map(|m| m.len()).unwrap_or(0),
                    },
                );
                self.file_versions.write().insert(f.clone(), 0);
                events.push((f.clone(), FileEvent::Created(f.clone())));
            }
        }

        // Check for deleted files.
        let current_set: std::collections::HashSet<PathBuf> = discovered.into_iter().collect();
        for f in &known_paths {
            if !current_set.contains(f) {
                self.known_files.write().remove(f);
                self.file_versions.write().remove(f);
                events.push((f.clone(), FileEvent::Deleted(f.clone())));
            }
        }

        events
    }

    /// Get the current version of a file, incrementing it.
    #[cfg(test)]
    pub fn next_version(&self, path: &PathBuf) -> u64 {
        let mut versions = self.file_versions.write();
        let v = versions.entry(path.clone()).or_insert(0);
        *v += 1;
        *v
    }

    fn discover_current_files(&self) -> Vec<PathBuf> {
        let exclude = &self.config.workspace.exclude_patterns;
        let mut files = Vec::new();

        for root in self.config.workspace.roots.read().iter() {
            if !root.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    let name = e.file_name().to_str().unwrap_or("");
                    if name.starts_with('.') && e.depth() == 1 {
                        return false;
                    }
                    let path = e.path().to_string_lossy();
                    !exclude.iter().any(|pat| {
                        if pat.ends_with("/**") {
                            path.contains(&pat[..pat.len() - 3])
                        } else {
                            path.contains(pat)
                        }
                    })
                })
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .map(|e| e == "rs")
                        .unwrap_or(false)
                {
                    files.push(entry.path().to_path_buf());
                }
            }
        }

        files
    }

    #[cfg(test)]
    pub fn current_version(&self, path: &PathBuf) -> u64 {
        self.file_versions
            .read()
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_track_and_poll_modified() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("test.rs");
        fs::write(&file, "v1").ok();

        let cfg = Arc::new(NeodosLspConfig::default());
        *cfg.workspace.roots.write() = vec![dir.path().to_path_buf()];
        let wm = WorkspaceManager::new(cfg);
        wm.register_files(&[file.clone()]);

        // First poll should report no changes.
        let first_poll = wm.poll_for_changes();
        assert!(first_poll.is_empty(), "expected no changes on first poll, got {first_poll:?}");

        // Modify.
        fs::write(&file, "v2").ok();
        let events = wm.poll_for_changes();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].1, FileEvent::Modified(_)));
    }

    #[test]
    fn test_next_version_increments() {
        let cfg = Arc::new(NeodosLspConfig::default());
        let wm = WorkspaceManager::new(cfg);
        let p = PathBuf::from("a.rs");

        assert_eq!(wm.current_version(&p), 0);
        assert_eq!(wm.next_version(&p), 1);
        assert_eq!(wm.current_version(&p), 1);
    }

    #[test]
    fn test_track_new_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Arc::new(NeodosLspConfig::default());
        *cfg.workspace.roots.write() = vec![dir.path().to_path_buf()];

        let wm = WorkspaceManager::new(cfg);
        wm.register_files(&[]);

        fs::write(dir.path().join("new.rs"), "fn x() {}").ok();
        let events = wm.poll_for_changes();
        assert!(events.iter().any(|(_, e)| matches!(e, FileEvent::Created(_))));
    }

    #[test]
    fn test_deleted_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("gone.rs");
        fs::write(&file, "fn x() {}").ok();

        let cfg = Arc::new(NeodosLspConfig::default());
        *cfg.workspace.roots.write() = vec![dir.path().to_path_buf()];

        let wm = WorkspaceManager::new(cfg);
        wm.register_files(&[file.clone()]);

        std::fs::remove_file(&file).ok();
        let events = wm.poll_for_changes();
        assert!(events.iter().any(|(_, e)| matches!(e, FileEvent::Deleted(_))));
    }
}
