//! Where the API keys live: the OS credential store (Windows Credential Manager, macOS Keychain),
//! one entry per provider, so switching provider in the settings doesn't lose the other's key. A
//! key never goes into the database, never into the webview, and is never read back out to the UI:
//! the frontend can only save one, remove one, and ask whether there is one.

use std::{collections::HashMap, sync::Mutex};

use super::error::{Error, Result};

pub trait KeyStore: Send + Sync {
    fn get(&self, provider: &str) -> Result<Option<String>>;
    fn set(&self, provider: &str, key: &str) -> Result<()>;
    /// Not an error if there's nothing stored.
    fn clear(&self, provider: &str) -> Result<()>;
}

/// The OS credential store, under a service name derived from the app's identifier (so a
/// differently identified build never touches the real keys).
pub struct KeyringKeys {
    service: String,
}

impl KeyringKeys {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self, provider: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, provider).map_err(credential_error)
    }
}

fn credential_error(e: keyring::Error) -> Error {
    Error::Credentials(e.to_string())
}

impl KeyStore for KeyringKeys {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        match self.entry(provider)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(credential_error(e)),
        }
    }

    fn set(&self, provider: &str, key: &str) -> Result<()> {
        self.entry(provider)?.set_password(key).map_err(credential_error)
    }

    fn clear(&self, provider: &str) -> Result<()> {
        match self.entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(credential_error(e)),
        }
    }
}

/// Keeps keys in memory only. For tests.
#[derive(Default)]
pub struct MemoryKeys(Mutex<HashMap<String, String>>);

impl KeyStore for MemoryKeys {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(provider).cloned())
    }

    fn set(&self, provider: &str, key: &str) -> Result<()> {
        self.0.lock().unwrap().insert(provider.to_owned(), key.to_owned());
        Ok(())
    }

    fn clear(&self, provider: &str) -> Result<()> {
        self.0.lock().unwrap().remove(provider);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keys_are_per_provider_and_clearing_a_missing_one_is_fine() {
        let keys = MemoryKeys::default();
        assert_eq!(keys.get("anthropic").unwrap(), None);
        keys.set("anthropic", "sk-ant-1").unwrap();
        keys.set("openai", "sk-2").unwrap();
        assert_eq!(keys.get("anthropic").unwrap().as_deref(), Some("sk-ant-1"));
        keys.clear("anthropic").unwrap();
        keys.clear("anthropic").unwrap();
        assert_eq!(keys.get("anthropic").unwrap(), None);
        assert_eq!(keys.get("openai").unwrap().as_deref(), Some("sk-2"));
    }

    /// The real OS store, with a throwaway service name. Windows' limit on one credential is far
    /// above what a key needs, but a key is the one thing this stores, so prove it round-trips.
    #[cfg(windows)]
    #[test]
    fn the_os_store_round_trips_a_key() {
        let keys = KeyringKeys::new(format!("sommelier-test-{}.coach", std::process::id()));
        let key = format!("sk-ant-api03-{}", "x".repeat(100));
        assert_eq!(keys.get("anthropic").unwrap(), None);
        keys.set("anthropic", &key).unwrap();
        assert_eq!(keys.get("anthropic").unwrap().as_deref(), Some(key.as_str()));
        keys.clear("anthropic").unwrap();
        assert_eq!(keys.get("anthropic").unwrap(), None);
        keys.clear("anthropic").unwrap();
    }
}
