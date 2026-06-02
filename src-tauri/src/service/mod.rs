pub mod cover;
pub mod files;
pub mod folder_import;
pub mod folders;
pub mod imports;
pub mod migrate_layout;
pub mod objects;
pub mod openscad;
pub mod orca;
pub mod paths;
pub mod settings;
pub mod usb;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("{0}")]
    Other(String),
}

impl ServiceError {
    /// Stable machine-readable discriminant. The frontend can switch on this
    /// without parsing the human-readable `message`.
    pub fn code(&self) -> &'static str {
        match self {
            ServiceError::Io(_) => "io",
            ServiceError::Db(_) => "db",
            ServiceError::Json(_) => "json",
            ServiceError::NotFound(_) => "not_found",
            ServiceError::Conflict(_) => "conflict",
            ServiceError::Validation(_) => "validation",
            ServiceError::Other(_) => "other",
        }
    }
}

pub type ServiceResult<T> = Result<T, ServiceError>;

/// Wire-format error returned to the frontend across the IPC boundary. Every
/// `#[tauri::command]` returns `Result<T, AppError>` so the renderer always
/// receives the same `{ code, message }` shape — this lets UI code switch on
/// `code` for retry/rollback decisions instead of parsing the message string.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl From<ServiceError> for AppError {
    fn from(e: ServiceError) -> Self {
        AppError::new(e.code(), e.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::new("db", format!("db: {e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new("io", format!("io: {e}"))
    }
}
