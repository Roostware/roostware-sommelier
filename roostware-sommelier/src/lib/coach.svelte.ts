// The AI coach's session: whether its drawer is open, and one conversation per problem.
//
// Like the open problem (solve.svelte.ts) and the judge panel (leetcode.svelte.ts), this is module
// state that survives switching tabs. Unlike them, conversations are also saved: every message is
// written to the database once it's complete, and a problem's conversation is read back the first
// time the problem is opened after a restart. (A reply that cost money isn't paid for twice.)
//
// What the coach can do depends on where the problem stands (`phaseOf`):
//   solving   nothing submitted yet       hints, guiding questions
//   wrong     the latest submission fails hints, debugging help
//   accepted  the latest one is Accepted  Big-O analysis and feedback, the optimal solution
//   gaveUp    the user gave up            the full solution, and questions about it
// The one rule that holds throughout: it never writes the solution except when the user gives up
// (or, once they've solved the problem, asks for the optimal one). The backend's prompts enforce
// that; this file decides what each button asks for.

import {
  getGiveUpInfo,
  giveUp,
  loadCoachThread,
  newCoachThread,
  saveCoachMessage,
  saveCoachState,
  type GiveUpInfo,
  type SavedMessage,
  type Submission,
} from "$lib/db";
import {
  asCoachError,
  backendProvider,
  DEBUG_STEPS,
  HINT_STEPS,
  MAX_DEBUG_LEVEL,
  MAX_HINT_LEVEL,
  type Analysis,
  type CoachAction,
  type CoachErrorKind,
  type CoachRequest,
  type HistoryMessage,
  type SharedSubmission,
  type SolutionExplanation,
  type TokenUsage,
} from "$lib/coachProvider";
import { COACH_NAME } from "$lib/coachName";
import { coachSetup } from "$lib/coachSetup.svelte";
import { leetcode } from "$lib/leetcode.svelte";

export type Phase = "solving" | "wrong" | "accepted" | "gaveUp";

/** What the Solve tab knows when it asks the coach something. */
export interface CoachContext {
  problemId: number;
  /** LeetCode language slug of the editor. */
  lang: string;
  /** The code in the editor right now. */
  getCode: () => string;
  /** The problem's submissions, newest first; null until loaded. */
  submissions: Submission[] | null;
}

/** The coach's context for the open problem. */
export function contextFor(problem: { id: number }, lang: string, getCode: () => string): CoachContext {
  return { problemId: problem.id, lang, getCode, submissions: leetcode.stateFor(problem.id).history };
}

export interface ProblemPhase {
  phase: Phase;
  /** The newest submission, if any. */
  latest: Submission | null;
  /** The newest Accepted submission, if any. */
  accepted: Submission | null;
  /** Submissions that didn't pass since the problem was last solved (or all of them). */
  failedSince: number;
}

/**
 * Where the problem stands, from its submissions (newest first) and its give-up state. Submissions
 * made at or before `since` (epoch ms) are ignored: that's a fresh start, where the problem is
 * treated as if it had never been solved (see `Thread.freshAfter`).
 */
export function phaseOf(submissions: Submission[] | null, giveUpInfo: GiveUpInfo | null, since = 0): ProblemPhase {
  const list = (submissions ?? []).filter((s) => s.submittedAt > since);
  const latest = list[0] ?? null;
  const acceptedAt = list.findIndex((s) => s.status === "Accepted");
  const accepted = acceptedAt === -1 ? null : list[acceptedAt];
  const failedSince = acceptedAt === -1 ? list.length : acceptedAt;
  const phase: Phase =
    giveUpInfo?.gaveUpAt != null ? "gaveUp" : latest === null ? "solving" : latest.status === "Accepted" ? "accepted" : "wrong";
  return { phase, latest, accepted, failedSince };
}

/** How long after the latest Accepted submission giving up still means something: before that it's the same sitting. */
const REVIEW_AFTER_MS = 12 * 60 * 60 * 1000;

/**
 * Whether to offer giving up: before the problem is solved, or on a problem solved long ago (a
 * review that went badly). Not right after solving it, and not once the user has already given up.
 */
export function canGiveUp(p: ProblemPhase): boolean {
  if (p.phase === "gaveUp") return false;
  if (p.phase === "accepted") return p.accepted !== null && Date.now() - p.accepted.submittedAt > REVIEW_AFTER_MS;
  return true;
}

export type MessageBody =
  | { kind: "text"; text: string }
  | { kind: "analysis"; analysis: Analysis; submissionId: string }
  | { kind: "solution"; solution: SolutionExplanation; viaGiveUp: boolean }
  | { kind: "notice"; text: string };

export interface CoachMessage {
  /** Its place in the conversation, and in the database. */
  id: number;
  role: "user" | "coach";
  /** A small heading for the reply, like "Hint 2 of 3 · Approach". */
  label: string | null;
  body: MessageBody;
  /** Still arriving. */
  pending: boolean;
  /** When it was asked (epoch ms; only while it is pending), so the panel can show how long it's been. */
  startedAt?: number;
  /** The user pressed Stop. */
  stopped: boolean;
  /** Why it failed, if it did. */
  error: string | null;
  /** What kind of failure, so the panel can offer the right next step (Settings, try again). */
  errorKind: CoachErrorKind | null;
  /** For a failed reply, how to ask again. */
  retry: { action: CoachAction; viaGiveUp: boolean } | null;
  /** The code the message was written about, so its line references can tell when the editor has moved on. */
  aboutCode: string | null;
  /** What the reply used, as the provider counted it. */
  usage: TokenUsage | null;
}

export interface Thread {
  messages: CoachMessage[];
  /** How many hint steps have been given (0 to 3). */
  hintLevel: number;
  /** How many debugging steps have been given, by the submission they were about. */
  debugLevels: Record<string, number>;
  /** A guiding-questions session is going: what the user types is an answer. */
  questioning: boolean;
  /** How many answers the user has given in it. */
  turn: number;
  /** Whether the editor's code goes along with hints and questions. */
  shareCode: boolean;
  /** Whether the latest submission's verdict goes along too. */
  shareSubmission: boolean;
  busy: boolean;
  /** The message holding the analysis of each submission, so asking again finds it instead of paying twice. */
  analyses: Record<string, number>;
  nextId: number;
  /** A message to bring into view (an analysis asked for twice). */
  focus: number | null;
  /**
   * When the problem was last given a fresh start (epoch ms; 0 for never): the coach acts as if
   * submissions up to then hadn't happened, so a problem solved before is offered hints and
   * questions again. Saved with the thread, and carried over when a new conversation starts.
   */
  freshAfter: number;
}

/** What's saved about a thread besides its messages. */
interface SavedState {
  hintLevel: number;
  debugLevels: Record<string, number>;
  questioning: boolean;
  turn: number;
  shareCode: boolean;
  shareSubmission: boolean;
  analyses: Record<string, number>;
  freshAfter: number;
}

const freshThread = (): Thread => ({
  messages: [],
  hintLevel: 0,
  debugLevels: {},
  questioning: false,
  turn: 0,
  shareCode: true,
  shareSubmission: true,
  busy: false,
  analyses: {},
  nextId: 0,
  focus: null,
  freshAfter: 0,
});

const EMPTY: Thread = freshThread();

const isAbort = (e: unknown) => e instanceof DOMException && e.name === "AbortError";
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;
const BODY_KINDS = ["text", "analysis", "solution", "notice"];
/** How many earlier messages go to the model with a new question. */
const HISTORY_MESSAGES = 10;

function shared(s: Submission): SharedSubmission {
  return { status: s.status, input: s.lastTestcase, output: s.codeOutput, expected: s.expectedOutput, error: s.errorMessage };
}

/** A saved thread's bookkeeping, with anything missing or wrong put back to its default. */
function restoredState(raw: unknown): SavedState {
  const fresh = freshThread();
  const s = isObject(raw) ? raw : {};
  const numbers = (v: unknown): Record<string, number> =>
    isObject(v) ? Object.fromEntries(Object.entries(v).filter(([, n]) => typeof n === "number")) as Record<string, number> : {};
  const level = (v: unknown) => (typeof v === "number" && v >= 0 ? Math.floor(v) : 0);
  return {
    hintLevel: Math.min(level(s.hintLevel), MAX_HINT_LEVEL),
    debugLevels: numbers(s.debugLevels),
    questioning: s.questioning === true,
    turn: level(s.turn),
    shareCode: typeof s.shareCode === "boolean" ? s.shareCode : fresh.shareCode,
    shareSubmission: typeof s.shareSubmission === "boolean" ? s.shareSubmission : fresh.shareSubmission,
    analyses: numbers(s.analyses),
    freshAfter: typeof s.freshAfter === "number" && s.freshAfter > 0 ? s.freshAfter : 0,
  };
}

function restoredMessage(m: SavedMessage): CoachMessage | null {
  if (!isObject(m.body) || !BODY_KINDS.includes(String(m.body.kind))) return null;
  return {
    id: m.seq,
    role: m.role,
    label: m.label,
    body: m.body as MessageBody,
    pending: false,
    stopped: false,
    error: null,
    errorKind: null,
    retry: null,
    aboutCode: m.aboutCode,
    usage: m.inputTokens === null && m.outputTokens === null ? null : { inputTokens: m.inputTokens ?? 0, outputTokens: m.outputTokens ?? 0 },
  };
}

class CoachSession {
  /** Whether the drawer is open (for whichever problem is showing). */
  open = $state(false);
  /** Something asked for from outside the drawer (the Judge panel's buttons), until the drawer has picked it up. */
  pending = $state<{ problemId: number; action: "debug" | "analyze" } | null>(null);

  #threads = $state<Record<number, Thread>>({});
  #giveUps = $state<Record<number, GiveUpInfo>>({});
  /** Tokens used by every conversation about a problem, saved ones included. */
  #usage = $state<Record<number, { input: number; output: number }>>({});
  #aborts = new Map<number, AbortController>();
  /** Problems whose saved conversation has been read (or is being read). */
  #loaded = new Set<number>();

  toggle() {
    this.open = !this.open;
  }

  /** The conversation for a problem. Reading creates nothing, so it's safe inside `$derived`. */
  thread(problemId: number): Thread {
    return this.#threads[problemId] ?? EMPTY;
  }

  #mutable(problemId: number): Thread {
    this.#threads[problemId] ??= freshThread();
    return this.#threads[problemId];
  }

  /** Tokens used by every conversation about a problem. */
  usageFor(problemId: number): { input: number; output: number } {
    return this.#usage[problemId] ?? { input: 0, output: 0 };
  }

  /** What's known about giving up on a problem; null until loaded. */
  giveUpInfo(problemId: number): GiveUpInfo | null {
    return this.#giveUps[problemId] ?? null;
  }

  /** Reads the problem's give-up state (and the preview of giving up now) from the backend. */
  async loadGiveUp(problemId: number) {
    try {
      this.#giveUps[problemId] = await getGiveUpInfo(problemId);
    } catch (e) {
      // Outside the app (a plain browser tab) there's no storage; nothing to show.
      console.warn("Couldn't read the give-up state:", e);
    }
  }

  // ---- Saved conversations ------------------------------------------------------

  /** Reads the problem's saved conversation, once per run of the app. */
  async loadSaved(problemId: number) {
    if (this.#loaded.has(problemId)) return;
    this.#loaded.add(problemId);
    try {
      const saved = await loadCoachThread(problemId);
      this.#usage[problemId] = saved.problemUsage;
      // Whatever the user did while this was loading is newer than what's saved.
      if (!saved.thread || this.#threads[problemId]) return;
      const t = freshThread();
      Object.assign(t, restoredState(saved.thread.state));
      t.messages = saved.thread.messages.map(restoredMessage).filter((m): m is CoachMessage => m !== null);
      t.nextId = saved.thread.messages.reduce((next, m) => Math.max(next, m.seq + 1), 0);
      this.#threads[problemId] = t;
    } catch (e) {
      this.#loaded.delete(problemId);
      console.warn("Couldn't read the saved conversation:", e);
    }
  }

  #saveMessage(problemId: number, m: CoachMessage) {
    const record: SavedMessage = {
      seq: m.id,
      role: m.role,
      label: m.label,
      body: $state.snapshot(m.body),
      aboutCode: m.aboutCode,
      inputTokens: m.usage?.inputTokens ?? null,
      outputTokens: m.usage?.outputTokens ?? null,
    };
    void this.#afterEnd(problemId).then(() =>
      saveCoachMessage(problemId, record).catch((e) => console.warn("Couldn't save the coach's message:", e)),
    );
  }

  #saveState(problemId: number, t: Thread) {
    const state: SavedState = {
      hintLevel: t.hintLevel,
      debugLevels: $state.snapshot(t.debugLevels),
      questioning: t.questioning,
      turn: t.turn,
      shareCode: t.shareCode,
      shareSubmission: t.shareSubmission,
      analyses: $state.snapshot(t.analyses),
      freshAfter: t.freshAfter,
    };
    void this.#afterEnd(problemId).then(() =>
      saveCoachState(problemId, state).catch((e) => console.warn("Couldn't save the conversation's state:", e)),
    );
  }

  // Ending a conversation is saved asynchronously; whatever is saved for the next one waits for
  // that, so it can't land first and be ended along with the old one.
  #ending = new Map<number, Promise<unknown>>();

  #afterEnd(problemId: number): Promise<unknown> {
    return this.#ending.get(problemId) ?? Promise.resolve();
  }

  // ---- The drawer ---------------------------------------------------------------

  /** Opens the drawer and has it run `action` as soon as it can (the Judge panel's buttons). */
  request(problemId: number, action: "debug" | "analyze") {
    this.open = true;
    this.pending = { problemId, action };
  }

  setShare(problemId: number, what: "code" | "submission", on: boolean) {
    const t = this.#mutable(problemId);
    if (what === "code") t.shareCode = on;
    else t.shareSubmission = on;
    this.#saveState(problemId, t);
  }

  /** Cuts a reply short. */
  stop(problemId: number) {
    this.#aborts.get(problemId)?.abort();
  }

  /**
   * Starts a fresh conversation for a problem. (Tokens already spent stay in the problem's total.)
   * With `freshStart`, the problem is also treated as never solved from here on: earlier
   * submissions stop counting for what the coach offers (they stay in the history).
   */
  newConversation(problemId: number, freshStart = false) {
    const freshAfter = freshStart ? Date.now() : this.thread(problemId).freshAfter;
    this.stop(problemId);
    delete this.#threads[problemId];
    this.#ending.set(
      problemId,
      newCoachThread(problemId).catch((e) => console.warn("Couldn't end the saved conversation:", e)),
    );
    if (freshAfter > 0) {
      const t = this.#mutable(problemId);
      t.freshAfter = freshAfter;
      this.#saveState(problemId, t);
    }
  }

  /**
   * Analyzes an accepted submission from the history, in a new conversation of its own (the drawer
   * opens to show it). Does nothing while a reply is being written.
   */
  analyzeSubmission(ctx: CoachContext, submission: Submission) {
    if (this.thread(ctx.problemId).busy) return;
    this.open = true;
    this.newConversation(ctx.problemId);
    return this.ask(ctx, "analyze", { subject: submission });
  }

  /** What the user typed: an answer in a guiding-questions session, otherwise a question. */
  send(ctx: CoachContext, text: string) {
    const t = this.thread(ctx.problemId);
    return this.ask(ctx, t.questioning ? "questions" : "chat", { message: text });
  }

  endQuestions(problemId: number) {
    const t = this.#mutable(problemId);
    if (!t.questioning) return;
    t.questioning = false;
    this.#saveState(problemId, t);
    this.#saveMessage(problemId, this.#push(t, { role: "coach", label: null, body: { kind: "notice", text: "Guiding questions ended." } }));
  }

  #push(t: Thread, m: Pick<CoachMessage, "role" | "label" | "body"> & Partial<CoachMessage>): CoachMessage {
    t.messages.push({
      id: t.nextId++,
      pending: false,
      stopped: false,
      error: null,
      errorKind: null,
      retry: null,
      aboutCode: null,
      usage: null,
      ...m,
    });
    return t.messages[t.messages.length - 1];
  }

  /**
   * Asks again after a failed reply: the failed reply and the question it was for are taken out
   * first (the new ones are saved over the same places), so the conversation doesn't show it twice.
   */
  retry(ctx: CoachContext, messageId: number) {
    const t = this.#mutable(ctx.problemId);
    const at = t.messages.findIndex((m) => m.id === messageId);
    const failed = t.messages[at];
    if (!failed?.retry || t.busy) return;
    const { action, viaGiveUp } = failed.retry;
    const before = t.messages[at - 1];
    const asked = before?.role === "user" && at === t.messages.length - 1 ? before : null;
    const keep = asked ? at - 1 : at;
    t.nextId = t.messages[keep].id;
    t.messages.splice(keep);
    if (action === "questions") {
      // The answer being retried was counted, and will be again. (The very first question
      // started the session, which asking starts again.)
      if (t.turn > 0) t.turn--;
      else t.questioning = false;
    }
    const message = asked?.body.kind === "text" ? asked.body.text : undefined;
    return this.ask(ctx, action, { viaGiveUp, message });
  }

  /** The conversation so far as plain text turns, for the model. */
  #history(t: Thread): HistoryMessage[] {
    const turns: HistoryMessage[] = [];
    for (const m of t.messages) {
      if (m.body.kind !== "text" || m.pending || m.error !== null) continue;
      const text = m.body.text.trim();
      if (text !== "") turns.push({ role: m.role, text });
    }
    return turns.slice(-HISTORY_MESSAGES);
  }

  /**
   * Asks the coach something. Hints and debugging climb a ladder (one rung per ask, to the top);
   * an analysis already made for a submission is found rather than made again. Does nothing when
   * the coach isn't set up (the drawer says what's missing).
   */
  async ask(ctx: CoachContext, action: CoachAction, options: { message?: string; viaGiveUp?: boolean; again?: boolean; subject?: Submission } = {}) {
    const id = ctx.problemId;
    const t = this.#mutable(id);
    if (t.busy || !coachSetup.ready) return;
    const p = phaseOf(ctx.submissions, this.giveUpInfo(id), t.freshAfter);

    // What's being asked, what to call it, and which submission it's about.
    let level = 0;
    let label: string | null = null;
    let said: string;
    let subject: Submission | null = null;
    switch (action) {
      case "hint":
        if (t.hintLevel >= MAX_HINT_LEVEL) return;
        level = t.hintLevel + 1;
        label = `Hint ${level} of ${MAX_HINT_LEVEL} · ${HINT_STEPS[level - 1]}`;
        said = level === 1 ? "Give me a hint" : "Another hint";
        break;
      case "debug": {
        subject = p.latest && p.latest.status !== "Accepted" ? p.latest : null;
        if (!subject) return;
        const done = t.debugLevels[subject.id] ?? 0;
        if (done >= MAX_DEBUG_LEVEL) return;
        level = done + 1;
        label = `Debugging ${level} of ${MAX_DEBUG_LEVEL} · ${DEBUG_STEPS[level - 1]}`;
        said = level === 1 ? "Why did this fail?" : "Go further";
        break;
      }
      case "analyze": {
        subject = options.subject ?? p.accepted;
        if (!subject) return;
        const existing = t.analyses[subject.id];
        if (existing !== undefined && !options.again) {
          t.focus = existing;
          return;
        }
        label = "Analysis";
        said = options.again ? "Analyze it again" : "Analyze my solution";
        break;
      }
      case "solution":
        label = options.viaGiveUp ? "The solution" : "The optimal solution";
        said = options.viaGiveUp ? "I give up. Show me the solution" : "Show me the optimal solution";
        break;
      case "questions":
        if (!t.questioning) {
          t.questioning = true;
          t.turn = 0;
          said = "Guide me with questions";
        } else {
          said = (options.message ?? "").trim();
          if (said === "") return;
          t.turn++;
        }
        label = `Question ${t.turn + 1}`;
        break;
      case "chat":
        said = (options.message ?? "").trim();
        if (said === "") return;
        break;
    }

    // What goes along. Debugging and analysis are about a submission, so its code and verdict
    // always do; for the rest the user's chips decide.
    const aboutSubmission = action === "debug" || action === "analyze";
    const code = aboutSubmission ? (subject?.code ?? null) : t.shareCode ? ctx.getCode() : null;
    const sharedSubmission = aboutSubmission ? subject : t.shareSubmission ? p.latest : null;
    const request: CoachRequest = {
      action,
      problemId: id,
      lang: ctx.lang,
      code,
      submission: sharedSubmission ? shared(sharedSubmission) : null,
      level,
      message: action === "chat" || (action === "questions" && t.turn > 0) ? said : null,
      turn: t.turn,
      failedAttempts: p.failedSince,
      solutionAllowed: p.phase === "gaveUp" || p.phase === "accepted",
      history: this.#history(t),
    };

    const userMessage = this.#push(t, { role: "user", label: null, body: { kind: "text", text: said } });
    this.#saveMessage(id, userMessage);
    const reply = this.#push(t, {
      role: "coach",
      label,
      body: { kind: "text", text: "" },
      pending: true,
      startedAt: Date.now(),
      aboutCode: code,
      retry: { action, viaGiveUp: options.viaGiveUp ?? false },
    });
    t.busy = true;
    const controller = new AbortController();
    this.#aborts.set(id, controller);
    const commit = () => {
      // The step counts once something of it has been shown.
      if (action === "hint") t.hintLevel = level;
      if (action === "debug" && subject) t.debugLevels[subject.id] = level;
      this.#saveState(id, t);
    };
    try {
      const result = await backendProvider.reply(request, {
        signal: controller.signal,
        onText: (soFar) => {
          reply.body = { kind: "text", text: soFar };
        },
      });
      const r = result.reply;
      if (r.kind === "text") reply.body = { kind: "text", text: r.text };
      else if (r.kind === "analysis") {
        reply.body = { kind: "analysis", analysis: r.analysis, submissionId: subject?.id ?? "" };
        if (subject) t.analyses[subject.id] = reply.id;
      } else reply.body = { kind: "solution", solution: r.solution, viaGiveUp: options.viaGiveUp ?? false };
      reply.usage = result.usage;
      reply.retry = null;
      const total = this.usageFor(id);
      this.#usage[id] = { input: total.input + result.usage.inputTokens, output: total.output + result.usage.outputTokens };
      commit();
      this.#saveMessage(id, reply);
    } catch (e) {
      if (isAbort(e)) {
        reply.stopped = true;
        const kept = reply.body.kind === "text" && reply.body.text.trim() !== "";
        if (kept) {
          reply.retry = null;
          commit();
          this.#saveMessage(id, reply);
        } else {
          // Nothing arrived: drop the empty reply, and note that it was stopped.
          t.messages = t.messages.filter((m) => m.id !== reply.id);
          if (action === "questions" && t.turn > 0) t.turn--;
          this.#saveMessage(id, this.#push(t, { role: "coach", label: null, body: { kind: "notice", text: "Stopped." } }));
        }
      } else {
        const error = asCoachError(e);
        reply.error = error.message;
        reply.errorKind = error.kind;
        // A refused or failed attempt at a step doesn't use it up; state still changes (questions, turn).
        this.#saveState(id, t);
      }
    } finally {
      reply.pending = false;
      t.busy = false;
      this.#aborts.delete(id);
    }
  }

  /**
   * Gives up on the open problem and shows the solution. Logs it first (an Again review, and the
   * rating's loss unless the problem already has a result), then has the coach explain. Rejects
   * if the coach isn't set up (nothing is logged then: giving up must never cost something and
   * show nothing) or if the give-up couldn't be saved. Resolves to false without doing anything
   * if the coach is in the middle of a reply.
   */
  async giveUpOn(ctx: CoachContext): Promise<boolean> {
    if (!coachSetup.ready) throw new Error(coachSetup.reason ?? `${COACH_NAME} isn't set up yet.`);
    if (this.thread(ctx.problemId).busy) return false;
    this.#giveUps[ctx.problemId] = await giveUp(ctx.problemId);
    this.open = true;
    const t = this.#mutable(ctx.problemId);
    t.questioning = false;
    await this.ask(ctx, "solution", { viaGiveUp: true });
    return true;
  }
}

export const coach = new CoachSession();
