use gitfleet_core::{AppError, AppResult, DocumentId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub const fn all() -> &'static [Self] {
        &[Self::Lf, Self::Crlf]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Lf => "Unix / macOS (\\n)",
            Self::Crlf => "Windows (\\r\\n)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FileEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
    Iso88591,
    Ascii,
}

impl FileEncoding {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Utf8,
            Self::Utf8Bom,
            Self::Utf16Le,
            Self::Utf16Be,
            Self::Windows1252,
            Self::Iso88591,
            Self::Ascii,
        ]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 with BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Windows1252 => "Windows 1252",
            Self::Iso88591 => "ISO 8859-1",
            Self::Ascii => "ASCII",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Utf8 => "Unicode UTF-8 (no BOM)",
            Self::Utf8Bom => "Unicode UTF-8 with byte order mark",
            Self::Utf16Le => "Unicode UTF-16 Little Endian (Windows)",
            Self::Utf16Be => "Unicode UTF-16 Big Endian",
            Self::Windows1252 => "Western European (CP1252)",
            Self::Iso88591 => "Western European (Latin-1)",
            Self::Ascii => "7-bit US-ASCII",
        }
    }
}

/// Official WHATWG/Microsoft CP1252 mapping for bytes `0x80..=0x9F`.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut chunks = bytes.chunks_exact(2);
    for pair in &mut chunks {
        let u = if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        };
        units.push(u);
    }
    if !chunks.remainder().is_empty() {
        units.push(0xFFFD);
    }
    String::from_utf16_lossy(&units)
}

fn decode_cp1252(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if (0x80..=0x9F).contains(&b) {
            out.push(CP1252_HIGH[(b - 0x80) as usize]);
        } else {
            out.push(b as char);
        }
    }
    out
}

fn decode_latin1(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        out.push(b as char);
    }
    out
}

fn decode_ascii(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b <= 0x7F {
            out.push(b as char);
        } else {
            out.push('\u{FFFD}');
        }
    }
    out
}

fn encode_cp1252_char(ch: char) -> Option<u8> {
    let cp = ch as u32;
    if cp <= 0x7F || (0xA0..=0xFF).contains(&cp) {
        return Some(cp as u8);
    }
    for (idx, &mapped) in CP1252_HIGH.iter().enumerate() {
        if mapped == ch {
            return Some(0x80 + idx as u8);
        }
    }
    None
}

/// Decodes raw bytes using an explicit `FileEncoding`.
pub fn decode_bytes_with_encoding(bytes: &[u8], encoding: FileEncoding) -> String {
    match encoding {
        FileEncoding::Utf8 => {
            let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            String::from_utf8_lossy(body).into_owned()
        }
        FileEncoding::Utf8Bom => {
            let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            String::from_utf8_lossy(body).into_owned()
        }
        FileEncoding::Utf16Le => {
            let body = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
            decode_utf16(body, true)
        }
        FileEncoding::Utf16Be => {
            let body = bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes);
            decode_utf16(body, false)
        }
        FileEncoding::Windows1252 => decode_cp1252(bytes),
        FileEncoding::Iso88591 => decode_latin1(bytes),
        FileEncoding::Ascii => decode_ascii(bytes),
    }
}

/// Auto-detects file encoding and decodes text bytes. Rejects binary files (containing NUL bytes
/// without a UTF-16 BOM) with `InvalidData` so images and binaries continue to preview natively.
pub fn detect_and_decode(bytes: &[u8]) -> AppResult<(FileEncoding, String)> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let s = std::str::from_utf8(rest).map_err(|e| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 BOM file: {e}"),
            ))
        })?;
        return Ok((FileEncoding::Utf8Bom, s.to_string()));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((FileEncoding::Utf16Le, decode_utf16(rest, true)));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((FileEncoding::Utf16Be, decode_utf16(rest, false)));
    }

    let probe_len = bytes.len().min(8192);
    if bytes[..probe_len].contains(&0) {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Binary file detected (contains NUL bytes)",
        )));
    }

    match std::str::from_utf8(bytes) {
        Ok(s) => Ok((FileEncoding::Utf8, s.to_string())),
        Err(_) => Ok((FileEncoding::Windows1252, decode_cp1252(bytes))),
    }
}

/// Detects the dominant end-of-line sequence in decoded text (`CRLF` vs `LF`).
pub fn detect_line_ending(decoded: &str) -> LineEnding {
    let mut crlf_count = 0usize;
    let mut lf_count = 0usize;
    let bytes = decoded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            crlf_count += 1;
            i += 2;
        } else if bytes[i] == b'\n' {
            lf_count += 1;
            i += 1;
        } else {
            i += 1;
        }
    }
    if crlf_count > 0 && crlf_count >= lf_count {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    }
}

/// Encodes normalized text using the target `LineEnding` and `FileEncoding`.
/// Returns `(encoded_bytes, lossy_char_count)` where `lossy_char_count` is the number of
/// characters replaced with `?` in single-byte encodings (`ASCII`, `ISO 8859-1`, `Windows 1252`).
pub fn encode_text(
    content: &str,
    line_ending: LineEnding,
    encoding: FileEncoding,
) -> (Vec<u8>, usize) {
    let normalized = content.replace("\r\n", "\n");
    let with_eol = match line_ending {
        LineEnding::Crlf => normalized.replace('\n', "\r\n"),
        LineEnding::Lf => normalized,
    };

    let mut lossy = 0usize;
    let bytes = match encoding {
        FileEncoding::Utf8 => with_eol.into_bytes(),
        FileEncoding::Utf8Bom => {
            let mut out = Vec::with_capacity(3 + with_eol.len());
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            out.extend_from_slice(with_eol.as_bytes());
            out
        }
        FileEncoding::Utf16Le => {
            let mut out = Vec::with_capacity(2 + with_eol.len() * 2);
            out.extend_from_slice(&[0xFF, 0xFE]);
            for u in with_eol.encode_utf16() {
                out.extend_from_slice(&u.to_le_bytes());
            }
            out
        }
        FileEncoding::Utf16Be => {
            let mut out = Vec::with_capacity(2 + with_eol.len() * 2);
            out.extend_from_slice(&[0xFE, 0xFF]);
            for u in with_eol.encode_utf16() {
                out.extend_from_slice(&u.to_be_bytes());
            }
            out
        }
        FileEncoding::Windows1252 => {
            let mut out = Vec::with_capacity(with_eol.len());
            for ch in with_eol.chars() {
                if let Some(b) = encode_cp1252_char(ch) {
                    out.push(b);
                } else {
                    lossy += 1;
                    out.push(b'?');
                }
            }
            out
        }
        FileEncoding::Iso88591 => {
            let mut out = Vec::with_capacity(with_eol.len());
            for ch in with_eol.chars() {
                let cp = ch as u32;
                if cp <= 0xFF {
                    out.push(cp as u8);
                } else {
                    lossy += 1;
                    out.push(b'?');
                }
            }
            out
        }
        FileEncoding::Ascii => {
            let mut out = Vec::with_capacity(with_eol.len());
            for ch in with_eol.chars() {
                let cp = ch as u32;
                if cp <= 0x7F {
                    out.push(cp as u8);
                } else {
                    lossy += 1;
                    out.push(b'?');
                }
            }
            out
        }
    };

    (bytes, lossy)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenDocumentCapability {
    pub document_id: DocumentId,
    pub version: String,
    pub line_ending: LineEnding,
    pub encoding: FileEncoding,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SaveDocumentResult {
    pub version: String,
    pub line_ending: LineEnding,
    pub encoding: FileEncoding,
    pub lossy_chars: usize,
    pub content: String,
}

struct TrackedDocument {
    path: PathBuf,
    version: String,
    line_ending: LineEnding,
    encoding: FileEncoding,
}

/// Capability-based document editor: UI holds an opaque `DocumentId`, never writes by raw path.
#[derive(Default)]
pub struct DocsService {
    open_docs: RwLock<HashMap<DocumentId, TrackedDocument>>,
}

fn compute_doc_version_bytes(raw: &[u8]) -> String {
    // FNV-1a 64-bit hash + byte length so even same-length or BOM/EOL edits produce a new version
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in raw {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("len:{}:h:{hash:016x}", raw.len())
}

/// Checks whether `path` on disk currently differs from `expected_version` without registering
/// a new `DocumentId` handle.
pub fn has_disk_version_changed(path: &Path, expected_version: &str) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    compute_doc_version_bytes(&raw) != expected_version
}

impl DocsService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_document(&self, path: &Path) -> AppResult<OpenDocumentCapability> {
        let raw_bytes = std::fs::read(path)?;
        let (encoding, decoded) = detect_and_decode(&raw_bytes)?;
        self.register_opened_doc(path, &raw_bytes, encoding, decoded)
    }

    /// Re-reads raw bytes from disk and decodes them with an explicit `FileEncoding`
    /// without modifying the file on disk.
    pub fn open_document_with_encoding(
        &self,
        path: &Path,
        encoding: FileEncoding,
    ) -> AppResult<OpenDocumentCapability> {
        let raw_bytes = std::fs::read(path)?;
        let decoded = decode_bytes_with_encoding(&raw_bytes, encoding);
        self.register_opened_doc(path, &raw_bytes, encoding, decoded)
    }

    fn register_opened_doc(
        &self,
        path: &Path,
        raw_bytes: &[u8],
        encoding: FileEncoding,
        decoded: String,
    ) -> AppResult<OpenDocumentCapability> {
        let line_ending = detect_line_ending(&decoded);
        let normalized = decoded.replace("\r\n", "\n");
        let version = compute_doc_version_bytes(raw_bytes);
        let document_id = DocumentId::new();

        if let Ok(mut guard) = self.open_docs.write() {
            guard.insert(
                document_id.clone(),
                TrackedDocument {
                    path: path.to_path_buf(),
                    version: version.clone(),
                    line_ending,
                    encoding,
                },
            );
        }

        Ok(OpenDocumentCapability {
            document_id,
            version,
            line_ending,
            encoding,
            content: normalized,
        })
    }

    pub fn set_document_line_ending(
        &self,
        document_id: &DocumentId,
        line_ending: LineEnding,
    ) -> AppResult<()> {
        let mut guard = self
            .open_docs
            .write()
            .map_err(|_| AppError::Io(std::io::Error::other("lock poisoned")))?;
        let tracked = guard.get_mut(document_id).ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "document capability not found",
            ))
        })?;
        tracked.line_ending = line_ending;
        Ok(())
    }

    pub fn set_document_encoding(
        &self,
        document_id: &DocumentId,
        encoding: FileEncoding,
    ) -> AppResult<()> {
        let mut guard = self
            .open_docs
            .write()
            .map_err(|_| AppError::Io(std::io::Error::other("lock poisoned")))?;
        let tracked = guard.get_mut(document_id).ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "document capability not found",
            ))
        })?;
        tracked.encoding = encoding;
        Ok(())
    }

    pub fn save_document(
        &self,
        document_id: &DocumentId,
        content: &str,
        expected_version: &str,
    ) -> AppResult<String> {
        let res = self.save_document_detailed(document_id, content, expected_version, None, None)?;
        Ok(res.version)
    }

    /// Saves `content` to the tracked document, optionally overriding `line_ending` and/or
    /// `encoding`, verifying the on-disk version first.
    pub fn save_document_detailed(
        &self,
        document_id: &DocumentId,
        content: &str,
        expected_version: &str,
        line_ending_override: Option<LineEnding>,
        encoding_override: Option<FileEncoding>,
    ) -> AppResult<SaveDocumentResult> {
        let mut guard = self
            .open_docs
            .write()
            .map_err(|_| AppError::Io(std::io::Error::other("lock poisoned")))?;
        let tracked = guard.get_mut(document_id).ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "document capability not found",
            ))
        })?;

        let disk_raw = std::fs::read(&tracked.path)?;
        let actual_version = compute_doc_version_bytes(&disk_raw);
        if actual_version != expected_version {
            return Err(AppError::DocumentConflict {
                expected: expected_version.to_string(),
                actual: actual_version,
            });
        }

        let target_eol = line_ending_override.unwrap_or(tracked.line_ending);
        let target_enc = encoding_override.unwrap_or(tracked.encoding);
        let (encoded_bytes, lossy_chars) = encode_text(content, target_eol, target_enc);
        std::fs::write(&tracked.path, &encoded_bytes)?;

        let new_version = compute_doc_version_bytes(&encoded_bytes);
        let reloaded_content =
            decode_bytes_with_encoding(&encoded_bytes, target_enc).replace("\r\n", "\n");
        tracked.line_ending = target_eol;
        tracked.encoding = target_enc;
        tracked.version = new_version.clone();

        Ok(SaveDocumentResult {
            version: new_version,
            line_ending: target_eol,
            encoding: target_enc,
            lossy_chars,
            content: reloaded_content,
        })
    }

    /// Converts an unmodified document directly on disk from its currently tracked encoding & EOL
    /// to `(new_line_ending, new_encoding)`, preserving exact trailing-newline presence/absence.
    pub fn convert_document_on_disk(
        &self,
        document_id: &DocumentId,
        expected_version: &str,
        line_ending: LineEnding,
        encoding: FileEncoding,
    ) -> AppResult<SaveDocumentResult> {
        let mut guard = self
            .open_docs
            .write()
            .map_err(|_| AppError::Io(std::io::Error::other("lock poisoned")))?;
        let tracked = guard.get_mut(document_id).ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "document capability not found",
            ))
        })?;

        let disk_raw = std::fs::read(&tracked.path)?;
        let actual_version = compute_doc_version_bytes(&disk_raw);
        if actual_version != expected_version {
            return Err(AppError::DocumentConflict {
                expected: expected_version.to_string(),
                actual: actual_version,
            });
        }

        let decoded = decode_bytes_with_encoding(&disk_raw, tracked.encoding);
        let (encoded_bytes, lossy_chars) = encode_text(&decoded, line_ending, encoding);
        std::fs::write(&tracked.path, &encoded_bytes)?;

        let new_version = compute_doc_version_bytes(&encoded_bytes);
        let reloaded_content =
            decode_bytes_with_encoding(&encoded_bytes, encoding).replace("\r\n", "\n");
        tracked.line_ending = line_ending;
        tracked.encoding = encoding;
        tracked.version = new_version.clone();

        Ok(SaveDocumentResult {
            version: new_version,
            line_ending,
            encoding,
            lossy_chars,
            content: reloaded_content,
        })
    }

    pub fn save_document_as(
        &self,
        dest: &Path,
        content: &str,
        line_ending: LineEnding,
        encoding: FileEncoding,
    ) -> AppResult<usize> {
        let (encoded, lossy) = encode_text(content, line_ending, encoding);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, encoded)?;
        Ok(lossy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_converts_line_endings_and_encodings_preserving_exact_content() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_docs_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = DocsService::new();
        let file = dir.join("sample.txt");

        // 1. UTF-8 LF without trailing newline
        std::fs::write(&file, "hello\nworld").unwrap();
        let cap = svc.open_document(&file).unwrap();
        assert_eq!(cap.encoding, FileEncoding::Utf8);
        assert_eq!(cap.line_ending, LineEnding::Lf);
        assert_eq!(cap.content, "hello\nworld");

        // 2. Convert LF -> CRLF on disk without adding phantom trailing newline
        let r1 = svc
            .convert_document_on_disk(&cap.document_id, &cap.version, LineEnding::Crlf, FileEncoding::Utf8)
            .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"hello\r\nworld");
        assert_eq!(r1.line_ending, LineEnding::Crlf);
        assert_eq!(r1.encoding, FileEncoding::Utf8);
        assert_eq!(r1.lossy_chars, 0);

        // 3. Convert to UTF-8 with BOM
        let r2 = svc
            .convert_document_on_disk(&cap.document_id, &r1.version, LineEnding::Crlf, FileEncoding::Utf8Bom)
            .unwrap();
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"\xEF\xBB\xBFhello\r\nworld"
        );
        let reopened_bom = svc.open_document(&file).unwrap();
        assert_eq!(reopened_bom.encoding, FileEncoding::Utf8Bom);
        assert_eq!(reopened_bom.line_ending, LineEnding::Crlf);
        assert_eq!(reopened_bom.content, "hello\nworld");

        // 4. Convert to UTF-16 LE with Unicode text
        let r3 = svc
            .save_document_detailed(
                &cap.document_id,
                "Xin chào\nGitFleet",
                &r2.version,
                Some(LineEnding::Lf),
                Some(FileEncoding::Utf16Le),
            )
            .unwrap();
        let raw_u16 = std::fs::read(&file).unwrap();
        assert!(raw_u16.starts_with(&[0xFF, 0xFE]));
        let reopened_u16 = svc.open_document(&file).unwrap();
        assert_eq!(reopened_u16.encoding, FileEncoding::Utf16Le);
        assert_eq!(reopened_u16.line_ending, LineEnding::Lf);
        assert_eq!(reopened_u16.content, "Xin chào\nGitFleet");
        assert_eq!(r3.lossy_chars, 0);

        // 5. Legacy Windows-1252 auto-detection & conversion to UTF-8
        let cp1252_file = dir.join("legacy.txt");
        // "café — €10\r\n" in Windows-1252: e9 = é, 97 = —, 80 = €
        std::fs::write(&cp1252_file, b"caf\xE9 \x97 \x8010\r\n").unwrap();
        let cap_cp = svc.open_document(&cp1252_file).unwrap();
        assert_eq!(cap_cp.encoding, FileEncoding::Windows1252);
        assert_eq!(cap_cp.line_ending, LineEnding::Crlf);
        assert_eq!(cap_cp.content, "café — €10\n");

        // Reopen same file explicitly as ISO 8859-1 without touching disk
        let cap_iso = svc
            .open_document_with_encoding(&cp1252_file, FileEncoding::Iso88591)
            .unwrap();
        assert_eq!(cap_iso.encoding, FileEncoding::Iso88591);
        assert_eq!(cap_iso.version, cap_cp.version);

        // 6. Binary file with NUL bytes is rejected by auto-detect
        let bin_file = dir.join("image.bin");
        std::fs::write(&bin_file, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").unwrap();
        assert!(svc.open_document(&bin_file).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
