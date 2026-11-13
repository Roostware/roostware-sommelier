// Typed access to judging code on LeetCode (Run and Submit) and the LeetCode login.
//
// Like db.ts, components import from here and never call `invoke()` directly. Types mirror
// src-tauri/src/leetcode/ (camelCase on the wire). The login itself (the session cookies) never
// reaches the frontend: it lives in the OS credential store and the Rust side attaches it.

import { invoke } from "@tauri-apps/api/core";
import type { Submission } from "./db";
import { isTauri } from "./platform";

// ---- Types ------------------------------------------------------------------

/** Whether a LeetCode login is stored, and whose. */
export interface AuthStatus {
  signedIn: boolean;
  username: string | null;
}

/** One example test case from a Run. */
export interface RunCase {
  /** One line per argument. */
  input: string;
  expected: string;
  output: string;
  /** What the code printed while running this case. */
  stdout: string;
  passed: boolean;
}

/** The outcome of running code against test cases without submitting it. */
export interface RunResult {
  /** "Accepted" (every case matched), "Wrong Answer", or LeetCode's own message ("Compile Error", "Runtime Error", …). */
  status: string;
  /** Whether every case matched. */
  correct: boolean;
  cases: RunCase[];
  /**
   * When the code didn't run to completion: the one case LeetCode says it stopped on (a line per
   * argument), if it says. Otherwise null, including when the run completed.
   */
  lastInput: string | null;
  runtimeMs: number | null;
  memoryKb: number | null;
  /** Compile or runtime error text. */
  error: string | null;
}

/** What went wrong, so the UI can tell "sign in again" from the rest. */
export type LeetCodeErrorKind =
  | "notSignedIn"
  | "sessionExpired"
  | "blocked"
  | "rateLimited"
  | "cancelled"
  | "network"
  | "unexpected"
  | "credentials"
  | "storage"
  | "other";

export class LeetCodeError extends Error {
  constructor(
    readonly kind: LeetCodeErrorKind,
    message: string,
  ) {
    super(message);
    this.name = "LeetCodeError";
  }

  /** The user needs to sign in (again) before this can work. */
  get needsSignIn() {
    return this.kind === "notSignedIn" || this.kind === "sessionExpired";
  }
}

// ---- Calls ------------------------------------------------------------------

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    throw new LeetCodeError("other", "Running code is only available inside the Tauri app, not in a plain browser tab.");
  }
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    // Rust rejects with { kind, message } (see leetcode/error.rs).
    if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
      throw new LeetCodeError(String(e.kind) as LeetCodeErrorKind, String(e.message));
    }
    throw new LeetCodeError("other", typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

/** Whether a login is stored. Doesn't contact LeetCode, so it can't tell if the login has expired. */
export function leetcodeStatus(): Promise<AuthStatus> {
  return call("leetcode_status");
}

/**
 * Opens LeetCode's sign-in page in its own window and resolves once the user has signed in.
 * Rejects with kind "cancelled" if they close the window first.
 */
export function signInToLeetCode(): Promise<AuthStatus> {
  return call("leetcode_sign_in");
}

/** Forgets the stored login. */
export function signOutOfLeetCode(): Promise<AuthStatus> {
  return call("leetcode_sign_out");
}

/**
 * Runs code on LeetCode against `input` (one line per argument, cases one after another)
 * without submitting it, or against the problem's example cases if there is none. Nothing is recorded.
 */
export function runCode(problemId: number, lang: string, code: string, input?: string): Promise<RunResult> {
  return call("run_code", { problemId, lang, code, input });
}

/**
 * Submits code to LeetCode for judging and records the verdict in the user's history. Settles
 * the draft for that language, so reload the problem afterwards.
 */
export function submitCode(problemId: number, lang: string, code: string): Promise<Submission> {
  return call("submit_code", { problemId, lang, code });
}
