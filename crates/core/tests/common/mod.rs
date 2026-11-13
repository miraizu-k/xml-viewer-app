//! 結合テストの共通部品。
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use xml_viewer_core::xslt::{TransformError, Transformer};

/// テストごとの作業フォルダ（OS の一時フォルダの下）。破棄すると消える。
pub struct Work(pub PathBuf);

impl Work {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("xmlv-it-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// テスト用のダミー文書のフォルダ。
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// フォルダを中身ごとコピーする。
pub fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// フォルダの中身を `prefix/` の下に入れた zip を作る。
pub fn zip_dir(src: &Path, prefix: &str, out: &Path) {
    let file = fs::File::create(out).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    zip.add_directory(format!("{prefix}/"), opts).unwrap();
    let mut entries: Vec<_> = fs::read_dir(src).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        let name = format!("{prefix}/{}", path.file_name().unwrap().to_string_lossy());
        zip.start_file(name, opts).unwrap();
        zip.write_all(&fs::read(&path).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

/// 名前をバイト列のまま書き込む、無圧縮の zip を作る（Shift_JIS の名前や、危険な名前の再現用）。
pub fn raw_zip(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes()); // version made by
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0u16.to_le_bytes()); // flags（UTF-8 フラグなし）
            h.extend_from_slice(&0u16.to_le_bytes()); // 無圧縮
            h.extend_from_slice(&0u32.to_le_bytes()); // 日時
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&0u16.to_le_bytes()); // comment
                h.extend_from_slice(&0u16.to_le_bytes()); // disk
                h.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
                h.extend_from_slice(&0u32.to_le_bytes()); // external attrs
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name);
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(data);
        central.extend(header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// MSXML の代わりに使う変換。XML と XSL の名前をタイトルに入れた HTML を返す。
/// `broken.xsl` だけは失敗する。
pub struct FakeTransformer;

impl Transformer for FakeTransformer {
    fn transform(&self, xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        let xml_name = xml.file_name().unwrap().to_string_lossy();
        let xsl_name = xsl.file_name().unwrap().to_string_lossy();
        if xsl_name == "broken.xsl" {
            let mut e = TransformError::new(Some(xsl), "終了タグが一致しません。");
            e.line = Some(8);
            return Err(e);
        }
        Ok(format!("<html><head><title>{xml_name} by {xsl_name}</title></head><body></body></html>"))
    }
}
