//! In-memory `WebDav` impl for tests. Models a flat key-value store of
//! `path → bytes` plus implicit directories — directories don't need
//! to be MKCOL'd before files inside them are PUT, which mirrors what
//! Nextcloud's WebDAV layer does in practice for the path tree we use.
//!
//! `Clone`-friendly via `Arc<Mutex<...>>` so two `Sync` instances
//! standing in for two devices can share the same "remote".

use super::webdav::{WebDav, WebDavError, WebDavResult};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FakeWebDav {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FakeWebDav {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test helper: how many files are currently stored, regardless of
    /// path. Useful for "X events are visible on the remote" assertions.
    #[allow(dead_code)]
    pub fn file_count(&self) -> usize {
        self.files.lock().unwrap().len()
    }

    /// Test helper: list every stored path. Sorted for deterministic
    /// assertions.
    #[allow(dead_code)]
    pub fn paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.files.lock().unwrap().keys().cloned().collect();
        v.sort();
        v
    }
}

fn norm(path: &str) -> String {
    // Treat leading slashes as optional and idempotent so callers don't
    // have to be picky about the exact form ("/x", "x", "/x/" all
    // address the same thing for path-prefix purposes).
    format!("/{}", path.trim_matches('/'))
}

impl WebDav for FakeWebDav {
    fn list_collection(&self, path: &str) -> WebDavResult<Vec<String>> {
        let prefix = format!("{}/", norm(path).trim_end_matches('/'));
        let files = self.files.lock().unwrap();
        let mut names = Vec::new();
        for full_path in files.keys() {
            if let Some(rest) = full_path.strip_prefix(&prefix) {
                // Direct children only, not deeper descendants.
                if !rest.contains('/') && !rest.is_empty() {
                    names.push(rest.to_string());
                }
            }
        }
        Ok(names)
    }

    fn get(&self, path: &str) -> WebDavResult<Vec<u8>> {
        let key = norm(path);
        self.files.lock().unwrap()
            .get(&key)
            .cloned()
            .ok_or(WebDavError::NotFound)
    }

    fn put(&self, path: &str, body: &[u8]) -> WebDavResult<()> {
        let key = norm(path);
        self.files.lock().unwrap().insert(key, body.to_vec());
        Ok(())
    }

    fn mkcol(&self, _path: &str) -> WebDavResult<()> {
        // Directories are implicit in our flat store. MKCOL is always
        // a no-op success — matches the spirit of "make sure this exists",
        // which is all the production caller cares about.
        Ok(())
    }

    fn delete(&self, path: &str) -> WebDavResult<()> {
        let key = norm(path);
        if self.files.lock().unwrap().remove(&key).is_some() {
            Ok(())
        } else {
            Err(WebDavError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_round_trips() {
        let fs = FakeWebDav::new();
        fs.put("/foo.json", b"hello").unwrap();
        assert_eq!(fs.get("/foo.json").unwrap(), b"hello");
    }

    #[test]
    fn put_overwrites_existing_file() {
        let fs = FakeWebDav::new();
        fs.put("/foo.json", b"v1").unwrap();
        fs.put("/foo.json", b"v2").unwrap();
        assert_eq!(fs.get("/foo.json").unwrap(), b"v2");
    }

    #[test]
    fn get_missing_path_returns_not_found() {
        let fs = FakeWebDav::new();
        assert!(matches!(fs.get("/missing").unwrap_err(), WebDavError::NotFound));
    }

    #[test]
    fn list_collection_returns_direct_children_only() {
        let fs = FakeWebDav::new();
        fs.put("/dir/a.json", b"").unwrap();
        fs.put("/dir/b.json", b"").unwrap();
        fs.put("/dir/sub/c.json", b"").unwrap();
        fs.put("/other/d.json", b"").unwrap();
        let mut names = fs.list_collection("/dir/").unwrap();
        names.sort();
        assert_eq!(names, vec!["a.json".to_string(), "b.json".to_string()]);
    }

    #[test]
    fn list_collection_normalises_trailing_slash_and_leading_slash() {
        // Catches "callers pass /x or /x/ or x/" inconsistencies. All
        // three must resolve to the same listing.
        let fs = FakeWebDav::new();
        fs.put("/dir/a.json", b"").unwrap();
        for variant in ["/dir/", "/dir", "dir/", "dir"] {
            let names = fs.list_collection(variant).unwrap();
            assert_eq!(names, vec!["a.json".to_string()],
                "variant `{variant}` should resolve identically");
        }
    }

    #[test]
    fn list_collection_empty_returns_empty_vec() {
        let fs = FakeWebDav::new();
        assert!(fs.list_collection("/empty/").unwrap().is_empty());
    }

    #[test]
    fn delete_removes_the_file() {
        let fs = FakeWebDav::new();
        fs.put("/x.json", b"").unwrap();
        fs.delete("/x.json").unwrap();
        assert!(matches!(fs.get("/x.json").unwrap_err(), WebDavError::NotFound));
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let fs = FakeWebDav::new();
        assert!(matches!(fs.delete("/x.json").unwrap_err(), WebDavError::NotFound));
    }

    #[test]
    fn clones_share_the_same_underlying_store() {
        // The whole point of the Arc<Mutex<...>>: clones see each
        // other's writes. Two devices' Sync instances share one fake.
        let fs_a = FakeWebDav::new();
        let fs_b = fs_a.clone();
        fs_a.put("/x.json", b"from a").unwrap();
        assert_eq!(fs_b.get("/x.json").unwrap(), b"from a");
    }
}
