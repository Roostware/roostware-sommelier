use serde::{ser::SerializeStruct, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong asking a model for something. Like the LeetCode errors, these
/// reach the frontend as `{ kind, message }` so it can treat "not set up", "stopped by the user"
/// and "the provider said no" differently. None of the messages can contain the API key.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A provider isn't set up yet (no key, no model, no address). The message says what's missing.
    #[error("{0}")]
    NotReady(String),

    #[error("Couldn't use the OS credential store: {0}")]
    Credentials(String),

    #[error("Couldn't reach the provider: {0}")]
    Network(String),

    /// The provider turned the key down.
    #[error("The provider didn't accept your API key ({0}). Check it in Settings.")]
    Auth(String),

    #[error("The provider is limiting how fast you can send requests. {0}")]
    RateLimited(String),

    /// Any other error the provider reported.
    #[error("{0}")]
    Api(String),

    /// The provider's safety systems declined to answer.
    #[error("The provider declined to answer this one. {0}")]
    Refused(String),

    /// The model ran out of room before finishing, so what arrived can't be used.
    #[error("The reply was cut off before it was finished. Try again.")]
    Truncated,

    /// A structured reply (an analysis, a solution) that didn't have the expected shape.
    #[error("The reply wasn't in the format the app expected. Try again.")]
    Unreadable,

    /// The user pressed Stop.
    #[error("Stopped.")]
    Cancelled,

    #[error("{0}")]
    Invalid(String),

    #[error(transparent)]
    Db(#[from] crate::db::Error),
}

impl Error {
    /// A stable machine-readable name for the frontend to branch on.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::NotReady(_) => "notReady",
            Error::Credentials(_) => "credentials",
            Error::Network(_) => "network",
            Error::Auth(_) => "auth",
            Error::RateLimited(_) => "rateLimited",
            Error::Api(_) => "api",
            Error::Refused(_) => "refused",
            Error::Truncated => "truncated",
            Error::Unreadable => "unreadable",
            Error::Cancelled => "cancelled",
            Error::Invalid(_) => "invalid",
            Error::Db(_) => "storage",
        }
    }
}

impl serde::Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("CoachError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}
