// The LeetCode side of the Solve tab: who's signed in, and the Run / Submit results for each
// problem. Like the open problem (solve.svelte.ts), this is module state: it survives switching
// tabs but not restarting the app. (Submissions themselves are stored by the backend.)

import { listSubmissions, setReviewGrade, type ProblemDetail, type Submission } from "$lib/db";
import {
  LeetCodeError,
  leetcodeStatus,
  runCode,
  signInToLeetCode,
  signOutOfLeetCode,
  submitCode,
  type AuthStatus,
  type RunResult,
} from "$lib/leetcode";
import { solve } from "$lib/solve.svelte";
import { caseFromInput, joinCases, layoutOrFreeform, sameCases, type Layout, type TestCase } from "$lib/testcases";

/** How a problem's test cases are laid out: its argument names and its example cases. */
export function layoutOf(problem: ProblemDetail): Layout {
  const python = problem.snippets.find((s) => s.lang === "python3")?.code;
  return layoutOrFreeform(problem.exampleTestCases, python);
}

/** What the last Run or Submit for a problem came to. */
export type Outcome =
  | { kind: "run"; lang: string; result: RunResult }
  | { kind: "submit"; submission: Submission }
  | { kind: "error"; action: "run" | "submit"; error: LeetCodeError };

export type JudgeTab = "testcase" | "result" | "history";

/** Everything the console panel shows for one problem. */
export interface JudgeState {
  /** Set while a Run or Submit is waiting on LeetCode. */
  busy: "run" | "submit" | null;
  outcome: Outcome | null;
  /** The user's edits to the test cases Run uses; null until they change them (then it's the examples). */
  cases: TestCase[] | null;
  /** Which test case is showing in the Testcase tab. */
  selectedCase: number;
  /** The problem's submission history; null until loaded. */
  history: Submission[] | null;
  historyError: string | null;
  open: boolean;
  tab: JudgeTab;
  /** True from an Accepted submission arriving until its celebration has played. */
  celebrating: boolean;
}

const fresh = (): JudgeState => ({
  busy: null,
  outcome: null,
  cases: null,
  selectedCase: 0,
  history: null,
  historyError: null,
  open: false,
  tab: "testcase",
  celebrating: false,
});

const asError = (e: unknown) => (e instanceof LeetCodeError ? e : new LeetCodeError("other", e instanceof Error ? e.message : String(e)));

class LeetCodeSession {
  /** Null until first loaded. */
  status = $state<AuthStatus | null>(null);
  /** True while the sign-in window is open. */
  signingIn = $state(false);
  /** Why signing in or out failed (not set when the user just closes the window). */
  authError = $state<string | null>(null);
  /** LeetCode turned the stored login down, so it needs signing in again. */
  expired = $state(false);

  #states = $state<Record<number, JudgeState>>({});

  get signedIn() {
    return this.status?.signedIn === true && !this.expired;
  }

  /** Reads whether a login is stored. Safe to call repeatedly (and outside the Tauri app). */
  async load() {
    try {
      this.status = await leetcodeStatus();
    } catch (e) {
      this.status = { signedIn: false, username: null };
      console.warn("Couldn't read the LeetCode login:", e);
    }
  }

  async signIn() {
    if (this.signingIn) return;
    this.signingIn = true;
    this.authError = null;
    try {
      this.status = await signInToLeetCode();
      this.expired = false;
    } catch (e) {
      const error = asError(e);
      if (error.kind !== "cancelled") this.authError = error.message;
    } finally {
      this.signingIn = false;
    }
  }

  async signOut() {
    this.authError = null;
    try {
      this.status = await signOutOfLeetCode();
      this.expired = false;
    } catch (e) {
      this.authError = asError(e).message;
    }
  }

  /** The panel's state for a problem. Reading creates nothing, so it's safe inside `$derived`. */
  stateFor(problemId: number): JudgeState {
    return this.#states[problemId] ?? EMPTY;
  }

  #mutable(problemId: number): JudgeState {
    this.#states[problemId] ??= fresh();
    return this.#states[problemId];
  }

  setTab(problemId: number, tab: JudgeTab) {
    const s = this.#mutable(problemId);
    s.tab = tab;
    s.open = true;
    if (tab === "history") void this.loadHistory(problemId);
  }

  toggle(problemId: number) {
    const s = this.#mutable(problemId);
    s.open = !s.open;
    if (s.open && s.tab === "history") void this.loadHistory(problemId);
  }

  // ---- Test cases ---------------------------------------------------------------
  // Kept for the session, like the rest of this panel. Until the user changes something, a
  // problem's cases are just its examples.

  /** The test cases Run will use for a problem. */
  casesFor(problem: ProblemDetail): TestCase[] {
    return this.stateFor(problem.id).cases ?? layoutOf(problem).examples;
  }

  /** Whether the user has changed the cases from the examples. */
  isEdited(problem: ProblemDetail): boolean {
    const cases = this.stateFor(problem.id).cases;
    return cases !== null && !sameCases(cases, layoutOf(problem).examples);
  }

  /** Which case is showing, kept within range as cases are removed. */
  selectedCase(problem: ProblemDetail): number {
    return Math.min(this.stateFor(problem.id).selectedCase, this.casesFor(problem).length - 1);
  }

  /** Copies the cases into the user's own list (the first time they change one) and returns it. */
  #editable(problem: ProblemDetail): TestCase[] {
    const s = this.#mutable(problem.id);
    s.cases ??= layoutOf(problem).examples.map((c) => [...c]);
    return s.cases;
  }

  selectCase(problemId: number, index: number) {
    this.#mutable(problemId).selectedCase = index;
  }

  editCase(problem: ProblemDetail, caseIndex: number, paramIndex: number, value: string) {
    const cases = this.#editable(problem);
    if (cases[caseIndex]) cases[caseIndex][paramIndex] = value;
  }

  /** Adds a case (blank unless `values` is given) and selects it. */
  addCase(problem: ProblemDetail, values?: TestCase) {
    const cases = this.#editable(problem);
    const blank = layoutOf(problem).params.map(() => "");
    cases.push(values ? [...values] : blank);
    this.#mutable(problem.id).selectedCase = cases.length - 1;
  }

  /** Removes a case. The last one can't be removed: Run needs something to run. */
  removeCase(problem: ProblemDetail, index: number) {
    const cases = this.#editable(problem);
    if (cases.length <= 1) return;
    cases.splice(index, 1);
    const s = this.#mutable(problem.id);
    s.selectedCase = Math.min(s.selectedCase > index ? s.selectedCase - 1 : s.selectedCase, cases.length - 1);
  }

  /** Back to the problem's own example cases. */
  resetCases(problemId: number) {
    const s = this.#mutable(problemId);
    s.cases = null;
    s.selectedCase = 0;
  }

  /**
   * Adds the input LeetCode reports for a failed submission as a new case and shows it, so it can
   * be re-run while fixing the code. Does nothing if the input doesn't split into the problem's arguments.
   */
  addFailingInput(problem: ProblemDetail, input: string): boolean {
    const values = caseFromInput(input, layoutOf(problem).params.length);
    if (!values || layoutOf(problem).freeform) return false;
    this.addCase(problem, values);
    this.setTab(problem.id, "testcase");
    return true;
  }

  /**
   * Sets how the problem just submitted went (1 Again to 4 Easy), replacing the grade the app
   * guessed. It decides when the problem comes back for review. Rejects if saving fails.
   */
  async setGrade(problemId: number, grade: number) {
    const s = this.#mutable(problemId);
    const submission = s.outcome?.kind === "submit" ? s.outcome.submission : null;
    if (!submission?.reviewId) return;
    await setReviewGrade(submission.reviewId, grade);
    submission.reviewGrade = grade;
    const inHistory = s.history?.find((h) => h.id === submission.id);
    if (inHistory) inHistory.reviewGrade = grade;
  }

  /** The Accepted submission, if the last Run or Submit for a problem was one. */
  acceptedThisSitting(problemId: number): Submission | null {
    const outcome = this.stateFor(problemId).outcome;
    return outcome?.kind === "submit" && outcome.submission.status === "Accepted" ? outcome.submission : null;
  }

  /** Forgets everything the console panel was showing for a problem (results, cases, tab). */
  reset(problemId: number) {
    delete this.#states[problemId];
  }

  /** The celebration for an accepted submission has played. */
  endCelebration(problemId: number) {
    this.#mutable(problemId).celebrating = false;
  }

  async loadHistory(problemId: number) {
    const s = this.#mutable(problemId);
    try {
      s.history = await listSubmissions(problemId);
      s.historyError = null;
    } catch (e) {
      s.historyError = asError(e).message;
    }
  }

  #noteFailure(error: LeetCodeError) {
    if (error.needsSignIn) this.expired = true;
  }

  /**
   * Runs the user's code against the problem's test cases (its examples, plus whatever the user
   * has changed or added). `code` is read once any pending autosave has landed.
   */
  async run(problem: ProblemDetail, lang: string, code: () => string) {
    const problemId = problem.id;
    const s = this.#mutable(problemId);
    if (s.busy) return;
    s.busy = "run";
    s.open = true;
    s.tab = "result";
    // Taken now, so editing the cases while LeetCode is thinking can't change what was run.
    const input = joinCases(this.casesFor(problem));
    try {
      await solve.flush();
      s.outcome = { kind: "run", lang, result: await runCode(problemId, lang, code(), input) };
    } catch (e) {
      const error = asError(e);
      this.#noteFailure(error);
      s.outcome = { kind: "error", action: "run", error };
    } finally {
      s.busy = null;
    }
  }

  /**
   * Submits the user's code for judging. The unsaved edits go to the database first, so the
   * backend can tell whether the draft it's about to settle is exactly what was submitted.
   */
  async submit(problemId: number, lang: string, code: () => string) {
    const s = this.#mutable(problemId);
    if (s.busy) return;
    s.busy = "submit";
    s.open = true;
    s.tab = "result";
    try {
      await solve.flush();
      const submission = await submitCode(problemId, lang, code());
      s.outcome = { kind: "submit", submission };
      if (submission.status === "Accepted") s.celebrating = true;
      void this.loadHistory(problemId);
      // The problem's progress, draft and "last submitted" code all just changed.
      if (solve.problemId === problemId) void solve.refresh();
    } catch (e) {
      const error = asError(e);
      this.#noteFailure(error);
      s.outcome = { kind: "error", action: "submit", error };
    } finally {
      s.busy = null;
    }
  }
}

// What stateFor returns for a problem nothing has been done with yet.
const EMPTY: JudgeState = fresh();

export const leetcode = new LeetCodeSession();
