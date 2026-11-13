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

/** A problem's submissions, newest first. (Making one is `submitCode` in leetcode.ts.) */
export function listSubmissions(problemId: number): Promise<Submission[]> {
  return call("list_submissions", { problemId });
}
