use gitfleet_core::{AppError, AppResult, DocumentId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenDocumentCapability {
    pub document_id: DocumentId,
    pub version: String,
    pub line_ending: LineEnding,
    pub content: String,
}

struct TrackedDocument {
    path: PathBuf,
    version: String,
    line_ending: LineEnding,
}

/// Capability-based document editor: UI holds an opaque `DocumentId`, never writes by raw path.
#[derive(Default)]
pub struct DocsService {
    open_docs: RwLock<HashMap<DocumentId, TrackedDocument>>,
}

impl DocsService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_document(&self, path: &Path) -> AppResult<OpenDocumentCapability> {
        let raw = std::fs::read_to_string(path)?;
        let line_ending = if raw.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        let normalized = raw.replace("\r\n", "\n");
        let version = format!("len:{}", raw.len());
        let document_id = DocumentId::new();

        if let Ok(mut guard) = self.open_docs.write() {
            guard.insert(
                document_id.clone(),
                TrackedDocument {
                    path: path.to_path_buf(),
                    version: version.clone(),
                    line_ending,
                },
            );
        }

        Ok(OpenDocumentCapability {
            document_id,
            version,
            line_ending,
            content: normalized,
        })
    }

    pub fn save_document(
        &self,
        document_id: &DocumentId,
        content: &str,
        expected_version: &str,
    ) -> AppResult<String> {
        let mut guard = self
            .open_docs
            .write()
            .map_err(|_| AppError::Io(std::io::Error::other("lock poisoned")))?;
        let tracked = guard
            .get_mut(document_id)
            .ok_or_else(|| AppError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "document capability not found")))?;

        let disk_raw = std::fs::read_to_string(&tracked.path)?;
        let actual_version = format!("len:{}", disk_raw.len());
        if actual_version != expected_version {
            return Err(AppError::DocumentConflict {
                expected: expected_version.to_string(),
                actual: actual_version,
            });
        }

        let encoded = match tracked.line_ending {
            LineEnding::Crlf => content.replace('\n', "\r\n"),
            LineEnding::Lf => content.to_string(),
        };
        std::fs::write(&tracked.path, &encoded)?;
        let new_version = format!("len:{}", encoded.len());
        tracked.version = new_version.clone();
        Ok(new_version)
    }

    pub fn save_document_as(&self, dest: &Path, content: &str, line_ending: LineEnding) -> AppResult<()> {
        let encoded = match line_ending {
            LineEnding::Crlf => content.replace('\n', "\r\n"),
            LineEnding::Lf => content.to_string(),
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, encoded)?;
        Ok(())
    }
}
