use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Metadata for a single bar cache file discovered during a scan.
pub struct BarFileInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub last_modified: SystemTime,
}

/// Aggregate statistics for the bar cache directory.
pub struct BarStoreStats {
    pub total_size_bytes: u64,
    pub file_count: usize,
    pub oldest_modified: Option<SystemTime>,
    pub newest_modified: Option<SystemTime>,
    /// Per-exchange `(file_count, total_bytes)`.
    pub per_exchange: HashMap<String, (usize, u64)>,
}

/// Bar cache cleanup utility.
///
/// Scans `bars/{exchange}/SYMBOL_TF.bin` files, evicts stale entries by age,
/// and trims to a maximum total size using LRU-by-mtime eviction.
pub struct BarStoreCleanup {
    bars_dir: PathBuf,
}

impl BarStoreCleanup {
    pub fn new(bars_dir: PathBuf) -> Self {
        Self { bars_dir }
    }

    /// Scan all `.bin` files under `bars_dir` recursively (one level of subdirs).
    pub fn scan(&self) -> Vec<BarFileInfo> {
        let mut files = Vec::new();
        let entries = match std::fs::read_dir(&self.bars_dir) {
            Ok(e) => e,
            Err(_) => return files,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Exchange subdirectory: bars/{exchange}/
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().map(|e| e == "bin").unwrap_or(false) {
                            if let Ok(meta) = std::fs::metadata(&sub_path) {
                                files.push(BarFileInfo {
                                    path: sub_path,
                                    size_bytes: meta.len(),
                                    last_modified: meta.modified().unwrap_or(UNIX_EPOCH),
                                });
                            }
                        }
                    }
                }
            } else if path.extension().map(|e| e == "bin").unwrap_or(false) {
                if let Ok(meta) = std::fs::metadata(&path) {
                    files.push(BarFileInfo {
                        path,
                        size_bytes: meta.len(),
                        last_modified: meta.modified().unwrap_or(UNIX_EPOCH),
                    });
                }
            }
        }
        files
    }

    /// Remove stale files (older than `max_age_days`) then trim by LRU until
    /// total size is within `max_size_mb`.
    pub fn run_cleanup(&self, max_size_mb: u32, max_age_days: u32) {
        let mut files = self.scan();
        if files.is_empty() {
            return;
        }

        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(max_age_days as u64 * 86_400))
            .unwrap_or(UNIX_EPOCH);

        // Pass 1: delete stale files
        let before = files.len();
        files.retain(|f| {
            if f.last_modified < cutoff {
                let _ = std::fs::remove_file(&f.path);
                false
            } else {
                true
            }
        });
        let stale_removed = before - files.len();

        // Pass 2: LRU size eviction
        let max_bytes = max_size_mb as u64 * 1024 * 1024;
        let total: u64 = files.iter().map(|f| f.size_bytes).sum();
        let mut lru_removed = 0usize;
        if total > max_bytes {
            // Sort oldest first so we evict least-recently-used first.
            files.sort_by_key(|f| f.last_modified);
            let mut running = total;
            for f in &files {
                if running <= max_bytes {
                    break;
                }
                if std::fs::remove_file(&f.path).is_ok() {
                    running = running.saturating_sub(f.size_bytes);
                    lru_removed += 1;
                }
            }
        }

        if stale_removed > 0 || lru_removed > 0 {
            eprintln!(
                "[BarStore] Cleanup: removed {} stale + {} LRU files",
                stale_removed, lru_removed
            );
        }
    }

    /// Collect aggregate statistics for the bar cache directory.
    pub fn stats(&self) -> BarStoreStats {
        let files = self.scan();
        let total_size_bytes: u64 = files.iter().map(|f| f.size_bytes).sum();
        let oldest = files.iter().map(|f| f.last_modified).min();
        let newest = files.iter().map(|f| f.last_modified).max();

        let mut per_exchange: HashMap<String, (usize, u64)> = HashMap::new();
        for f in &files {
            // Extract exchange name from path: bars/{exchange}/file.bin
            let exchange = f
                .path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let entry = per_exchange.entry(exchange).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += f.size_bytes;
        }

        BarStoreStats {
            total_size_bytes,
            file_count: files.len(),
            oldest_modified: oldest,
            newest_modified: newest,
            per_exchange,
        }
    }

    /// Delete every cached bar file under `bars_dir`.
    pub fn clear_all(&self) {
        let files = self.scan();
        let mut removed = 0usize;
        for f in &files {
            if std::fs::remove_file(&f.path).is_ok() {
                removed += 1;
            }
        }
        eprintln!("[BarStore] Cleared all: removed {} files", removed);
    }
}
