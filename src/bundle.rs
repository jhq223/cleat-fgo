//! TextBundle (.script) reader compatible with cleat-fgo-helper.
//! Rijndael uses a 256-bit block; this format is not AES.
use flate2::read::ZlibDecoder;
use simple_rijndael::{
    impls::RijndaelCbc,
    paddings::{Padding, Pkcs7Padding},
};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read},
    path::Path,
};

const MAGIC: u32 = 0x7a684244;
const KEY: &[u8; 32] = b"mYq3t6v9y$B&E)H@McQfTjWnZr4u7x!z";
const IV: &[u8; 32] = b"TjWnZr4u7x!A%D*G-KaNdRgUkXp2s5v8";
pub const MAX_BUNDLE: u64 = 128 * 1024 * 1024;
pub const MAX_ENTRY: u64 = 8 * 1024 * 1024;
const MAX_TEXT: usize = 256 * 1024 * 1024;

struct CheckedPadding(usize);
impl Padding for CheckedPadding {
    fn new(size: usize) -> Self {
        Self(size)
    }
    fn size(&self) -> usize {
        self.0
    }
    fn encode(&self, data: Vec<u8>) -> Vec<u8> {
        Pkcs7Padding::new(self.0).encode(data)
    }
    fn decode(&self, mut data: Vec<u8>) -> Result<Vec<u8>, simple_rijndael::Errors> {
        let count = usize::from(
            *data
                .last()
                .ok_or(simple_rijndael::Errors::InvalidDataSize)?,
        );
        if count == 0
            || count > self.0
            || count > data.len()
            || !data.len().is_multiple_of(self.0)
            || !data[data.len() - count..]
                .iter()
                .all(|b| usize::from(*b) == count)
        {
            return Err(simple_rijndael::Errors::InvalidDataSize);
        }
        data.truncate(data.len() - count);
        Ok(data)
    }
}

#[derive(Default)]
pub struct ScriptBundle {
    entries: HashMap<String, String>,
}
impl ScriptBundle {
    pub fn load_dir(dir: &Path) -> Self {
        let mut result = Self::default();
        let Ok(files) = fs::read_dir(dir) else {
            return result;
        };
        let mut files: Vec<_> = files
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "script"))
            .collect();
        files.sort_by_key(|p| {
            (
                p.file_stem()
                    .and_then(|v| v.to_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0),
                p.clone(),
            )
        });
        for file in files {
            let parsed = (|| {
                if fs::metadata(&file).map_err(|e| e.to_string())?.len() > MAX_BUNDLE {
                    return Err("bundle too large".into());
                }
                Self::parse(&fs::read(&file).map_err(|e| e.to_string())?)
            })();
            match parsed {
                Ok(bundle) => {
                    if let Err(e) = result.merge(bundle) {
                        log::warn!("bundle: {}: {e}", file.display());
                    }
                }
                Err(e) => log::warn!("bundle: {}: {e}", file.display()),
            }
        }
        // Current community resources also publish individual UTF-16/UTF-8
        // scripts. These override legacy bundles and keep local edits simple.
        if let Ok(files) = fs::read_dir(dir.join("Script")) {
            let mut plain = Self::default();
            let mut total = 0usize;
            let mut files: Vec<_> = files.flatten().map(|e| e.path()).collect();
            files.sort();
            for file in files {
                let Some(id) = file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(script_id)
                else {
                    continue;
                };
                let parsed = (|| {
                    if fs::metadata(&file).map_err(|e| e.to_string())?.len() > MAX_ENTRY {
                        return Err("script too large".into());
                    }
                    decode_script(&fs::read(&file).map_err(|e| e.to_string())?)
                })();
                match parsed {
                    Ok(text) => {
                        total += text.len();
                        if total > MAX_TEXT {
                            log::warn!("plain story text exceeds memory budget");
                            break;
                        }
                        plain.entries.insert(id.to_owned(), text);
                    }
                    Err(e) => log::warn!("story {}: {e}", file.display()),
                }
            }
            if let Err(e) = result.merge(plain) {
                log::warn!("stories: {e}");
            }
        }
        result
    }
    pub fn merge(&mut self, other: Self) -> Result<(), String> {
        let added = other.entries.values().map(String::len).sum::<usize>();
        let existing = self.entries.values().map(String::len).sum::<usize>();
        let replaced = other
            .entries
            .keys()
            .filter_map(|key| self.entries.get(key))
            .map(String::len)
            .sum::<usize>();
        if added.saturating_add(existing - replaced) > MAX_TEXT {
            return Err("combined story text exceeds memory budget".into());
        }
        self.entries.extend(other.entries);
        Ok(())
    }
    pub fn get(&self, id: &str) -> Option<&str> {
        self.entries.get(id).map(String::as_str)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() as u64 > MAX_BUNDLE {
            return Err("bundle too large".into());
        }
        let mut cursor = Cursor::new(data);
        if number(&mut cursor)? != MAGIC {
            return Err("invalid TextBundle magic".into());
        }
        let count = number(&mut cursor)? as usize;
        if count > 100_000 || count > data.len().saturating_sub(8) / 14 {
            return Err("invalid TextBundle entry count".into());
        }
        let mut infos = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = number(&mut cursor)? as usize;
            let mut length = [0];
            cursor.read_exact(&mut length).map_err(|e| e.to_string())?;
            if length[0] == 0 {
                return Err("empty story ID".into());
            }
            let mut id = vec![0; length[0] as usize];
            cursor.read_exact(&mut id).map_err(|e| e.to_string())?;
            let id = String::from_utf8(id).map_err(|e| e.to_string())?;
            let size = number(&mut cursor)? as usize;
            // Legacy writers use zero here. SHA-256 is checked by the updater;
            // do not invent a checksum interpretation for older bundles.
            let _reserved_crc = number(&mut cursor)?;
            infos.push((id, offset, size));
        }
        let header_end = cursor.position() as usize;
        let mut result = Self::default();
        let mut total = 0usize;
        for (id, start, size) in infos {
            let end = start.checked_add(size).ok_or("entry offset overflow")?;
            if start < header_end || size == 0 || !size.is_multiple_of(32) || end > data.len() {
                return Err(format!("{id}: invalid encrypted entry bounds"));
            }
            let cipher = RijndaelCbc::<CheckedPadding>::new(KEY, 32)
                .map_err(|e| format!("cipher: {e:?}"))?;
            let decrypted = cipher
                .decrypt(IV, data[start..end].to_vec())
                .map_err(|e| format!("{id}: decrypt: {e:?}"))?;
            let mut bytes = Vec::new();
            ZlibDecoder::new(decrypted.as_slice())
                .take(MAX_ENTRY + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_ENTRY {
                return Err(format!("{id}: expanded text exceeds limit"));
            }
            let text = decode_utf16le(&bytes)?;
            total = total.checked_add(text.len()).ok_or("text size overflow")?;
            if total > MAX_TEXT {
                return Err("expanded bundle exceeds limit".into());
            }
            if result.entries.insert(id, text).is_some() {
                return Err("duplicate story ID".into());
            }
        }
        Ok(result)
    }
}
pub fn script_id(name: &str) -> Option<&str> {
    let id = name.strip_suffix(".txt").unwrap_or(name);
    ((9..=10).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}
pub fn decode_script(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() as u64 > MAX_ENTRY {
        return Err("script exceeds size limit".into());
    }
    let text = decode_text(bytes)?;
    if !text.trim_start().starts_with(['＄', '$'])
        && !text.trim_start().starts_with("[soundStopAll]")
    {
        return Err("script is missing its scenario header".into());
    }
    Ok(text)
}
pub fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.iter().take(100).any(|b| *b == 0) {
        decode_utf16le(bytes)
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|e| e.to_string())
    }
}
fn number(cursor: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut bytes = [0; 4];
    cursor.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    Ok(u32::from_be_bytes(bytes))
}
pub fn decode_utf16le(bytes: &[u8]) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("odd UTF-16 byte count".into());
    }
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    let words: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&words).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn bundle(text: &str) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        for word in text.encode_utf16() {
            encoder.write_all(&word.to_le_bytes()).unwrap();
        }
        let compressed = encoder.finish().unwrap();
        let cipher = RijndaelCbc::<Pkcs7Padding>::new(KEY, 32).unwrap();
        let bytes = cipher.encrypt(IV, compressed).unwrap();
        let mut file = Vec::new();
        file.extend(MAGIC.to_be_bytes());
        file.extend(1u32.to_be_bytes());
        file.extend(24u32.to_be_bytes());
        file.push(3);
        file.extend(b"001");
        file.extend((bytes.len() as u32).to_be_bytes());
        file.extend(0u32.to_be_bytes());
        file.extend(bytes);
        file
    }
    #[test]
    fn roundtrip_and_reject_corrupt_bundles() {
        let good = bundle("中文剧情\n😀[r]");
        assert_eq!(
            ScriptBundle::parse(&good).unwrap().get("001"),
            Some("中文剧情\n😀[r]")
        );
        for end in 0..good.len() {
            assert!(ScriptBundle::parse(&good[..end]).is_err());
        }
        let mut bad = good.clone();
        bad[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(ScriptBundle::parse(&bad).is_err());
        let mut bad = good;
        bad[8..12].copy_from_slice(&0u32.to_be_bytes());
        assert!(ScriptBundle::parse(&bad).is_err());
        assert!(decode_utf16le(&[1]).is_err());
    }
    #[test]
    fn merges_chapters_and_newer_overrides() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("20260101.script"), bundle("旧译文")).unwrap();
        fs::write(dir.path().join("20261019.script"), bundle("新译文")).unwrap();
        fs::write(dir.path().join("broken.script"), b"bad").unwrap();
        assert_eq!(
            ScriptBundle::load_dir(dir.path()).get("001"),
            Some("新译文")
        );
    }
}
