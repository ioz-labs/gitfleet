pub mod command_log;
pub mod error;
pub mod events;
pub mod path_utils;
pub mod settings;
pub mod types;
pub mod undo;

pub use command_log::{CommandLogEntry, CommandLogStore};
pub use error::{AppError, AppResult};
pub use events::{AppEvent, EventBus};
pub use path_utils::{canonicalize_clean, strip_windows_verbatim_prefix, strip_windows_verbatim_str};
pub use settings::AppSettings;
pub use types::{CancelToken, DocumentId, RepoId, SessionId};
pub use undo::{UndoEntry, UndoStack};
