//! The recommendation engine: what to review and what to learn next.
//!
//! Pure logic with no database or UI in it (the `db::recommendations` module feeds it and
//! stores its output):
//!   * [`fsrs`]: the memory model that says when a solved problem is worth reviewing;
//!   * [`patterns`]: the 17 patterns, and which problems belong to which;
//!   * [`engine`]: pattern strength and the daily set;
//!   * [`elo`]: the overall skill rating shown on the dashboard (it doesn't feed back into
//!     recommendations).

pub mod elo;
pub mod engine;
pub mod fsrs;
pub mod patterns;
