// What the AI coach asks of a model, and how that reaches one.
//
// The UI only talks to a `CoachProvider`. The real one calls the Rust side (`coach_reply` in
// src-tauri/src/coach/), which builds the prompt, holds the API key, talks to the provider over
// HTTP and streams the text back over a channel. Nothing in the webview ever sees a key. Types
// mirror src-tauri/src/coach/ (camelCase on the wire); keep the two in sync.

import { Channel, invoke } from "@tauri-apps/api/core";
import { COACH_NAME } from "./coachName";
import { isTauri } from "./platform";

/** What the user asked for. */
export type CoachAction = "hint" | "questions" | "debug" | "analyze" | "solution" | "chat";

/** How far a hint (or a piece of debugging help) goes, from vaguest to most specific. */
export const HINT_STEPS = ["Idea", "Approach", "Near-pseudocode"] as const;
export const DEBUG_STEPS = ["Where", "Why", "The fix, in words"] as const;
export const MAX_HINT_LEVEL = HINT_STEPS.length;
export const MAX_DEBUG_LEVEL = DEBUG_STEPS.length;

/** The failed submission being debugged, as far as the user chose to share it. */
export interface SharedSubmission {
  status: string;
  /** The input LeetCode stopped on, what the code gave, and what was expected. */
  input: string | null;
  output: string | null;
  expected: string | null;
  error: string | null;
}

/** One earlier message of the conversation, text only. */
export interface HistoryMessage {
  role: "user" | "coach";
  text: string;
}

/** Everything the backend is given for one reply. */
export interface CoachRequest {
  action: CoachAction;
  /** The problem's internal id: the backend reads the statement from the catalog. */
  problemId: number;
  /** LeetCode language slug of the editor. */
  lang: string;
  /** The user's code, when they chose to share it (null otherwise). */
  code: string | null;
  /** The submission being discussed, when shared. */
  submission: SharedSubmission | null;
  /** Which step of the hint or debugging ladder this is (1-based); 0 for the rest. */
  level: number;
  /** What the user typed, for a question or an answer. */
  message: string | null;
  /** How many answers the user has given in a guiding-questions session. */
  turn: number;
  /** How many failed submissions came before. */
  failedAttempts: number;
  /** The user has solved the problem or given up, so the solution may be discussed and written. */
  solutionAllowed: boolean;
  /** The conversation so far (without this request's own message). */
  history: HistoryMessage[];
}

/** The Big-O analysis of an accepted submission. */
export interface Analysis {
  time: { big: string; why: string };
  space: { big: string; why: string };
  /** The best known bounds for the problem, and how they compare with the user's. */
  optimal: { time: string; space: string; note: string };
  wentWell: string[];
  /** Things to improve. Text may reference lines of the user's code as `[L7]` or `[L7-9]`. */
  improve: string[];
  takeaways: string[];
}

/** The full explanation shown after giving up (or on request after solving). */
export interface SolutionExplanation {
  intuition: string;
  steps: string[];
  code: string;
  complexity: string;
  /** Where the user's own attempt went a different way. Null when there was no attempt to compare. */
  divergence: string | null;
  takeaways: string[];
}

export type CoachReply =
  | { kind: "text"; text: string }
  | { kind: "analysis"; analysis: Analysis }
  | { kind: "solution"; solution: SolutionExplanation };

/** What a reply used, as the provider counted it. */
export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
}

export interface CoachResult {
  reply: CoachReply;
  usage: TokenUsage;
}

export type CoachErrorKind =
  | "notReady"
  | "credentials"
  | "network"
  | "auth"
  | "rateLimited"
  | "api"
  | "refused"
  | "truncated"
  | "unreadable"
  | "cancelled"
  | "invalid"
  | "storage"
  | "other";

/** Something went wrong asking a model. `message` never contains the API key. */
export class CoachError extends Error {
  constructor(
    readonly kind: CoachErrorKind,
    message: string,
  ) {
    super(message);
    this.name = "CoachError";
  }
}

/** Coach commands reject with `{ kind, message }` (or a plain string for storage errors). */
export function asCoachError(e: unknown): CoachError {
  if (e instanceof CoachError) return e;
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    const { kind, message } = e as { kind: CoachErrorKind; message: string };
    return new CoachError(kind, String(message));
  }
  return new CoachError("other", typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
}

/** Calls a coach command, turning whatever it rejects with into a `CoachError`. */
export async function callCoach<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    throw new CoachError("other", `${COACH_NAME} is only available inside the Tauri app, not in a plain browser tab. Use \`npm run tauri dev\`.`);
  }
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw asCoachError(e);
  }
}

export interface CoachProvider {
  /**
   * Answers a request. For a text reply, `onText` is called with the text so far as it arrives,
   * and the returned promise resolves with the whole reply. Rejects with an `AbortError` when
   * `signal` is aborted (the Stop button), and otherwise with a `CoachError`.
   */
  reply(request: CoachRequest, options: { signal: AbortSignal; onText: (soFar: string) => void }): Promise<CoachResult>;
}

const abortError = () => new DOMException("Stopped", "AbortError");

/** The real provider: the Rust side makes the call and streams the text back over a channel. */
export const backendProvider: CoachProvider = {
  async reply(request, { signal, onText }) {
    if (signal.aborted) throw abortError();
    const requestId = crypto.randomUUID();

    let soFar = "";
    const channel = new Channel<{ kind: "text"; text: string }>();
    channel.onmessage = (event) => {
      if (event.kind === "text") {
        soFar += event.text;
        onText(soFar);
      }
    };

    const stop = () => void invoke("coach_cancel", { requestId }).catch(() => undefined);
    signal.addEventListener("abort", stop, { once: true });
    try {
      return await callCoach<CoachResult>("coach_reply", { request: { requestId, ...request }, onEvent: channel });
    } catch (e) {
      const error = asCoachError(e);
      throw error.kind === "cancelled" ? abortError() : error;
    } finally {
      signal.removeEventListener("abort", stop);
    }
  },
};
