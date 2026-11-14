//! アプリの設定。
//!
//! 起動した直後（ウィンドウを作る前）に読む設定があるので、画面の localStorage ではなくファイルに置く。
//! エクスプローラー連携のスイッチは、実際の登録を見て表示するので、ここには保存しない。

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fsutil::write_replacing;

/// 設定の値。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// ウィンドウを 1 つにまとめる。後から起動したときに渡されたファイルを、動いているウィンドウで開く。
    pub single_window: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SettingsFile {
    version: u32,
    single_window: bool,
}

/// 設定のファイル。
#[derive(Debug, Clone)]
pub struct SettingsStore {
    file: PathBuf,
}

impl SettingsStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// 保存してある設定。ファイルが無い、または壊れていれば既定の値（どれも切）。
    pub fn load(&self) -> Settings {
        fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SettingsFile>(&bytes).ok())
            .map(|f| Settings { single_window: f.single_window })
            .unwrap_or_default()
    }

    /// 設定を保存する。
    pub fn save(&self, settings: Settings) -> io::Result<()> {
        let data = SettingsFile { version: 1, single_window: settings.single_window };
        let json = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
        write_replacing(&self.file, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (SettingsStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("xmlv-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (SettingsStore::new(dir.join("settings.json")), dir)
    }

    #[test]
    fn missing_file_reads_as_default() {
        let (s, dir) = store("missing");
        assert_eq!(s.load(), Settings { single_window: false });
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saves_and_loads() {
        let (s, dir) = store("save");
        s.save(Settings { single_window: true }).unwrap();
        assert_eq!(s.load(), Settings { single_window: true });
        s.save(Settings { single_window: false }).unwrap();
        assert_eq!(s.load(), Settings { single_window: false });
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_file_reads_as_default() {
        let (s, dir) = store("broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.json"), "{not json").unwrap();
        assert_eq!(s.load(), Settings::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ignores_unknown_and_missing_fields() {
        let (s, dir) = store("fields");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.json"), r#"{"version":2,"singleWindow":true,"later":1}"#).unwrap();
        assert_eq!(s.load(), Settings { single_window: true });
        fs::write(dir.join("settings.json"), r#"{"version":1}"#).unwrap();
        assert_eq!(s.load(), Settings { single_window: false });
        let _ = fs::remove_dir_all(dir);
    }
}
