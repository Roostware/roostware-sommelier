use serde::{ser::SerializeStruct, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to LeetCode. Unlike the storage errors, the frontend
/// needs to tell these apart (a missing or expired session means "show the sign-in prompt"), so
/// they cross the IPC boundary as `{ kind, message }`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Sign in to LeetCode first.")]
    NotSignedIn,

    #[error("Your LeetCode session has expired. Sign in again.")]
    SessionExpired,

    /// Cloudflare (LeetCode's bot protection) answered instead of LeetCode.
    #[error("LeetCode's bot protection blocked the request. Try again in a moment.")]
    Blocked,

    #[error("LeetCode is limiting how fast you can submit. {0}")]
    RateLimited(String),

    #[error("Sign-in was cancelled.")]
    Cancelled,

    #[error("Couldn't reach LeetCode: {0}")]
    Network(String),

    #[error("LeetCode returned an unexpected response: {0}")]
    Unexpected(String),

    #[error("Couldn't use the OS credential store: {0}")]
    Credentials(String),

    #[error("{0}")]
    Other(String),

    #[error(transparent)]
    Db(#[from] crate::db::Error),
}

impl Error {
    /// A stable machine-readable name for the frontend to branch on.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::NotSignedIn => "notSignedIn",
            Error::SessionExpired => "sessionExpired",
            Error::Blocked => "blocked",
            Error::RateLimited(_) => "rateLimited",
            Error::Cancelled => "cancelled",
            Error::Network(_) => "network",
            Error::Unexpected(_) => "unexpected",
            Error::Credentials(_) => "credentials",
            Error::Other(_) => "other",
            Error::Db(_) => "storage",
        }
    }
}

impl serde::Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("LeetCodeError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}
