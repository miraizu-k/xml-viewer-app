//! 最近開いたファイルの記録。
//!
//! 開いたパスと日時だけを記録する。文書の中身は保存しない。
//! 複数のウィンドウから書かれることがあるので、書く直前に読み直してから書く（厳密な排他はしない）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil::write_replacing;
use crate::pathutil::same_path;

/// 記録する件数の上限。
pub const MAX_ENTRIES: usize = 20;

/// 履歴の 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub path: String,
    /// 最後に開いた日時（UNIX 時間の秒）。
    pub opened_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentFile {
    version: u32,
    items: Vec<RecentEntry>,
}

/// 履歴ファイル。
#[derive(Debug, Clone)]
pub struct RecentStore {
    file: PathBuf,
}

impl RecentStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// 新しい順の一覧。ファイルが無い、または壊れていれば空。
    pub fn load(&self) -> Vec<RecentEntry> {
        fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RecentFile>(&bytes).ok())
            .map(|f| f.items)
            .unwrap_or_default()
    }

    /// 開いたパスを先頭に記録する。同じパスがあれば先頭に移す。
    pub fn add(&self, path: &Path, now: u64) -> io::Result<Vec<RecentEntry>> {
        let mut items = self.load();
        items.retain(|e| !same_path(Path::new(&e.path), path));
        items.insert(0, RecentEntry { path: path.to_string_lossy().into_owned(), opened_at: now });
        items.truncate(MAX_ENTRIES);
        self.save(&items)?;
        Ok(items)
    }

    /// 1 件消す。
    pub fn remove(&self, path: &Path) -> io::Result<Vec<RecentEntry>> {
        let mut items = self.load();
        items.retain(|e| !same_path(Path::new(&e.path), path));
        self.save(&items)?;
        Ok(items)
    }

    /// 全部消す。
    pub fn clear(&self) -> io::Result<()> {
        self.save(&[])
    }

    fn save(&self, items: &[RecentEntry]) -> io::Result<()> {
        let data = RecentFile { version: 1, items: items.to_vec() };
        let json = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
        write_replacing(&self.file, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (RecentStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("xmlv-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (RecentStore::new(dir.join("recent.json")), dir)
    }

    #[test]
    fn adds_to_front_and_moves_duplicates() {
        let (s, dir) = store("add");
        s.add(Path::new("C:\\a.zip"), 1).unwrap();
        s.add(Path::new("C:\\b.zip"), 2).unwrap();
        let items = s.add(Path::new("c:\\A.ZIP"), 3).unwrap();
        assert_eq!(items.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["c:\\A.ZIP", "C:\\b.zip"]);
        assert_eq!(s.load(), items);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn keeps_at_most_twenty() {
        let (s, dir) = store("max");
        for i in 0..21 {
            s.add(Path::new(&format!("C:\\{i}.zip")), i).unwrap();
        }
        let items = s.load();
        assert_eq!(items.len(), MAX_ENTRIES);
        assert_eq!(items[0].path, "C:\\20.zip");
        assert!(items.iter().all(|e| e.path != "C:\\0.zip"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn removes_and_clears() {
        let (s, dir) = store("remove");
        s.add(Path::new("C:\\a.zip"), 1).unwrap();
        s.add(Path::new("C:\\b.zip"), 2).unwrap();
        assert_eq!(s.remove(Path::new("C:\\a.zip")).unwrap().len(), 1);
        s.clear().unwrap();
        assert!(s.load().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_file_reads_as_empty() {
        let (s, dir) = store("broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("recent.json"), "{not json").unwrap();
        assert!(s.load().is_empty());
        assert_eq!(s.add(Path::new("C:\\a.zip"), 1).unwrap().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
