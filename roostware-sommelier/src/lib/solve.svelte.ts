// The Solve tab's session: which problem is open, and the code being written for it.
//
// Which problem is open lives in memory on purpose. Module state survives switching tabs (the
// pages unmount, this doesn't), but is gone when the app is closed and relaunched, so the app
// always starts with no problem open.
//
// The code is different: it is autosaved. Every edit goes into memory straight away (so the
// editor can be rebuilt without losing anything) and is written to the database shortly after
// the user stops typing. The backend decides whether it counts as a draft (code identical to
// the starter code doesn't), and a problem with a draft is "in progress". Saved drafts are
// loaded back in when a problem is opened, so they survive restarting the app.

import { goto } from "$app/navigation";
import {
  deleteDraft,
  getProblem,
  recordProblemOpened,
  saveDraft,
  type ProblemDetail,
} from "$lib/db";
import { initialLanguage } from "$lib/languages";
import { isTauri } from "$lib/platform";

/** Quiet time after the last keystroke before the code is saved... */
const SAVE_DELAY_MS = 600;
/** ...but while typing continues without a pause, it's saved at least this often. */
const SAVE_MAX_WAIT_MS = 5000;
/** On closing the window, give up waiting for pending saves after this long. */
const CLOSE_SAVE_TIMEOUT_MS = 2000;

const keyOf = (problemId: number, lang: string) => `${problemId}:${lang}`;
const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

class SolveSession {
  /** The open problem's internal id, or null when none is open (the Solve tab then shows the picker). */
  problemId = $state<number | null>(null);
  /** Its catalog data. Null until loaded (and while loading a different problem). */
  problem = $state<ProblemDetail | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);

  /** The language showing in the editor for the open problem; null until it's chosen on load. */
  lang = $state<string | null>(null);
  /** The language the user last picked themselves; preferred for problems opened next. */
  preferredLang = $state<string | null>(null);

  /** Set while the latest attempt to save the user's code has failed. */
  saveError = $state<string | null>(null);

  /** How many hint steps have been revealed, by problem. Kept for the session, like the open problem. */
  #hintCounts = $state<Record<number, number>>({});

  /** How many hint steps the user has revealed for a problem. */
  hintsShown(problemId: number): number {
    return this.#hintCounts[problemId] ?? 0;
  }

  /** Reveals the next hint step for a problem, if there is one (`total` is how many it has). */
  revealHint(problemId: number, total: number) {
    const shown = this.hintsShown(problemId);
    if (shown < total) this.#hintCounts[problemId] = shown + 1;
  }

  // The user's code, by problem and language. Plain (non-reactive) storage: the editor owns the
  // live text, and this remembers it for when the editor is rebuilt and feeds the autosave.
  #code = new Map<string, string>();
  // Problems whose saved drafts have already been loaded into #code. Loaded once: after that,
  // memory is the newer truth and a late response must never overwrite it.
  #seeded = new Set<number>();
  // Saves waiting out their quiet period, by key.
  #pending = new Map<string, { timer: ReturnType<typeof setTimeout>; firstAt: number }>();
  // The last write in flight for each key. Writes for a key run one after another, so an
  // older save can't land after (and overwrite) a newer one.
  #chain = new Map<string, Promise<void>>();

  // Bumped whenever the open problem changes, so a slow response for a problem the user
  // has already moved away from can't overwrite the current one.
  #generation = 0;

  /** True while anything the user wrote hasn't reached the database yet. */
  get hasUnsavedCode() {
    return this.#pending.size > 0 || this.#chain.size > 0;
  }

  /** Loads (or reloads) the open problem's data. Keeps what's shown while refreshing the same problem. */
  async refresh() {
    const id = this.problemId;
    if (id === null) return;
    const generation = this.#generation;
    this.loading = true;
    try {
      const problem = await getProblem(id);
      if (generation !== this.#generation) return;
      if (problem === null) throw new Error(`Problem ${id} was not found.`);
      this.#seed(problem);
      // Settle the language before the problem appears, so the editor starts in the right one.
      this.lang ??= initialLanguage(
        problem.snippets.map((s) => s.lang),
        problem.drafts,
        this.preferredLang,
      );
      this.problem = problem;
      this.error = null;
    } catch (e) {
      if (generation !== this.#generation) return;
      this.error = messageOf(e);
    }
    this.loading = false;
  }

  /**
   * Brings the user's saved code for a problem into memory, the first time it's loaded: their
   * draft in each language, or failing that the code they last submitted in it.
   */
  #seed(problem: ProblemDetail) {
    if (this.#seeded.has(problem.id)) return;
    this.#seeded.add(problem.id);
    for (const saved of [...problem.drafts, ...problem.lastSubmitted]) {
      const key = keyOf(problem.id, saved.lang);
      if (!this.#code.has(key)) this.#code.set(key, saved.code);
    }
  }

  open(id: number) {
    if (this.problemId !== id) {
      this.#generation++;
      this.problemId = id;
      this.problem = null; // never show the previous problem's data under the new one
      this.lang = null; // chosen again once the problem (and its drafts) have loaded
    }
    this.error = null;
    void this.refresh();
  }

  /** The user's code for a problem in a language, or undefined if they have none. */
  codeFor(problemId: number, lang: string): string | undefined {
    return this.#code.get(keyOf(problemId, lang));
  }

  /** The user picked a language in the editor. */
  selectLanguage(lang: string) {
    this.lang = lang;
    this.preferredLang = lang;
  }

  /** Records an edit and schedules it to be saved. */
  setCode(problemId: number, lang: string, code: string) {
    const key = keyOf(problemId, lang);
    this.#code.set(key, code);

    const now = Date.now();
    const previous = this.#pending.get(key);
    if (previous) clearTimeout(previous.timer);
    const firstAt = previous?.firstAt ?? now;
    const wait = Math.max(0, Math.min(SAVE_DELAY_MS, firstAt + SAVE_MAX_WAIT_MS - now));
    const timer = setTimeout(() => void this.#save(key, problemId, lang), wait);
    this.#pending.set(key, { timer, firstAt });
  }

  /** Discards the user's code for a problem in a language, so it starts from the starter code again. */
  resetCode(problemId: number, lang: string) {
    const key = keyOf(problemId, lang);
    this.#code.delete(key);
    const pending = this.#pending.get(key);
    if (pending) {
      clearTimeout(pending.timer);
      this.#pending.delete(key);
    }
    const starter =
      this.problem?.id === problemId ? this.problem.snippets.find((s) => s.lang === lang)?.code : undefined;
    // Queued behind any save already on its way, so it can't be undone by one.
    this.#enqueue(key, async () => {
      try {
        // Saving the starter code is how to say "back to the start": the backend stores no draft
        // for it, unless the user has submitted in this language, in which case the starter is
        // a change from what they submitted and is kept (so it's what they see next time).
        if (starter !== undefined) await saveDraft(problemId, lang, starter);
        else await deleteDraft(problemId, lang);
        this.saveError = null;
      } catch (e) {
        this.saveError = messageOf(e);
        console.warn("Couldn't discard the saved code:", e);
      }
    }).then(() => this.#afterWrite(problemId));
  }

  /** Runs `task` after whatever is already in flight for `key`. */
  #enqueue(key: string, task: () => Promise<void>): Promise<void> {
    const run = (this.#chain.get(key) ?? Promise.resolve()).then(task);
    this.#chain.set(key, run);
    void run.finally(() => {
      if (this.#chain.get(key) === run) this.#chain.delete(key);
    });
    return run;
  }

  async #save(key: string, problemId: number, lang: string) {
    this.#pending.delete(key);
    await this.#enqueue(key, async () => {
      // Read the code when it's this save's turn, so the latest text is what gets written.
      const code = this.#code.get(key);
      if (code === undefined) return; // reset in the meantime
      try {
        await saveDraft(problemId, lang, code);
        this.saveError = null;
      } catch (e) {
        this.saveError = messageOf(e);
        console.warn("Couldn't save the code:", e);
      }
    });
    this.#afterWrite(problemId);
  }

  /** Refreshes the open problem once everything has settled, so its progress is up to date. */
  #afterWrite(problemId: number) {
    if (problemId === this.problemId && !this.hasUnsavedCode) void this.refresh();
  }

  /** Saves everything that's waiting right now, without waiting out the quiet period. */
  async flush() {
    for (const [key, { timer }] of [...this.#pending]) {
      clearTimeout(timer);
      const [problemId, ...lang] = key.split(":");
      void this.#save(key, Number(problemId), lang.join(":"));
    }
    await Promise.allSettled([...this.#chain.values()]);
  }

  close() {
    this.#generation++;
    this.problemId = null;
    this.problem = null;
    this.lang = null;
    this.error = null;
    this.loading = false;
  }
}

export const solve = new SolveSession();

/**
 * Opens a problem in the Solve tab, from anywhere in the app: sets it as the open problem,
 * records it in the recently-opened history, and switches to the Solve tab.
 */
export async function openProblem(id: number) {
  solve.open(id);
  // History is a nicety; failing to save it must never stop the problem opening.
  recordProblemOpened(id).catch((e) => console.warn("Couldn't record the opened problem:", e));
  await goto("/solve");
}

export function closeProblem() {
  solve.close();
}

// ---- Don't lose the last few keystrokes when the app goes away ------------------------------
// Saving waits for a pause in typing, so make sure anything still waiting is written when the
// window is hidden, unloaded or closed.
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => void solve.flush());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") void solve.flush();
  });
  if (isTauri) void saveBeforeWindowCloses();
}

async function saveBeforeWindowCloses() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    // With a listener registered, Tauri leaves closing to us: unless we prevent it, the window
    // is destroyed once the handler returns. So only hold it open when there's something to save.
    await win.onCloseRequested(async (event) => {
      if (!solve.hasUnsavedCode) return;
      event.preventDefault();
      await Promise.race([solve.flush(), new Promise((r) => setTimeout(r, CLOSE_SAVE_TIMEOUT_MS))]);
      await win.destroy();
    });
  } catch (e) {
    console.warn("Couldn't set up saving on close:", e);
  }
}
