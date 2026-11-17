// Typed access to the app's local storage (SQLite, owned by the Rust side).
//
// Components should import from here and never call `invoke()` directly. The
// types mirror the serde types in src-tauri/src/db/problems.rs (camelCase on
// the wire); keep the two in sync when either changes.

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./platform";

// ---- Types ------------------------------------------------------------------

export type Difficulty = "Easy" | "Medium" | "Hard";

/**
 * Derived, in priority order: "solved" = has an Accepted submission; "attempted" = has any
 * submission (even if there's also unsubmitted code); "inProgress" = has unsubmitted code that
 * differs from the starter and no submissions; "untouched" = none of these.
 */
export type Progress = "solved" | "attempted" | "inProgress" | "untouched";

export interface ProblemSummary {
  /** LeetCode's internal question id. This is what user data and submissions reference. */
  id: number;
  /** The number shown on leetcode.com. Show this to the user, not `id`. */
  frontendId: number;
  title: string;
  titleSlug: string;
  difficulty: Difficulty;
  isPaidOnly: boolean;
  /** 0..1, a snapshot from when the dataset was built. */
  acceptanceRate: number;
  progress: Progress;
  /** Number of Accepted submissions. */
  solveCount: number;
  tags: string[];
  /** Ids of the user's (non-deleted) collections that contain this problem. */
  collectionIds: string[];
}

export interface Snippet {
  /** LeetCode langSlug: python3, cpp, javascript, … Not every problem has every language. */
  lang: string;
  code: string;
}

/**
 * The user's unsubmitted code for a problem in one language. Only exists while it differs from
 * where they're starting from: their latest submission in that language, or the starter code.
 */
export interface Draft {
  lang: string;
  code: string;
  /** Epoch milliseconds, UTC. */
  updatedAt: number;
}

/** The code of the user's latest submission in one language. */
export interface LastSubmitted {
  lang: string;
  code: string;
  /** Epoch milliseconds, UTC. */
  submittedAt: number;
}

/** One of the user's submissions, as LeetCode judged it. Mirrors `Submission` in db/submissions.rs. */
export interface Submission {
  id: string;
  problemId: number;
  lang: string;
  code: string;
  leetcodeSubmissionId: string | null;
  /** LeetCode's verdict: "Accepted", "Wrong Answer", "Time Limit Exceeded", "Compile Error", … */
  status: string;
  statusCode: number | null;
  /** Null when LeetCode doesn't report one (e.g. any failure). */
  runtimeMs: number | null;
  runtimePercentile: number | null;
  memoryKb: number | null;
  memoryPercentile: number | null;
  /** LeetCode stops at the first failing test, so on a failure this is not "how many passed". */
  totalCorrect: number | null;
  totalTestcases: number | null;
  /** On a wrong answer: the input that failed (one line per argument), what was expected, and what the code gave. */
  lastTestcase: string | null;
  expectedOutput: string | null;
  codeOutput: string | null;
  /** Compile or runtime error text. */
  errorMessage: string | null;
  /** Epoch milliseconds, UTC. */
  submittedAt: number;
  /**
   * For an Accepted submission: the spaced-repetition review it logged, and its grade (1 Again,
   * 2 Hard, 3 Good, 4 Easy). Both null when it wasn't logged (a resubmit within the same sitting).
   */
  reviewId: string | null;
  reviewGrade: number | null;
}

/** The user's own state for a problem. All defaults until they set something. */
export interface ProblemState {
  status: string | null;
  starred: boolean;
  notes: string;
  lastOpenedAt: number | null;
}

export interface ProblemDetail extends ProblemSummary {
  /** Third-party HTML: sanitize before rendering with {@html}. */
  contentHtml: string;
  /** Raw and newline-separated; one case can span several lines (one per argument). */
  exampleTestCases: string;
  likes: number;
  dislikes: number;
  totalAccepted: number;
  totalSubmissions: number;
  createdAtApprox: string | null;
  snippets: Snippet[];
  /** Unsubmitted code, one per language, most recently edited first. */
  drafts: Draft[];
  /** Latest submitted code per language: what to show for a language that has no draft. */
  lastSubmitted: LastSubmitted[];
  state: ProblemState;
}

export interface ProblemPage {
  items: ProblemSummary[];
  /** Number of matches ignoring limit/offset, for paging. */
  total: number;
}

/** A problem the user has opened before. */
export interface RecentProblem extends ProblemSummary {
  /** Epoch milliseconds, UTC. */
  lastOpenedAt: number;
}

export interface TagCount {
  name: string;
  count: number;
}

/** One of the user's own collections (custom groups of problems, not LeetCode's topic tags). */
export interface CollectionSummary {
  id: string;
  name: string;
  description: string;
  /** Problems currently in the collection. */
  problemCount: number;
  /** One of the app's own lists (Blind 75, NeetCode 150, …): fixed, so problems can't be added or removed. */
  builtin: boolean;
}

/** One of the problems recommended for today. Mirrors `DailyItem` in db/recommendations.rs. */
export interface DailyItem {
  /** LeetCode's internal id: what `openProblem` takes. */
  problemId: number;
  /** The number shown on leetcode.com. */
  frontendId: number;
  title: string;
  titleSlug: string;
  difficulty: Difficulty;
  /** "review" is a solved problem due for another go; "new" hasn't been solved. */
  kind: "new" | "review";
  /** Why it was picked, in a sentence. */
  reason: string;
  /** The pattern it mainly serves. */
  pattern: string | null;
  /** "Blind 75" or "NeetCode 150" when it is on one of those lists. */
  curated: string | null;
  /** Epoch milliseconds: when the user last solved it. */
  lastSolvedAt: number | null;
  /** Solved since today's set was made. */
  done: boolean;
}

export interface DailySet {
  /** The local date, YYYY-MM-DD. */
  day: string;
  count: number;
  items: DailyItem[];
}

/** How strong the user is in one of the 17 patterns. */
export interface PatternStrength {
  id: string;
  name: string;
  /** A label short enough for a chart axis. */
  short: string;
  /** Learning order: 0 is the foundations. */
  tier: number;
  /** 0 to 100. */
  value: number;
  solved: number;
  /** Free curated problems in the pattern: what a full score is measured against. */
  curatedTotal: number;
  /** Whether the patterns it builds on have been partly solved yet. Once true it stays true, even as memory fades. */
  unlocked: boolean;
}

/** One step of the rating's history. */
export interface RatingPoint {
  /** Epoch milliseconds. */
  at: number;
  rating: number;
}

/** What the dashboard's Rating card shows. Mirrors `RatingSummary` in db/rating.rs. */
export interface RatingSummary {
  rating: number;
  /** Problems the rating is based on. */
  games: number;
  /** Too few games for the number to mean much yet. */
  provisional: boolean;
  /** How far it moved over the last 7 days. */
  weeklyChange: number;
  /** Oldest first. Empty before the first solve; otherwise the first entry is the starting rating. */
  history: RatingPoint[];
}

export type StatusFilter = "all" | "solved" | "unsolved";
export type PaidFilter = "any" | "free" | "paid";

export interface ProblemFilter {
  /** Free text over title and displayed number (not tags; use `tags` to filter by topic). Each word is a prefix match and all must match. */
  query?: string | null;
  /** Omit/null for any difficulty. An empty array matches nothing. */
  difficulties?: Difficulty[] | null;
  /** The problem must have *all* of these tags. */
  tags?: string[];
  /** "unsolved" includes attempted-but-never-accepted problems. */
  status?: StatusFilter;
  collectionId?: string | null;
  paid?: PaidFilter;
}

export type SortKey = "number" | "title" | "difficulty" | "progress" | "solveCount" | "acceptanceRate";

export interface ProblemQuery {
  filter?: ProblemFilter;
  /** Omitted: by problem number, or best text match first when `filter.query` is set. Ties fall back to number. */
  sort?: { key: SortKey; dir?: "asc" | "desc" };
  limit?: number;
  offset?: number;
}

// ---- Calls ------------------------------------------------------------------

/** Anything that went wrong talking to storage. `message` is the Rust-side error text. */
export class DbError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DbError";
  }
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    throw new DbError(
      "Local storage is only available inside the Tauri app, not in a plain browser tab. Use `npm run tauri dev`.",
    );
  }
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    // Tauri rejects with the serialized Rust error, which is a plain string.
    throw new DbError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

/** Problems matching a filter, sorted and optionally paged. All fields of `query` are optional. */
export function listProblems(query: ProblemQuery = {}): Promise<ProblemPage> {
  return call("list_problems", { query });
}

/** Full problem by internal id, or `null` if there is none. */
export function getProblem(id: number): Promise<ProblemDetail | null> {
  return call("get_problem", { id });
}

/** Full problem by URL slug (e.g. "two-sum"), or `null` if there is none. */
export function getProblemBySlug(slug: string): Promise<ProblemDetail | null> {
  return call("get_problem_by_slug", { slug });
}

/** Every topic tag with its problem count, most common first. */
export function listTags(): Promise<TagCount[]> {
  return call("list_tags");
}

/** The user's collections in their chosen order. */
export function listCollections(): Promise<CollectionSummary[]> {
  return call("list_collections");
}

/**
 * Makes a new collection that holds this problem. (Collections only exist while they have
 * problems, so there is no way to make an empty one.) Rejects if the name is blank, over 60
 * characters, or already one of the user's collections (ignoring case).
 */
export function createCollection(name: string, problemId: number): Promise<CollectionSummary> {
  return call("create_collection", { name, problemId });
}

/** Adds a problem to one of the user's collections. Fine if it's already in it. */
export function addProblemToCollection(collectionId: string, problemId: number): Promise<void> {
  return call("add_problem_to_collection", { collectionId, problemId });
}

/** Takes a problem out of a collection. A collection left with no problems is deleted. */
export function removeProblemFromCollection(collectionId: string, problemId: number): Promise<void> {
  return call("remove_problem_from_collection", { collectionId, problemId });
}

/** Notes that the user just opened a problem. Rejects if there is no problem with this id. */
export function recordProblemOpened(id: number): Promise<void> {
  return call("record_problem_opened", { id });
}

/** The problems opened most recently, newest first. `limit` defaults to 10 (max 100). */
export function listRecentProblems(limit?: number): Promise<RecentProblem[]> {
  return call("list_recent_problems", { limit });
}

/**
 * Saves the user's code for a problem in a language as an unsubmitted draft. Resolves to whether
 * a draft is now stored: false means the code matches the starter code, so there is none (and
 * any earlier draft was removed). Rejects if the problem has no starter code in that language.
 */
export function saveDraft(problemId: number, lang: string, code: string): Promise<boolean> {
  return call("save_draft", { problemId, lang, code });
}

/** Discards the draft for a problem in a language. Fine if there isn't one. */
export function deleteDraft(problemId: number, lang: string): Promise<void> {
  return call("delete_draft", { problemId, lang });
}

/**
 * When each of the user's submissions was made (any problem, any verdict), oldest first, as epoch
 * milliseconds UTC. Which day each falls on is up to the caller, in the user's local time.
 */
export function listSubmissionTimes(): Promise<number[]> {
  return call("list_submission_times");
}

/**
 * Today's recommended problems: a few due reviews, then new problems for the patterns the user
 * knows least. `day` is the local date (`localDateKey`). Made the first time it's asked for each
 * day, then kept.
 */
export function getDailyRecommendations(day: string): Promise<DailySet> {
  return call("get_daily_recommendations", { day });
}

/** The user's strength in each of the 17 patterns, in learning order. */
export function getPatternStrengths(): Promise<PatternStrength[]> {
  return call("get_pattern_strengths");
}

/** The overall skill rating, its history (oldest first) and its change over the last 7 days. */
export function getRating(): Promise<RatingSummary> {
  return call("get_rating");
}

/** The AI coach's providers. Mirrors `COACH_PROVIDERS` in db/settings.rs. "other" is any server that speaks OpenAI's chat API (OpenRouter, Ollama, LM Studio, ...). */
export type CoachProviderName = "anthropic" | "openai" | "other";

/** The AI coach's settings. Mirrors `CoachSettings` in db/settings.rs. (The API key isn't one: it lives in the system's credential store, see coachSetup.svelte.ts.) */
export interface CoachSettings {
  /** Whether the Coach shows in the Solve tab. On by default. */
  enabled: boolean;
  provider: CoachProviderName;
  /** The model's name as the provider calls it; empty means Anthropic's default (and "not ready" for the others). */
  model: string;
  /** Where an OpenAI-compatible server lives, e.g. http://localhost:11434/v1. Only used by "other". */
  baseUrl: string;
  /** What the user pays per million input and output tokens (US dollars), if they've said. */
  priceInput: number | null;
  priceOutput: number | null;
}

/** The user's preferences. Mirrors `Settings` in db/settings.rs. */
export interface Settings {
  /** How many problems the daily set has (1 to 10, 3 by default). */
  dailyProblemCount: number;
  /** Whether the app's curated collections (Blind 75, ...) show in the problem list. Off by default. */
  showCuratedCollections: boolean;
  coach: CoachSettings;
}

export const MIN_DAILY_PROBLEMS = 1;
export const MAX_DAILY_PROBLEMS = 10;
export const MAX_MODEL_NAME_LENGTH = 100;

export function getSettings(): Promise<Settings> {
  return call("get_settings");
}

/** Takes effect on today's set at once: raising it tops it up, lowering it trims it. Rejects outside 1 to 10. */
export function setDailyProblemCount(count: number): Promise<void> {
  return call("set_daily_problem_count", { count });
}

export function setShowCuratedCollections(show: boolean): Promise<void> {
  return call("set_show_curated_collections", { show });
}

/**
 * Erases everything saved in the app's database (drafts, submissions, ratings, reviews, collections,
 * notes, settings, saved conversations) and leaves it as on a first launch. The LeetCode login and
 * API keys are in the system credential store, not the database: clear those separately.
 * With `keepAiSetup`, the AI provider, model, address and prices are kept as they were. Pass it when
 * the keys are being kept: a key only works with the provider it was saved for.
 * The page should be reloaded afterwards, since what it holds in memory is now out of date.
 */
export function resetUserData(keepAiSetup: boolean): Promise<void> {
  return call("reset_user_data", { keepAiSetup });
}

/** Saves the AI coach's settings. Rejects an unknown provider, a model name over 100 characters, a base URL that isn't http(s), or a price out of range. */
export function setCoachSettings(settings: CoachSettings): Promise<void> {
  return call("set_coach_settings", { settings });
}

/** A problem's give-up state, with a preview of giving up now. Mirrors `GiveUpInfo` in db/give_ups.rs. */
export interface GiveUpInfo {
  /** When the user gave up (epoch ms), if they did and haven't solved the problem since. */
  gaveUpAt: number | null;
  /** From when a re-solve counts toward the rating again. */
  retryAt: number | null;
  /** What a clean re-solve from `retryAt` would win back (0 when nothing, or no give-up). */
  recoverable: number;
  /** Whether giving up now would move the rating at all (not for a problem that already has a result). */
  affectsRating: boolean;
  /** What giving up now would do to the rating: zero or negative. */
  previewChange: number;
  /** What a clean re-solve after that would win back. */
  previewRecoverable: number;
  /** The wait before a re-solve counts toward the rating, in days. */
  waitDays: number;
}

/**
 * Gives up on a problem: logs an Again review for it and, if it has no result yet, a loss in the
 * rating that a later solve can win most of back. Resolves to the problem's give-up state as it
 * now stands. (Showing the solution is the caller's job.)
 */
export function giveUp(problemId: number): Promise<GiveUpInfo> {
  return call("give_up", { problemId });
}

/** A problem's give-up state, and what giving up now would do to the rating. */
export function getGiveUpInfo(problemId: number): Promise<GiveUpInfo> {
  return call("get_give_up_info", { problemId });
}

/** Sets how a solve went (1 Again, 2 Hard, 3 Good, 4 Easy), replacing the guessed grade. */
export function setReviewGrade(reviewId: string, grade: number): Promise<void> {
  return call("set_review_grade", { reviewId, grade });
}

/** A problem's submissions, newest first. (Making one is `submitCode` in leetcode.ts.) */
export function listSubmissions(problemId: number): Promise<Submission[]> {
  return call("list_submissions", { problemId });
}

// ---- The AI coach's saved conversations ---------------------------------------------------

/** One saved message. `body` and a thread's `state` are the frontend's own JSON (see coach.svelte.ts). Mirrors `MessageRecord` in db/coach.rs. */
export interface SavedMessage {
  /** Its place in the thread; saving the same place again replaces it. */
  seq: number;
  role: "user" | "coach";
  label: string | null;
  body: unknown;
  /** The code the message was written about, so its line references can be checked later. */
  aboutCode: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
}

/** What a problem's saved coaching looks like. Mirrors `Loaded` in db/coach.rs. */
export interface SavedCoaching {
  /** The live conversation, if there is one. */
  thread: { state: unknown; messages: SavedMessage[] } | null;
  /** Tokens used by every conversation about the problem, live or not. */
  problemUsage: { input: number; output: number };
}

export function loadCoachThread(problemId: number): Promise<SavedCoaching> {
  return call("load_coach_thread", { problemId });
}

export function saveCoachMessage(problemId: number, message: SavedMessage): Promise<void> {
  return call("save_coach_message", { problemId, message });
}

export function saveCoachState(problemId: number, state: unknown): Promise<void> {
  return call("save_coach_state", { problemId, state });
}

/** Ends the problem's live conversation. The next message starts a new one. */
export function newCoachThread(problemId: number): Promise<void> {
  return call("new_coach_thread", { problemId });
}
