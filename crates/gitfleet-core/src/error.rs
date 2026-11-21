use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Repository not found: {0}")]
    RepoNotFound(String),

    #[error("Workspace locked by another process: {0}")]
    WorkspaceLocked(String),

    #[error("Document conflict: expected version {expected}, found {actual}")]
    DocumentConflict { expected: String, actual: String },

    #[error("Git operation failed: {0}")]
    Git(String),

    #[error("Security/Secrets error: {0}")]
    Security(String),

    #[error("Terminal error: {0}")]
    Terminal(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Not yet implemented: {0}")]
    NotImplemented(&'static str),
}

pub type AppResult<T> = Result<T, AppError>;
