//! Signing in to LeetCode from inside the app, and keeping the result.
//!
//! LeetCode has no sign-in API, so the app opens leetcode.com's own login page in a separate
//! window, lets the user sign in there (password, Google, GitHub, whatever LeetCode offers),
//! and reads the two cookies that make requests count as that user: `LEETCODE_SESSION` and
//! `csrftoken`. The window is private (nothing survives it) and is closed as soon as the
//! cookies are in hand; the app never sees the user's password.
//!
//! The cookies are stored in the OS credential store (Windows Credential Manager), not in
//! user.db, so a copy of the database doesn't carry a login with it.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

use super::{error::Error, error::Result, LeetCode};

const SIGN_IN_WINDOW: &str = "leetcode-signin";
const POLL: Duration = Duration::from_millis(750);
const GIVE_UP_AFTER: Duration = Duration::from_secs(15 * 60);

/// The two cookies that authenticate requests.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub session: String,
    pub csrf: String,
}

// Never print the secrets, even in a panic message or a debug log.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials(<redacted>)")
    }
}

/// What's kept between launches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub credentials: Credentials,
    pub username: String,
}

/// Where the login lives. The real one is the OS credential store; tests use memory.
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<Stored>>;
    fn save(&self, stored: &Stored) -> Result<()>;
    /// Not an error if there's nothing stored.
    fn clear(&self) -> Result<()>;
}

/// What the frontend needs to know about the login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub signed_in: bool,
    pub username: Option<String>,
}

impl AuthStatus {
    pub fn signed_out() -> Self {
        Self { signed_in: false, username: None }
    }

    pub fn of(stored: Option<&Stored>) -> Self {
        match stored {
            Some(s) => Self { signed_in: true, username: Some(s.username.clone()) },
            None => Self::signed_out(),
        }
    }
}

// ---- Credential stores ----------------------------------------------------------------------

/// The OS credential store. One entry per value, under `service`, so each stays well inside the
/// per-credential size limit Windows enforces.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, name).map_err(credential_error)
    }

    fn read(&self, name: &str) -> Result<Option<String>> {
        match self.entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(credential_error(e)),
        }
    }
}

fn credential_error(e: keyring::Error) -> Error {
    Error::Credentials(e.to_string())
}

impl CredentialStore for KeyringStore {
    fn load(&self) -> Result<Option<Stored>> {
        let (Some(session), Some(csrf)) = (self.read("session")?, self.read("csrf")?) else {
            return Ok(None);
        };
        let username = self.read("username")?.unwrap_or_default();
        Ok(Some(Stored { credentials: Credentials { session, csrf }, username }))
    }

    fn save(&self, stored: &Stored) -> Result<()> {
        self.entry("session")?.set_password(&stored.credentials.session).map_err(credential_error)?;
        self.entry("csrf")?.set_password(&stored.credentials.csrf).map_err(credential_error)?;
        self.entry("username")?.set_password(&stored.username).map_err(credential_error)
    }

    fn clear(&self) -> Result<()> {
        for name in ["session", "csrf", "username"] {
            match self.entry(name)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(credential_error(e)),
            }
        }
        Ok(())
    }
}

/// Keeps the login in memory only. For tests.
#[derive(Default)]
pub struct MemoryStore(Mutex<Option<Stored>>);

impl MemoryStore {
    pub fn with(stored: Stored) -> Self {
        Self(Mutex::new(Some(stored)))
    }
}

impl CredentialStore for MemoryStore {
    fn load(&self) -> Result<Option<Stored>> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(&self, stored: &Stored) -> Result<()> {
        *self.0.lock().unwrap() = Some(stored.clone());
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

// ---- Signing in -----------------------------------------------------------------------------

/// Picks the two authenticating cookies out of a cookie list, if both are there and non-empty.
pub fn pick_credentials(cookies: &[(String, String)]) -> Option<Credentials> {
    let find = |name: &str| cookies.iter().find(|(n, v)| n == name && !v.is_empty()).map(|(_, v)| v.clone());
    Some(Credentials { session: find("LEETCODE_SESSION")?, csrf: find("csrftoken")? })
}

// Kept out of `sign_in` so the (non-Send) window builder is gone before any `.await`.
fn open_window<R: Runtime>(app: &AppHandle<R>, login: Url) -> Result<tauri::WebviewWindow<R>> {
    let mut builder = WebviewWindowBuilder::new(app, SIGN_IN_WINDOW, WebviewUrl::External(login))
        .title("Sign in to LeetCode")
        .inner_size(520.0, 760.0)
        .center()
        // Nothing the page stores outlives the window.
        .incognito(true);
    if let Some(main) = app.get_webview_window("main") {
        builder = builder.parent(&main).map_err(|e| Error::Other(e.to_string()))?;
    }
    builder.build().map_err(|e| Error::Other(format!("couldn't open the sign-in window: {e}")))
}

/// Opens LeetCode's login page in its own window and waits for the user to sign in (or to close
/// the window, which cancels). On success the login is stored and the window is closed.
pub async fn sign_in<R: Runtime>(app: &AppHandle<R>, lc: &LeetCode) -> Result<AuthStatus> {
    if let Some(open) = app.get_webview_window(SIGN_IN_WINDOW) {
        let _ = open.set_focus();
        return Err(Error::Other("The sign-in window is already open.".into()));
    }

    let site: Url = lc.client.base().parse().map_err(|e| Error::Other(format!("bad LeetCode address: {e}")))?;
    let login = site.join("accounts/login/").map_err(|e| Error::Other(e.to_string()))?;

    let window = open_window(app, login)?;

    let started = Instant::now();
    // A session already found not to be signed in: don't ask LeetCode about it again.
    let mut rejected: Option<String> = None;
    loop {
        tokio::time::sleep(POLL).await;

        if app.get_webview_window(SIGN_IN_WINDOW).is_none() {
            return Err(Error::Cancelled);
        }
        if started.elapsed() > GIVE_UP_AFTER {
            let _ = window.destroy();
            return Err(Error::Other("Timed out waiting for you to sign in.".into()));
        }

        let Ok(cookies) = window.cookies_for_url(site.clone()) else { continue };
        let pairs: Vec<(String, String)> =
            cookies.iter().map(|c| (c.name().to_owned(), c.value().to_owned())).collect();
        let Some(credentials) = pick_credentials(&pairs) else { continue };
        if rejected.as_deref() == Some(credentials.session.as_str()) {
            continue;
        }

        match lc.client.user_status(&credentials).await {
            Ok(Some(username)) => {
                let stored = Stored { credentials, username };
                let saved = lc.store.save(&stored);
                let _ = window.destroy();
                saved?;
                return Ok(AuthStatus::of(Some(&stored)));
            }
            Ok(None) => rejected = Some(credentials.session),
            Err(_) => {} // a hiccup: look again on the next tick
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookies(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn needs_both_cookies() {
        let both = cookies(&[("other", "x"), ("csrftoken", "c"), ("LEETCODE_SESSION", "s")]);
        assert_eq!(pick_credentials(&both), Some(Credentials { session: "s".into(), csrf: "c".into() }));
        assert_eq!(pick_credentials(&cookies(&[("csrftoken", "c")])), None, "not signed in yet");
        assert_eq!(pick_credentials(&cookies(&[("LEETCODE_SESSION", "s")])), None);
        assert_eq!(pick_credentials(&cookies(&[("LEETCODE_SESSION", ""), ("csrftoken", "c")])), None);
        assert_eq!(pick_credentials(&[]), None);
    }

    #[test]
    fn credentials_never_print() {
        let c = Credentials { session: "top-secret".into(), csrf: "also-secret".into() };
        let shown = format!("{c:?}");
        assert!(!shown.contains("secret"), "{shown}");
    }

    #[test]
    fn memory_store_round_trips_and_clears() {
        let store = MemoryStore::default();
        assert_eq!(store.load().unwrap(), None);
        let stored = Stored { credentials: Credentials { session: "s".into(), csrf: "c".into() }, username: "ada".into() };
        store.save(&stored).unwrap();
        assert_eq!(store.load().unwrap(), Some(stored.clone()));
        assert_eq!(AuthStatus::of(store.load().unwrap().as_ref()), AuthStatus { signed_in: true, username: Some("ada".into()) });
        store.clear().unwrap();
        store.clear().unwrap(); // clearing twice is fine
        assert_eq!(AuthStatus::of(store.load().unwrap().as_ref()), AuthStatus::signed_out());
    }

    // Talks to the real Windows Credential Manager, under a throwaway service name.
    #[test]
    #[cfg(windows)]
    fn keyring_store_round_trips_a_realistically_long_session() {
        let store = KeyringStore::new(format!("roostware-sommelier-test-{}", crate::db::new_id()));
        assert_eq!(store.load().unwrap(), None);
        // A LeetCode session cookie is a JWT around 800 characters; leave generous headroom.
        let stored = Stored {
            credentials: Credentials { session: "j".repeat(1100), csrf: "c".repeat(32) },
            username: "Ada Lovelace".into(),
        };
        store.save(&stored).unwrap();
        assert_eq!(store.load().unwrap(), Some(stored));
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }
}
