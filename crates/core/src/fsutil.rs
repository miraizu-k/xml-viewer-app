//! 設定や履歴のファイルの書き込み。

use std::fs;
use std::io;
use std::path::Path;

/// ファイルを書き換える。書きかけのファイルを読まれないよう、別名で書いてから置き換える。
/// 置き場所のフォルダが無ければ作る。
pub(crate) fn write_replacing(file: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, file)
}
