//! Tauri commands for the AI coach. Keep the TypeScript mirror in `src/lib/db.ts` (settings and
//! saved conversations) and `src/lib/coachProvider.ts` (asking) in sync with the types here.

use serde::Serialize;
use serde_json::Value;
use tauri::{ipc::Channel, State};

use super::{
    ask, prompt, provider::Provider, resolve, status, test_connection, Coach, ConnectionReport, Error,
    ReplyResult, Result, Status,
};
use crate::db::{
    self,
    coach::{self as threads, Loaded, MessageRecord},
    settings, Db,
};

/// The longest an API key can be. Real ones are around 100 characters.
const MAX_KEY_LEN: usize = 500;
/// What a request may carry, so a runaway editor can't send megabytes.
const MAX_CODE_BYTES: usize = 400 * 1024;
const MAX_MESSAGE_BYTES: usize = 40 * 1024;
const MAX_HISTORY: usize = 60;

// ---- Setting up -----------------------------------------------------------------------------

/// Whether the coach can answer right now, and if not what's missing. Never contains a key.
#[tauri::command]
pub async fn coach_status(db: State<'_, Db>, coach: State<'_, Coach>) -> Result<Status> {
    let settings = db.with(|c| settings::coach(c))?;
    status(&settings, coach.keys())
}

fn provider_name(name: &str) -> Result<&'static str> {
    Provider::parse(name).map(Provider::name).ok_or_else(|| Error::Invalid(format!("{name:?} is not one of the providers")))
}

/// Saves a provider's API key in the OS credential store. It can't be read back out.
#[tauri::command]
pub async fn set_coach_key(coach: State<'_, Coach>, provider: String, key: String) -> Result<()> {
    let provider = provider_name(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Invalid("The key is empty.".into()));
    }
    if key.chars().count() > MAX_KEY_LEN || key.chars().any(char::is_control) {
        return Err(Error::Invalid("That doesn't look like an API key.".into()));
    }
    coach.keys().set(provider, key)
}

/// Removes a provider's saved API key. Fine if there wasn't one.
#[tauri::command]
pub async fn clear_coach_key(coach: State<'_, Coach>, provider: String) -> Result<()> {
    coach.keys().clear(provider_name(&provider)?)
}

/// Sends the smallest possible request with the saved settings and key, to prove they work.
/// Resolves to the model that answered and what it cost; rejects with what went wrong.
#[tauri::command]
pub async fn test_coach_connection(db: State<'_, Db>, coach: State<'_, Coach>) -> Result<ConnectionReport> {
    let settings = db.with(|c| settings::coach(c))?;
    let cfg = resolve(&settings, coach.keys())?;
    let tracked = coach.track("connection-test");
    test_connection(coach.http(), &cfg, &tracked.notify).await
}

// ---- Asking ---------------------------------------------------------------------------------

/// A piece of a reply, as it arrives.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StreamEvent {
    Text { text: String },
}

fn check(request: &prompt::ReplyRequest) -> Result<()> {
    let too_big = |what: &str| Error::Invalid(format!("The {what} is too large to send."));
    if request.request_id.is_empty() || request.request_id.len() > 100 {
        return Err(Error::Invalid("A request needs an id.".into()));
    }
    if request.code.as_ref().is_some_and(|c| c.len() > MAX_CODE_BYTES) {
        return Err(too_big("code"));
    }
    if request.message.as_ref().is_some_and(|m| m.len() > MAX_MESSAGE_BYTES) {
        return Err(too_big("message"));
    }
    if request.history.len() > MAX_HISTORY {
        return Err(too_big("conversation"));
    }
    Ok(())
}

/// Asks the coach something. Text arrives on `on_event` as it's written; the result is the whole
/// reply (text, an analysis, or a solution) and what it cost. Rejects with `cancelled` if
/// `coach_cancel` is called with the same request id.
#[tauri::command]
pub async fn coach_reply(
    db: State<'_, Db>,
    coach: State<'_, Coach>,
    request: prompt::ReplyRequest,
    on_event: Channel<StreamEvent>,
) -> Result<ReplyResult> {
    check(&request)?;
    // Everything read from the database is read here, so the lock isn't held while waiting on the network.
    let settings = db.with(|c| settings::coach(c))?;
    let problem = db.with(|c| Ok(prompt::problem_text(c, request.problem_id)))??;
    let cfg = resolve(&settings, coach.keys())?;
    let built = prompt::build(&request, &problem);

    let tracked = coach.track(&request.request_id);
    ask(coach.http(), &cfg, &built, &tracked.notify, |piece| {
        let _ = on_event.send(StreamEvent::Text { text: piece.to_owned() });
    })
    .await
}

/// Stops a reply in flight (the Stop button). Fine if it already finished.
#[tauri::command]
pub async fn coach_cancel(coach: State<'_, Coach>, request_id: String) -> Result<()> {
    coach.cancel(&request_id);
    Ok(())
}

// ---- Saved conversations --------------------------------------------------------------------

/// The problem's live conversation (if any) and the tokens every conversation about it has used.
#[tauri::command]
pub async fn load_coach_thread(db: State<'_, Db>, problem_id: i64) -> db::Result<Loaded> {
    db.with(|conn| threads::load(conn, problem_id))
}

/// Saves a message in the problem's live conversation, replacing one at the same `seq`.
#[tauri::command]
pub async fn save_coach_message(db: State<'_, Db>, problem_id: i64, message: MessageRecord) -> db::Result<()> {
    db.with(|conn| threads::save_message(conn, problem_id, &message))
}

/// Saves the frontend's bookkeeping for the problem's live conversation.
#[tauri::command]
pub async fn save_coach_state(db: State<'_, Db>, problem_id: i64, state: Value) -> db::Result<()> {
    db.with(|conn| threads::save_state(conn, problem_id, &state))
}

/// Ends the problem's live conversation; the next message starts a new one.
#[tauri::command]
pub async fn new_coach_thread(db: State<'_, Db>, problem_id: i64) -> db::Result<()> {
    db.with(|conn| threads::new_thread(conn, problem_id))
}
