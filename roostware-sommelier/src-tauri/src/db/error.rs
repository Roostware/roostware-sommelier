use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0} not found")]
    NotFound(String),

    #[error("could not resolve an app path: {0}")]
    Path(String),

    #[error(
        "user.db is at schema version {found}, newer than this build supports ({supported}); \
         update the app (the file has not been modified)"
    )]
    UserDbTooNew { found: i64, supported: i64 },

    #[error("catalog.db not found at {0}; build it with `npm run catalog`")]
    CatalogMissing(PathBuf),

    #[error(
        "catalog.db has schema version {found:?}, expected {expected}; \
         rebuild it with `npm run catalog`"
    )]
    CatalogVersion { found: Option<String>, expected: i64 },
}

// Tauri commands need their error type to be Serialize; send the message.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
