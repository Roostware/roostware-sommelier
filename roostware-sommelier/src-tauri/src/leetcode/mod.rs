//! Judging code on LeetCode itself (Run and Submit) through its unofficial web API, and the
//! login that needs. See `client` for the API, `auth` for signing in and keeping the login,
//! and `result` for turning LeetCode's responses into our types.
//!
//! This is unofficial and against LeetCode's terms of service for automated access; it is meant
//! for one person's own account, one request at a time, driven by a click.

pub mod auth;
pub mod client;
mod error;
pub mod result;

use tauri::AppHandle;

pub use error::{Error, Result};

use auth::{AuthStatus, Credentials, CredentialStore, KeyringStore};
use client::Client;

/// Managed Tauri state: the LeetCode client and where the login is kept.
pub struct LeetCode {
    pub store: Box<dyn CredentialStore>,
    pub client: Client,
}

impl LeetCode {
    pub fn new(store: Box<dyn CredentialStore>, client: Client) -> Self {
        Self { store, client }
    }

    /// The real thing: leetcode.com, with the login in the OS credential store under a name
    /// derived from the app's identifier (so a differently-identified build, like a test
    /// instance, never touches the real login).
    pub fn production(app: &AppHandle) -> Self {
        let service = format!("{}.leetcode", app.config().identifier);
        Self::new(Box::new(KeyringStore::new(service)), Client::new(base_url()))
    }

    /// The stored login, or `NotSignedIn`.
    pub fn credentials(&self) -> Result<Credentials> {
        self.store.load()?.map(|s| s.credentials).ok_or(Error::NotSignedIn)
    }

    pub fn status(&self) -> Result<AuthStatus> {
        Ok(AuthStatus::of(self.store.load()?.as_ref()))
    }
}

/// leetcode.com. Debug builds can point at another server with `LEETCODE_BASE_URL`, which lets
/// the sign-in flow be tested against a local stand-in; release builds ignore it.
fn base_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("LEETCODE_BASE_URL") {
        return url;
    }
    client::DEFAULT_BASE.to_owned()
}
