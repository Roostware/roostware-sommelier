//! Small user preferences, stored as text key/value pairs: how many problems the daily set has,
//! and whether the app's curated collections (Blind 75, …) are shown. The settings menu reads
//! and writes through here.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{now_ms, Error, Result};

const DAILY_COUNT_KEY: &str = "daily_problem_count";
const SHOW_CURATED_KEY: &str = "show_curated_collections";
pub const DEFAULT_DAILY_COUNT: usize = 3;
pub const MIN_DAILY_COUNT: usize = 1;
pub const MAX_DAILY_COUNT: usize = 10;

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM main.settings WHERE key = ?", [key], |r| r.get(0)).optional()?)
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO main.settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now_ms()],
    )?;
    Ok(())
}

/// How many problems the daily set has (3 until the user changes it). A stored value that is
/// somehow out of range or unreadable falls back to the default rather than breaking the dashboard.
pub fn daily_count(conn: &Connection) -> Result<usize> {
    let stored = get(conn, DAILY_COUNT_KEY)?.and_then(|v| v.parse::<usize>().ok());
    Ok(stored.filter(|n| (MIN_DAILY_COUNT..=MAX_DAILY_COUNT).contains(n)).unwrap_or(DEFAULT_DAILY_COUNT))
}

pub fn set_daily_count(conn: &Connection, count: usize) -> Result<()> {
    if !(MIN_DAILY_COUNT..=MAX_DAILY_COUNT).contains(&count) {
        return Err(Error::Invalid(format!("the daily count must be between {MIN_DAILY_COUNT} and {MAX_DAILY_COUNT}")));
    }
    set(conn, DAILY_COUNT_KEY, &count.to_string())
}

/// Whether the app's curated collections show up in the problem list's collection dropdown and
/// on its rows (no until the user turns it on). Anything stored but "true" counts as off.
pub fn show_curated_collections(conn: &Connection) -> Result<bool> {
    Ok(get(conn, SHOW_CURATED_KEY)?.as_deref() == Some("true"))
}

pub fn set_show_curated_collections(conn: &Connection, show: bool) -> Result<()> {
    set(conn, SHOW_CURATED_KEY, if show { "true" } else { "false" })
}

// ---- The AI coach ----
// Which provider and model it talks to, and whether it's there at all (not everyone wants it or
// can pay for it). The API key is deliberately not a setting: it lives in the system's credential
// store (see `coach::keys`), like the LeetCode login.

const COACH_ENABLED_KEY: &str = "ai_coach_enabled";
const COACH_PROVIDER_KEY: &str = "ai_provider";
const COACH_MODEL_KEY: &str = "ai_model";
const COACH_BASE_URL_KEY: &str = "ai_base_url";
const COACH_PRICE_IN_KEY: &str = "ai_price_input";
const COACH_PRICE_OUT_KEY: &str = "ai_price_output";
/// Every setting that is part of how the coach is set up (which provider, model and address; the
/// API key is not here, it's in the credential store). A reset that keeps the keys keeps these too:
/// a key is no use without the provider it belongs to.
pub const COACH_KEYS: [&str; 6] =
    [COACH_ENABLED_KEY, COACH_PROVIDER_KEY, COACH_MODEL_KEY, COACH_BASE_URL_KEY, COACH_PRICE_IN_KEY, COACH_PRICE_OUT_KEY];
/// The providers the settings menu offers: Anthropic, OpenAI, and any other server that speaks
/// OpenAI's chat API (OpenRouter, a local Ollama or LM Studio, …), which needs a base URL.
pub const COACH_PROVIDERS: [&str; 3] = ["anthropic", "openai", "other"];
pub const DEFAULT_COACH_PROVIDER: &str = "anthropic";
const MAX_MODEL_LEN: usize = 100;
const MAX_BASE_URL_LEN: usize = 300;
/// A sanity bound on a price (US dollars per million tokens): no model costs more than this.
const MAX_PRICE: f64 = 100_000.0;

/// The AI coach's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoachSettings {
    /// Whether the Coach shows in the Solve tab. On until the user turns it off.
    pub enabled: bool,
    /// One of [`COACH_PROVIDERS`].
    pub provider: String,
    /// The model's name as the provider calls it. Empty means the provider's default where there
    /// is one (Anthropic), otherwise the coach isn't ready.
    pub model: String,
    /// Where an OpenAI-compatible server lives, e.g. `http://localhost:11434/v1`. Only used by the
    /// "other" provider.
    pub base_url: String,
    /// What the user pays per million input and output tokens, in US dollars, if they've said, so
    /// the coach can show what a conversation cost.
    pub price_input: Option<f64>,
    pub price_output: Option<f64>,
}

fn stored_price(conn: &Connection, key: &str) -> Result<Option<f64>> {
    Ok(get(conn, key)?.and_then(|v| v.parse::<f64>().ok()).filter(|p| p.is_finite() && (0.0..=MAX_PRICE).contains(p)))
}

pub fn coach(conn: &Connection) -> Result<CoachSettings> {
    let provider = get(conn, COACH_PROVIDER_KEY)?.filter(|p| COACH_PROVIDERS.contains(&p.as_str()));
    Ok(CoachSettings {
        // Anything stored but "false" counts as on.
        enabled: get(conn, COACH_ENABLED_KEY)?.as_deref() != Some("false"),
        provider: provider.unwrap_or_else(|| DEFAULT_COACH_PROVIDER.to_owned()),
        model: get(conn, COACH_MODEL_KEY)?.unwrap_or_default(),
        base_url: get(conn, COACH_BASE_URL_KEY)?.unwrap_or_default(),
        price_input: stored_price(conn, COACH_PRICE_IN_KEY)?,
        price_output: stored_price(conn, COACH_PRICE_OUT_KEY)?,
    })
}

fn check_price(name: &str, price: Option<f64>) -> Result<()> {
    match price {
        Some(p) if !p.is_finite() || !(0.0..=MAX_PRICE).contains(&p) => {
            Err(Error::Invalid(format!("the {name} price must be between 0 and {MAX_PRICE} dollars per million tokens")))
        }
        _ => Ok(()),
    }
}

/// Saves the coach's settings (trimming the text ones). Rejects an unknown provider, an over-long
/// model name, a base URL that isn't an http(s) address, or a price out of range, saving nothing.
pub fn set_coach(conn: &Connection, new: &CoachSettings) -> Result<()> {
    if !COACH_PROVIDERS.contains(&new.provider.as_str()) {
        return Err(Error::Invalid(format!("{:?} is not one of the providers ({})", new.provider, COACH_PROVIDERS.join(", "))));
    }
    let model = new.model.trim();
    if model.chars().count() > MAX_MODEL_LEN {
        return Err(Error::Invalid(format!("the model name can be at most {MAX_MODEL_LEN} characters")));
    }
    let base_url = new.base_url.trim();
    if base_url.chars().count() > MAX_BASE_URL_LEN {
        return Err(Error::Invalid(format!("the base URL can be at most {MAX_BASE_URL_LEN} characters")));
    }
    if !base_url.is_empty() {
        let ok = reqwest::Url::parse(base_url).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
        if !ok {
            return Err(Error::Invalid("the base URL must be an http:// or https:// address".into()));
        }
    }
    check_price("input", new.price_input)?;
    check_price("output", new.price_output)?;

    set(conn, COACH_ENABLED_KEY, if new.enabled { "true" } else { "false" })?;
    set(conn, COACH_PROVIDER_KEY, &new.provider)?;
    set(conn, COACH_MODEL_KEY, model)?;
    set(conn, COACH_BASE_URL_KEY, base_url)?;
    set(conn, COACH_PRICE_IN_KEY, &new.price_input.map(|p| p.to_string()).unwrap_or_default())?;
    set(conn, COACH_PRICE_OUT_KEY, &new.price_output.map(|p| p.to_string()).unwrap_or_default())
}

/// Every setting, as the settings menu shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub daily_problem_count: usize,
    pub show_curated_collections: bool,
    pub coach: CoachSettings,
}

pub fn all(conn: &Connection) -> Result<Settings> {
    Ok(Settings {
        daily_problem_count: daily_count(conn)?,
        show_curated_collections: show_curated_collections(conn)?,
        coach: coach(conn)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support;
    use tempfile::TempDir;

    #[test]
    fn the_daily_count_defaults_to_three_and_can_be_changed() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        assert_eq!(db.with(|c| daily_count(c)).unwrap(), 3);
        db.with(|c| set_daily_count(c, 5)).unwrap();
        assert_eq!(db.with(|c| daily_count(c)).unwrap(), 5);
        db.with(|c| set_daily_count(c, 1)).unwrap();
        assert_eq!(db.with(|c| daily_count(c)).unwrap(), 1);
    }

    #[test]
    fn out_of_range_counts_are_refused_and_bad_stored_values_ignored() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        for bad in [0, 11, 500] {
            assert!(matches!(db.with(|c| set_daily_count(c, bad)), Err(Error::Invalid(_))), "{bad}");
        }
        assert_eq!(db.with(|c| daily_count(c)).unwrap(), 3, "nothing was stored");
        for junk in ["abc", "-2", "0", "99", ""] {
            db.with(|c| set(c, DAILY_COUNT_KEY, junk)).unwrap();
            assert_eq!(db.with(|c| daily_count(c)).unwrap(), 3, "stored {junk:?}");
        }
    }

    #[test]
    fn curated_collections_are_hidden_until_turned_on() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        assert!(!db.with(|c| show_curated_collections(c)).unwrap());
        db.with(|c| set_show_curated_collections(c, true)).unwrap();
        assert!(db.with(|c| show_curated_collections(c)).unwrap());
        let s = db.with(|c| all(c)).unwrap();
        assert_eq!((s.daily_problem_count, s.show_curated_collections), (3, true));
        db.with(|c| set_show_curated_collections(c, false)).unwrap();
        assert!(!db.with(|c| show_curated_collections(c)).unwrap());
        db.with(|c| set(c, SHOW_CURATED_KEY, "junk")).unwrap();
        assert!(!db.with(|c| show_curated_collections(c)).unwrap(), "unreadable means the default");
    }

    fn coach_settings(provider: &str, model: &str) -> CoachSettings {
        CoachSettings {
            enabled: true,
            provider: provider.into(),
            model: model.into(),
            base_url: String::new(),
            price_input: None,
            price_output: None,
        }
    }

    #[test]
    fn the_coach_is_on_by_default_and_its_settings_round_trip() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        let defaults = db.with(|c| coach(c)).unwrap();
        assert_eq!(defaults, coach_settings("anthropic", ""));

        let new = CoachSettings {
            enabled: false,
            base_url: "  http://localhost:11434/v1  ".into(),
            price_input: Some(2.5),
            price_output: Some(10.0),
            ..coach_settings("other", "  llama3  ")
        };
        db.with(|c| set_coach(c, &new)).unwrap();
        let saved = db.with(|c| coach(c)).unwrap();
        assert_eq!(
            saved,
            CoachSettings { model: "llama3".into(), base_url: "http://localhost:11434/v1".into(), ..new }
        );
        assert_eq!(db.with(|c| all(c)).unwrap().coach, saved);

        // Clearing a price puts it back to "not set".
        db.with(|c| set_coach(c, &CoachSettings { price_input: None, ..saved.clone() })).unwrap();
        assert_eq!(db.with(|c| coach(c)).unwrap().price_input, None);
    }

    #[test]
    fn bad_coach_settings_are_refused_and_bad_stored_ones_ignored() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        let refused = |new: CoachSettings| matches!(db.with(|c| set_coach(c, &new)), Err(Error::Invalid(_)));
        assert!(refused(coach_settings("skynet", "")));
        assert!(refused(coach_settings("other", &"x".repeat(101))));
        assert!(refused(CoachSettings { base_url: "ftp://example.com".into(), ..coach_settings("other", "m") }));
        assert!(refused(CoachSettings { base_url: "not a url".into(), ..coach_settings("other", "m") }));
        assert!(refused(CoachSettings { price_input: Some(-1.0), ..coach_settings("openai", "m") }));
        assert!(refused(CoachSettings { price_output: Some(f64::NAN), ..coach_settings("openai", "m") }));
        assert_eq!(db.with(|c| coach(c)).unwrap(), coach_settings("anthropic", ""), "nothing was stored");

        db.with(|c| set(c, COACH_PROVIDER_KEY, "skynet")).unwrap();
        db.with(|c| set(c, COACH_ENABLED_KEY, "junk")).unwrap();
        db.with(|c| set(c, COACH_PRICE_IN_KEY, "-5")).unwrap();
        let s = db.with(|c| coach(c)).unwrap();
        assert_eq!((s.provider.as_str(), s.enabled, s.price_input), ("anthropic", true, None), "unreadable means the defaults");
    }

    #[test]
    fn settings_are_overwritten_in_place() {
        let dir = TempDir::new().unwrap();
        let db = test_support::open_with_fixture(&dir);
        assert_eq!(db.with(|c| get(c, "k")).unwrap(), None);
        db.with(|c| set(c, "k", "1")).unwrap();
        db.with(|c| set(c, "k", "2")).unwrap();
        assert_eq!(db.with(|c| get(c, "k")).unwrap().as_deref(), Some("2"));
    }
}
