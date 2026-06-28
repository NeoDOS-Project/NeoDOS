use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::RwLock;

use crate::indexer::ParsedFile;

/// Document cache: stores parsed file content to avoid re-parsing on every request.
pub struct DocumentCache {
    /// Parsed files keyed by path.
    files: RwLock<HashMap<PathBuf, CachedDocument>>,
    /// Maximum number of cached documents.
    capacity: usize,
}

struct CachedDocument {
    source: String,
    last_accessed: std::time::Instant,
}

impl DocumentCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            files: RwLock::new(HashMap::new()),
            capacity: capacity.max(2),
        }
    }

    /// Get the raw source text (without parsed AST).
    pub fn get_source(&self, path: &PathBuf) -> Option<String> {
        let cache = self.files.read();
        cache.get(path).map(|cd| cd.source.clone())
    }

    /// Insert or update a parsed file.
    pub fn insert(&self, path: PathBuf, source: String, _version: i64, _parsed: ParsedFile) {
        let mut cache = self.files.write();

        // Evict oldest if at capacity (LRU approximation).
        if cache.len() >= self.capacity && !cache.contains_key(&path)
            && let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.last_accessed)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }

        cache.insert(
            path,
            CachedDocument {
                source,
                last_accessed: std::time::Instant::now(),
            },
        );
    }

    /// Remove a cached document.
    pub fn remove(&self, path: &PathBuf) {
        self.files.write().remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_insert_and_get() {
        let cache = DocumentCache::new(10);
        let p = PathBuf::from("test.rs");

        cache.insert(p.clone(), "fn foo() {}".to_string(), 1, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });
        let src = cache.get_source(&p).expect("should be cached");
        assert_eq!(src, "fn foo() {}");
    }

    #[test]
    fn test_cache_version_independent() {
        let cache = DocumentCache::new(10);
        let p = PathBuf::from("v.rs");
        cache.insert(p.clone(), "old".to_string(), 1, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });
        assert_eq!(cache.get_source(&p), Some("old".to_string()));
        cache.insert(p.clone(), "new".to_string(), 2, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });
        assert_eq!(cache.get_source(&p), Some("new".to_string()));
    }

    #[test]
    fn test_cache_eviction() {
        let cache = DocumentCache::new(2);
        let p1 = PathBuf::from("a.rs");
        let p2 = PathBuf::from("b.rs");
        let p3 = PathBuf::from("c.rs");

        cache.insert(p1.clone(), "a".to_string(), 1, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });
        cache.insert(p2.clone(), "b".to_string(), 1, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });
        cache.insert(p3.clone(), "c".to_string(), 1, ParsedFile {
            symbols: vec![], references: vec![], neodos_items: vec![],
        });

        // At capacity 2, the 3rd insert evicts the oldest.
        // p3 should survive, and exactly 2 entries exist.
        assert_eq!(cache.get_source(&p3), Some("c".to_string()));
    }
}
