<script lang="ts">
  // Under the editor: the LeetCode account (a green dot while signed in, with the details in
  // Settings; a prompt when signing in is needed), the Run and Submit buttons, and a console that opens
  // above them with three tabs: the test cases Run will use (the problem's examples, which can be
  // edited and added to), what the last Run or Submit came to, and the problem's submission history.
  import { tick } from "svelte";
  import { coach, contextFor } from "$lib/coach.svelte";
  import { COACH_NAME } from "$lib/coachName";
  import type { ProblemDetail, Submission } from "$lib/db";
  import GiveUpButton from "$lib/GiveUpButton.svelte";
  import { languageLabel } from "$lib/languages";
  import { layoutOf, leetcode } from "$lib/leetcode.svelte";
  import type { RunResult } from "$lib/leetcode";
  import { settings } from "$lib/settings.svelte";
  import { settingsDialog } from "$lib/settingsDialog.svelte";
  import SubmittedCode from "$lib/SubmittedCode.svelte";
  import { caseFromInput, hasBlank } from "$lib/testcases";

  let { problem, lang, getCode }: { problem: ProblemDetail; lang: string; getCode: () => string } = $props();

  /** The coach's drawer is open beside the editor (see `showCoach` in ProblemWorkspace). */
  let coachOpen = $derived(settings.coachEnabled && coach.open);
  let judge = $derived(leetcode.stateFor(problem.id));
  let canJudge = $derived(leetcode.signedIn && judge.busy === null);
  let judgeTip = $derived(
    !leetcode.signedIn
      ? leetcode.expired
        ? "Your LeetCode session expired. Sign in again"
        : "Sign in to LeetCode first"
      : judge.busy
        ? "Waiting for LeetCode…"
        : "",
  );

  // ---- Test cases (the Testcase tab) ------------------------------------------
  let layout = $derived(layoutOf(problem));
  let cases = $derived(leetcode.casesFor(problem));
  let selected = $derived(leetcode.selectedCase(problem));
  let edited = $derived(leetcode.isEdited(problem));
  let blank = $derived(hasBlank(cases));
  // Run needs every argument of every case filled in (LeetCode rejects a blank one).
  let runTip = $derived(judgeTip || (blank ? "Fill in every input of every test case first" : ""));
  let fieldsEl: HTMLElement | undefined = $state();

  async function addCase() {
    leetcode.addCase(problem);
    await tick();
    fieldsEl?.querySelector("textarea")?.focus(); // a new case is blank, so start typing into it
  }

  // A failed submission's input can become a test case if it has one value per argument.
  const canAdd = (input: string | null) => !!input && !layout.freeform && caseFromInput(input, layout.params.length) !== null;

  // Which of a Run's cases is showing. Reset to "the first one that failed" for each new result.
  let picked = $state<number | null>(null);
  $effect(() => {
    void judge.outcome;
    picked = null;
  });
  function shownCase(result: RunResult): number {
    if (picked !== null && picked < result.cases.length) return picked;
    const failing = result.cases.findIndex((c) => !c.passed);
    return failing === -1 ? 0 : failing;
  }

  // Which submission in the history is expanded.
  let expanded = $state<string | null>(null);

  // ---- How did it go? (the grade of an Accepted submission) -----------------------
  // The app guesses a grade from how many failed attempts came first; the user can correct it.
  // It decides when the problem comes back for review.
  const GRADES = [
    { value: 1, label: "Again", hint: "I needed the solution" },
    { value: 2, label: "Hard", hint: "A real struggle" },
    { value: 3, label: "Good", hint: "Solved it, with some thought" },
    { value: 4, label: "Easy", hint: "Effortless" },
  ];
  // The reviews whose grade the user has set themselves, and a failure to save one.
  let graded = $state<Record<string, boolean>>({});
  let gradeError = $state<string | null>(null);

  async function pickGrade(reviewId: string, value: number) {
    gradeError = null;
    try {
      await leetcode.setGrade(problem.id, value);
      graded[reviewId] = true;
    } catch (e) {
      gradeError = e instanceof Error ? e.message : String(e);
    }
  }

  // ---- The celebration for an Accepted submission --------------------------------
  // A burst of confetti and a checkmark that draws itself, centered on the whole editor pane (the
  // overlay is positioned against that pane, not this panel, so opening or collapsing the console
  // doesn't move it). It plays once per accepted submission (the store holds the flag until it
  // has run its course).
  const CONFETTI_COLORS = ["#e9bc22", "#f2c93a", "#3fae7f", "#62c79a", "#9db4e8", "#eba337", "#edeae3"];
  const CELEBRATION_MS = 2600;

  interface Confetto {
    dx: number;
    dy: number;
    fall: number;
    spin: number;
    delay: number;
    size: number;
    color: string;
    round: boolean;
  }

  function makeConfetti(): Confetto[] {
    const between = (lo: number, hi: number) => lo + Math.random() * (hi - lo);
    return Array.from({ length: 44 }, () => {
      // Fired outward in every direction, then pulled down.
      const angle = (between(0, 360) * Math.PI) / 180;
      const power = between(110, 330);
      return {
        dx: -Math.cos(angle) * power,
        dy: -Math.sin(angle) * power,
        fall: between(120, 260),
        spin: between(-540, 540),
        delay: between(0, 0.12),
        size: between(6, 11),
        color: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
        round: Math.random() < 0.3,
      };
    });
  }

  let confetti = $state<Confetto[]>([]);
  $effect(() => {
    if (!judge.celebrating) return;
    const id = problem.id;
    confetti = makeConfetti();
    const timer = setTimeout(() => leetcode.endCelebration(id), CELEBRATION_MS);
    return () => clearTimeout(timer);
  });

  // ---- Formatting -------------------------------------------------------------
  const isAccepted = (status: string) => status === "Accepted";
  const runtimeText = (ms: number | null) => (ms === null ? "—" : `${ms} ms`);
  const memoryText = (kb: number | null) => (kb === null ? "—" : `${(kb / 1024).toFixed(1)} MB`);
  const percentText = (p: number | null) => (p === null ? "" : `Beats ${p.toFixed(2)}%`);

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
  }
  const exactTime = (ms: number) => new Date(ms).toLocaleString();
</script>

{#snippet failure(sub: Submission)}
  {#if sub.errorMessage}
    <pre class="block err">{sub.errorMessage}</pre>
  {/if}
  {#if sub.lastTestcase}
    <div class="field">
      <span class="label-row">
        <span class="label">Last executed input</span>
        {#if canAdd(sub.lastTestcase)}
          <button type="button" class="link-btn" onclick={() => sub.lastTestcase && leetcode.addFailingInput(problem, sub.lastTestcase)}>Add to testcases</button>
        {/if}
      </span>
      <pre class="block">{sub.lastTestcase}</pre>
    </div>
    {#if sub.codeOutput !== null}
      <div class="field">
        <span class="label">Your output</span>
        <pre class="block bad">{sub.codeOutput}</pre>
      </div>
    {/if}
    {#if sub.expectedOutput !== null}
      <div class="field">
        <span class="label">Expected</span>
        <pre class="block good">{sub.expectedOutput}</pre>
      </div>
    {/if}
  {/if}
{/snippet}

{#snippet submittedCode(sub: Submission)}
  <div class="field">
    <span class="label">Your code · {languageLabel(sub.lang)}</span>
    <SubmittedCode code={sub.code} lang={sub.lang} />
  </div>
{/snippet}

{#snippet stats(sub: Submission)}
  <div class="stats">
    <div class="stat">
      <span class="label">Runtime</span>
      <span class="value">{runtimeText(sub.runtimeMs)}</span>
      <span class="beats">{percentText(sub.runtimePercentile)}</span>
    </div>
    <div class="stat">
      <span class="label">Memory</span>
      <span class="value">{memoryText(sub.memoryKb)}</span>
      <span class="beats">{percentText(sub.memoryPercentile)}</span>
    </div>
  </div>
{/snippet}

<div class="judge">
  {#if judge.celebrating}
    <div class="celebrate" aria-hidden="true">
      <span class="ring"></span>
      <span class="ring late"></span>
      <span class="badge">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
          <path d="m5 12.5 4.5 4.5L19 7.5" pathLength="1" />
        </svg>
      </span>
      <span class="burst">
        {#each confetti as c, i (i)}
          <span
            class="bit"
            class:round={c.round}
            style="--dx:{c.dx.toFixed(0)};--dy:{c.dy.toFixed(0)};--fall:{c.fall.toFixed(0)};--spin:{c.spin.toFixed(0)}deg;--delay:{c.delay.toFixed(2)}s;--size:{c.size.toFixed(1)};--color:{c.color}"
          ></span>
        {/each}
      </span>
    </div>
  {/if}

  {#if judge.open}
    <section class="dock" aria-label="Results">
      <div class="tabs" role="tablist" aria-label="Console">
        <button
          type="button"
          role="tab"
          class="tab"
          aria-selected={judge.tab === "testcase"}
          onclick={() => leetcode.setTab(problem.id, "testcase")}>Testcase</button
        >
        <button
          type="button"
          role="tab"
          class="tab"
          aria-selected={judge.tab === "result"}
          onclick={() => leetcode.setTab(problem.id, "result")}>Result</button
        >
        <button
          type="button"
          role="tab"
          class="tab"
          aria-selected={judge.tab === "history"}
          onclick={() => leetcode.setTab(problem.id, "history")}
          >Submissions{#if judge.history && judge.history.length > 0}<span class="count">{judge.history.length}</span>{/if}</button
        >
      </div>

      <!-- Not a live region on the Testcase tab, where every keystroke would be announced. -->
      <div class="body" role="tabpanel" aria-live={judge.tab === "testcase" ? "off" : "polite"}>
        {#if judge.tab === "testcase"}
          <div class="tc-bar">
            <div class="tc-pills" role="group" aria-label="Test cases">
              {#each cases as _, i (i)}
                <span class="tc-pill" class:on={i === selected}>
                  <button type="button" class="tc-select" aria-pressed={i === selected} onclick={() => leetcode.selectCase(problem.id, i)}>Case {i + 1}</button>
                  {#if cases.length > 1}
                    <button type="button" class="tc-remove" aria-label="Remove case {i + 1}" onclick={() => leetcode.removeCase(problem, i)}>
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true"><path d="M18 6 6 18" /><path d="m6 6 12 12" /></svg>
                    </button>
                  {/if}
                </span>
              {/each}
              {#if !layout.freeform}
                <button type="button" class="tc-add" aria-label="Add a test case" onclick={addCase}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14" /><path d="M5 12h14" /></svg>
                </button>
              {/if}
            </div>
            {#if edited}
              <button type="button" class="link-btn" onclick={() => leetcode.resetCases(problem.id)}>Reset to examples</button>
            {/if}
          </div>
          {#if layout.freeform}
            <p class="tc-note">This problem's input can't be split into separate arguments, so it is one block of text: a line per argument.</p>
          {/if}
          <div bind:this={fieldsEl}>
            {#each layout.params as name, p (name + p)}
              <label class="tc-field">
                <span class="label">{layout.freeform ? "Input" : `${name} =`}</span>
                <textarea
                  class="tc-input"
                  class:empty={(cases[selected]?.[p] ?? "").trim() === ""}
                  rows="1"
                  spellcheck="false"
                  autocomplete="off"
                  value={cases[selected]?.[p] ?? ""}
                  oninput={(e) => leetcode.editCase(problem, selected, p, e.currentTarget.value)}
                ></textarea>
              </label>
            {/each}
          </div>
        {:else if judge.tab === "result"}
          {#if judge.busy}
            <p class="busy"><span class="spinner" aria-hidden="true"></span>{judge.busy === "run" ? "Running your code…" : "Waiting for LeetCode to judge your submission…"}</p>
          {:else if !judge.outcome}
            <p class="hint">
              <strong>Run</strong> checks your code against the example cases. <strong>Submit</strong> has LeetCode judge it against every test and
              adds it to your history.
            </p>
          {:else if judge.outcome.kind === "error"}
            <p class="problem" role="alert">{judge.outcome.error.message}</p>
            {#if judge.outcome.error.needsSignIn}
              <button type="button" class="link-btn" disabled={leetcode.signingIn} onclick={() => leetcode.signIn()}>Sign in to LeetCode</button>
            {/if}
          {:else if judge.outcome.kind === "run"}
            {@const result = judge.outcome.result}
            <div class="head">
              <span class="verdict" class:ok={result.correct}>{result.status}</span>
              <span class="meta">Run · {languageLabel(judge.outcome.lang)}{result.runtimeMs !== null ? ` · ${result.runtimeMs} ms` : ""}</span>
            </div>
            {#if result.error}
              <pre class="block err">{result.error}</pre>
            {/if}
            {#if result.cases.length > 0}
              {@const current = shownCase(result)}
              <div class="cases" role="group" aria-label="Test cases">
                {#each result.cases as c, i (i)}
                  <button type="button" class="case" class:on={i === current} aria-pressed={i === current} onclick={() => (picked = i)}>
                    <span class="dot" class:ok={c.passed} aria-hidden="true"></span>Case {i + 1}
                  </button>
                {/each}
              </div>
              {@const shown = result.cases[current]}
              <div class="field">
                <span class="label">Input</span>
                <pre class="block">{shown.input}</pre>
              </div>
              <div class="field">
                <span class="label">Output</span>
                <pre class="block" class:bad={!shown.passed}>{shown.output}</pre>
              </div>
              <div class="field">
                <span class="label">Expected</span>
                <pre class="block good">{shown.expected}</pre>
              </div>
              {#if shown.stdout}
                <div class="field">
                  <span class="label">Stdout</span>
                  <pre class="block">{shown.stdout}</pre>
                </div>
              {/if}
            {:else}
              <!-- The code didn't run to completion, so there are no per-case results. LeetCode may name
                   the one case it stopped on; if it doesn't, nothing is claimed. -->
              {#if result.lastInput}
                <div class="field">
                  <span class="label">Last executed input</span>
                  <pre class="block">{result.lastInput}</pre>
                </div>
              {/if}
              {#if !result.error && !result.lastInput}
                <p class="hint">LeetCode didn't report any test cases for this run.</p>
              {/if}
            {/if}
          {:else}
            {@const sub = judge.outcome.submission}
            <div class="head">
              <span class="verdict" class:ok={isAccepted(sub.status)}>{sub.status}</span>
              <span class="meta">Submitted · {languageLabel(sub.lang)}</span>
              {#if settings.coachEnabled}
                <!-- Opens the coach's drawer and has it start on this submission. -->
                <button type="button" class="coach-link" onclick={() => coach.request(problem.id, isAccepted(sub.status) ? "analyze" : "debug")}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
                  </svg>
                  {isAccepted(sub.status) ? `Analyze with ${COACH_NAME}` : `Debug with ${COACH_NAME}`}
                </button>
              {/if}
            </div>
            {#if isAccepted(sub.status)}
              {#if sub.totalTestcases !== null}
                <p class="line">{sub.totalCorrect ?? sub.totalTestcases} / {sub.totalTestcases} test cases passed</p>
              {/if}
              {#if sub.reviewId}
                {@const reviewId = sub.reviewId}
                <div class="grade" role="group" aria-label="How did it go?">
                  <span class="label">How did it go?</span>
                  <div class="grade-row">
                    {#each GRADES as g (g.value)}
                      <button type="button" class="grade-btn" aria-pressed={sub.reviewGrade === g.value} onclick={() => pickGrade(reviewId, g.value)}>
                        {g.label}
                      </button>
                    {/each}
                  </div>
                  <p class="grade-note" class:problem={gradeError !== null}>
                    {#if gradeError}
                      Couldn't save that: {gradeError}
                    {:else if graded[reviewId]}
                      Saved. {GRADES.find((g) => g.value === sub.reviewGrade)?.hint}.
                    {:else}
                      Guessed from your attempts. Change it if that's off: it decides when this comes back for review.
                    {/if}
                  </p>
                </div>
              {/if}
              {@render stats(sub)}
            {:else}
              {@render failure(sub)}
            {/if}
            {@render submittedCode(sub)}
          {/if}
        {:else if judge.historyError}
          <p class="problem" role="alert">Couldn't load your submissions: {judge.historyError}</p>
        {:else if judge.history === null}
          <p class="hint">Loading…</p>
        {:else if judge.history.length === 0}
          <p class="hint">You haven't submitted this problem yet.</p>
        {:else}
          <ul class="history">
            {#each judge.history as sub (sub.id)}
              <li>
                <button
                  type="button"
                  class="row"
                  aria-expanded={expanded === sub.id}
                  onclick={() => (expanded = expanded === sub.id ? null : sub.id)}
                >
                  <span class="status" class:ok={isAccepted(sub.status)}>{sub.status}</span>
                  <span class="cell">{languageLabel(sub.lang)}</span>
                  <span class="cell">{runtimeText(sub.runtimeMs)}</span>
                  <span class="cell">{memoryText(sub.memoryKb)}</span>
                  <time class="when" datetime={new Date(sub.submittedAt).toISOString()} title={exactTime(sub.submittedAt)}>{timeAgo(sub.submittedAt)}</time>
                </button>
                {#if expanded === sub.id}
                  <div class="detail">
                    {#if isAccepted(sub.status)}
                      {#if settings.coachEnabled}
                        <!-- Starts a new conversation about this submission. -->
                        <button type="button" class="coach-link" onclick={() => coach.analyzeSubmission(contextFor(problem, lang, getCode), sub)}>
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                            <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
                          </svg>
                          Analyze with {COACH_NAME}
                        </button>
                      {/if}
                      {@render stats(sub)}
                    {:else}
                      {@render failure(sub)}
                    {/if}
                    {@render submittedCode(sub)}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </section>
  {/if}

  <div class="bar">
    <button
      type="button"
      class="toggle"
      aria-expanded={judge.open}
      aria-label={judge.open ? "Hide console" : "Show console"}
      onclick={() => leetcode.toggle(problem.id)}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class:flip={judge.open}>
        <path d="m18 15-6-6-6 6" />
      </svg>
      Console
      {#if judge.outcome && !judge.open}
        <span class="dot" class:ok={judge.outcome.kind === "run" ? judge.outcome.result.correct : judge.outcome.kind === "submit" && isAccepted(judge.outcome.submission.status)} aria-hidden="true"></span>
      {/if}
    </button>

    <div class="account">
      {#if leetcode.status === null}
        <!-- Still reading whether there is a login; show nothing rather than flash "Sign in". -->
      {:else if leetcode.signingIn}
        <span class="who muted" role="status">Waiting for you to sign in…</span>
      {:else if leetcode.expired}
        <span class="who warn">Session expired</span>
        <button type="button" class="link-btn strong" onclick={() => leetcode.signIn()}>Sign in again</button>
      {:else if leetcode.status.signedIn}
        <!-- Just a dot while all is well. It says who's signed in, and opens the account in Settings (to sign out). -->
        <span class="tip left has-tip" data-tip={leetcode.status.username ? `Signed in as ${leetcode.status.username}` : "Signed in to LeetCode"}>
          <button
            type="button"
            class="account-dot"
            aria-label={leetcode.status.username ? `Signed in to LeetCode as ${leetcode.status.username}. Open account settings` : "Signed in to LeetCode. Open account settings"}
            onclick={() => settingsDialog.open("leetcode")}
          >
            <span class="dot ok" aria-hidden="true"></span>
          </button>
        </span>
      {:else}
        <button type="button" class="link-btn strong" onclick={() => leetcode.signIn()}>Sign in to run code</button>
      {/if}
      {#if leetcode.authError}
        <span class="auth-error" role="alert">{leetcode.authError}</span>
      {/if}
    </div>

    <div class="actions">
      <!-- With the coach open the editor is narrow, and the coach's own footer has this same button,
           so the bar leaves it out rather than wrapping onto a second row. -->
      {#if !coachOpen}
        <GiveUpButton {problem} {lang} {getCode} variant="bar" />
      {/if}
      <span class="tip" data-tip={runTip} class:has-tip={runTip !== ""}>
        <button type="button" class="btn run" disabled={!canJudge || blank} onclick={() => leetcode.run(problem, lang, getCode)}>
          {#if judge.busy === "run"}
            <span class="spinner" aria-hidden="true"></span>
          {:else}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15a1 1 0 0 0 1.5.86l12.5-7.5a1 1 0 0 0 0-1.72L8.5 3.64A1 1 0 0 0 7 4.5Z" /></svg>
          {/if}
          Run
        </button>
      </span>
      <span class="tip" data-tip={judgeTip} class:has-tip={judgeTip !== ""}>
        <button type="button" class="btn submit" disabled={!canJudge} onclick={() => leetcode.submit(problem.id, lang, getCode)}>
          {#if judge.busy === "submit"}
            <span class="spinner" aria-hidden="true"></span>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5" /><path d="m5 12 7-7 7 7" /></svg>
          {/if}
          Submit
        </button>
      </span>
    </div>
  </div>
</div>

<style>
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* ---- How did it go? ---- */
  .grade {
    margin-top: calc(10 * var(--u));
  }

  .grade-row {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
    margin-top: calc(6 * var(--u));
  }

  .grade-btn {
    padding: calc(4 * var(--u)) calc(14 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #a8a296;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s,
      color 0.15s;
  }

  .grade-btn:hover {
    color: #edeae3;
  }

  .grade-btn[aria-pressed="true"] {
    border-color: rgba(233, 188, 34, 0.6);
    background-color: rgba(233, 188, 34, 0.12);
    color: #edeae3;
  }

  .grade-btn:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .grade-note {
    margin: calc(6 * var(--u)) 0 0;
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #8d877b;
  }

  /* ---- Accepted celebration ---- */
  .celebrate {
    position: absolute;
    inset: 0;
    z-index: 5;
    pointer-events: none;
  }

  .badge {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    display: grid;
    place-items: center;
    width: calc(76 * var(--u));
    height: calc(76 * var(--u));
    border-radius: 50%;
    background-color: #3fae7f;
    box-shadow:
      0 0 calc(34 * var(--u)) rgba(63, 174, 127, 0.55),
      0 calc(6 * var(--u)) calc(18 * var(--u)) rgba(0, 0, 0, 0.35);
    color: #0d2a1f;
    animation: pop 2.1s cubic-bezier(0.2, 0.9, 0.25, 1.2) both;
  }

  .badge svg {
    width: 56%;
    height: 56%;
  }

  .badge path {
    stroke-dasharray: 1;
    animation: draw 0.45s 0.3s ease-out both;
  }

  .ring {
    position: absolute;
    top: 50%;
    left: 50%;
    box-sizing: border-box;
    width: calc(76 * var(--u));
    height: calc(76 * var(--u));
    border: 2px solid #3fae7f;
    border-radius: 50%;
    animation: ring 1.3s ease-out both;
  }

  .ring.late {
    border-color: #e9bc22;
    animation-delay: 0.22s;
  }

  /* Confetti leaves from the middle of the pane, where the badge is. */
  .burst {
    position: absolute;
    top: 50%;
    left: 50%;
  }

  .bit {
    position: absolute;
    width: calc(var(--size) * var(--u));
    height: calc(var(--size) * 0.55 * var(--u));
    border-radius: calc(1.5 * var(--u));
    background-color: var(--color);
    opacity: 0;
    animation: confetti 2.2s var(--delay) cubic-bezier(0.15, 0.7, 0.3, 1) both;
  }

  .bit.round {
    height: calc(var(--size) * var(--u));
    border-radius: 50%;
  }

  @keyframes pop {
    0% {
      opacity: 0;
      transform: translate(-50%, -50%) scale(0.2);
    }
    22% {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1.12);
    }
    34% {
      transform: translate(-50%, -50%) scale(1);
    }
    78% {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
    100% {
      opacity: 0;
      transform: translate(-50%, -62%) scale(0.92);
    }
  }

  @keyframes draw {
    from {
      stroke-dashoffset: 1;
    }
    to {
      stroke-dashoffset: 0;
    }
  }

  @keyframes ring {
    from {
      opacity: 0.8;
      transform: translate(-50%, -50%) scale(0.5);
    }
    to {
      opacity: 0;
      transform: translate(-50%, -50%) scale(3);
    }
  }

  /* Out to (dx, dy) fast, then drifting down and fading as it tumbles. */
  @keyframes confetti {
    0% {
      opacity: 1;
      transform: translate(0, 0) rotate(0) scale(0.5);
    }
    38% {
      opacity: 1;
      transform: translate(calc(var(--dx) * var(--u)), calc(var(--dy) * var(--u))) rotate(calc(var(--spin) * 0.5)) scale(1);
    }
    100% {
      opacity: 0;
      transform: translate(calc(var(--dx) * 1.12 * var(--u)), calc((var(--dy) + var(--fall)) * var(--u))) rotate(var(--spin)) scale(0.9);
    }
  }

  .judge {
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: 60%;
    border-top: 1px solid var(--card-border);
  }

  /* ---- The results panel ---- */
  .dock {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    height: calc(260 * var(--u));
    border-bottom: 1px solid var(--card-border);
    background-color: #1d2027;
  }

  .tabs {
    flex: none;
    display: flex;
    gap: calc(4 * var(--u));
    padding: calc(6 * var(--u)) calc(10 * var(--u)) 0;
    border-bottom: 1px solid var(--card-border);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    margin-bottom: -1px;
    padding: calc(6 * var(--u)) calc(10 * var(--u));
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    font: inherit;
    font-size: calc(12.5 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #a8a296;
    cursor: pointer;
    transition: color 0.15s;
  }

  .tab:hover {
    color: #edeae3;
  }

  .tab[aria-selected="true"] {
    border-bottom-color: #e9bc22;
    color: #edeae3;
  }

  .count {
    padding: 0 calc(6 * var(--u));
    border-radius: calc(999 * var(--u));
    background-color: rgba(255, 255, 255, 0.08);
    font-size: calc(11 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #c9c2b4;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: calc(14 * var(--u)) calc(16 * var(--u)) calc(16 * var(--u));
    font-size: calc(13 * var(--u));
    line-height: calc(20 * var(--u));
    color: #c9c2b4;
  }

  .hint,
  .busy,
  .line,
  .problem {
    margin: 0;
  }

  .hint strong {
    font-weight: 600;
    color: #edeae3;
  }

  .busy {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
  }

  .problem {
    color: #ff8a8a;
  }

  .head {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: calc(10 * var(--u));
    margin-bottom: calc(10 * var(--u));
  }

  .verdict {
    font-size: calc(17 * var(--u));
    font-weight: 600;
    line-height: calc(24 * var(--u));
    color: #ff8a8a;
  }

  .verdict.ok {
    color: #3fae7f;
  }

  .meta {
    font-size: calc(12 * var(--u));
    color: #a8a296;
  }

  /* Opens the AI coach on this submission, to the right of the verdict. */
  .coach-link {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    margin-left: auto;
    align-self: center;
    padding: calc(3 * var(--u)) calc(11 * var(--u)) calc(3 * var(--u)) calc(9 * var(--u));
    border: 1px solid var(--ai-border);
    border-radius: calc(999 * var(--u));
    background-color: var(--ai-tint);
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s;
  }

  .coach-link svg {
    width: calc(13 * var(--u));
    height: calc(13 * var(--u));
    stroke: url(#ai-gradient);
  }

  .coach-link:hover {
    border-color: var(--ai-border-strong);
    background-color: var(--ai-tint-strong);
  }

  .coach-link:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: calc(10 * var(--u));
    margin-top: calc(10 * var(--u));
  }

  .stat {
    display: flex;
    flex-direction: column;
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(10 * var(--u));
    background-color: #181b21;
  }

  .stat .value {
    font-size: calc(18 * var(--u));
    font-weight: 600;
    line-height: calc(26 * var(--u));
    color: #edeae3;
  }

  .beats {
    font-size: calc(12 * var(--u));
    color: #3fae7f;
  }

  .label {
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: #a8a296;
  }

  .field {
    margin-top: calc(10 * var(--u));
  }

  .label-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    font-size: calc(12 * var(--u));
  }

  /* ---- Testcase tab ---- */
  .tc-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    font-size: calc(12 * var(--u));
  }

  .tc-pills {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: calc(6 * var(--u));
  }

  .tc-pill {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    transition: border-color 0.2s;
  }

  .tc-pill.on {
    border-color: rgba(233, 188, 34, 0.5);
  }

  .tc-select {
    padding: calc(4 * var(--u)) calc(11 * var(--u));
    border: 0;
    border-radius: calc(999 * var(--u));
    background: none;
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #a8a296;
    cursor: pointer;
    transition: color 0.15s;
  }

  .tc-pill:hover .tc-select,
  .tc-pill.on .tc-select {
    color: #edeae3;
  }

  /* The remove cross sits inside the pill, to the right of its name. */
  .tc-remove {
    display: grid;
    place-items: center;
    width: calc(18 * var(--u));
    height: calc(18 * var(--u));
    margin-right: calc(5 * var(--u));
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: #8d877b;
    cursor: pointer;
    transition:
      background-color 0.15s,
      color 0.15s;
  }

  .tc-remove svg {
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
  }

  .tc-remove:hover {
    background-color: rgba(255, 85, 85, 0.16);
    color: #ff8a8a;
  }

  .tc-add {
    display: grid;
    place-items: center;
    width: calc(26 * var(--u));
    height: calc(26 * var(--u));
    padding: 0;
    border: 1px solid var(--card-border);
    border-radius: 50%;
    background-color: #181b21;
    color: #a8a296;
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .tc-add svg {
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
  }

  .tc-add:hover {
    border-color: rgba(233, 188, 34, 0.5);
    color: #edeae3;
  }

  .tc-note {
    margin: calc(10 * var(--u)) 0 0;
    font-size: calc(12 * var(--u));
    color: #a8a296;
  }

  .tc-field {
    display: block;
    margin-top: calc(10 * var(--u));
  }

  /* Argument names are code (and case-sensitive), so unlike the other labels they're shown as written. */
  .tc-field .label {
    font-family: var(--font-code);
    font-weight: 500;
    letter-spacing: 0;
    text-transform: none;
  }

  .tc-input {
    display: block;
    box-sizing: border-box;
    width: 100%;
    min-height: calc(36 * var(--u));
    max-height: calc(160 * var(--u));
    margin-top: calc(4 * var(--u));
    padding: calc(8 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #181b21;
    font-family: var(--font-code);
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    resize: none;
    /* Grows with its text, up to the cap above. */
    field-sizing: content;
    overflow-wrap: anywhere;
    transition: border-color 0.2s;
  }

  .tc-input:hover {
    border-color: rgba(255, 255, 255, 0.14);
  }

  .tc-input:focus-visible {
    outline: none;
    border-color: rgba(233, 188, 34, 0.6);
  }

  .tc-input.empty {
    border-color: rgba(255, 138, 138, 0.45);
  }

  .block {
    margin: calc(4 * var(--u)) 0 0;
    padding: calc(8 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #181b21;
    font-family: var(--font-code);
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .block.good {
    color: #5fd0a0;
  }

  .block.bad,
  .block.err {
    color: #ff9c9c;
  }

  .cases {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
    margin-top: calc(4 * var(--u));
  }

  .case {
    display: inline-flex;
    align-items: center;
    gap: calc(7 * var(--u));
    padding: calc(4 * var(--u)) calc(11 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #a8a296;
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .case:hover {
    color: #edeae3;
  }

  .case.on {
    border-color: rgba(233, 188, 34, 0.5);
    color: #edeae3;
  }

  .dot {
    flex: none;
    width: calc(7 * var(--u));
    height: calc(7 * var(--u));
    border-radius: 50%;
    background-color: #ff6b6b;
  }

  .dot.ok {
    background-color: #3fae7f;
  }

  /* ---- Submission history ---- */
  .history {
    margin: calc(-6 * var(--u)) calc(-8 * var(--u)) 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 0.8fr) minmax(0, 0.8fr) minmax(0, 1fr);
    align-items: center;
    gap: calc(10 * var(--u));
    width: 100%;
    box-sizing: border-box;
    padding: calc(8 * var(--u)) calc(8 * var(--u));
    border: 0;
    border-radius: calc(8 * var(--u));
    background: none;
    font: inherit;
    font-size: calc(12.5 * var(--u));
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    text-align: left;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .row:hover {
    background-color: rgba(255, 255, 255, 0.04);
  }

  .status {
    font-weight: 600;
    color: #ff8a8a;
  }

  .status.ok {
    color: #3fae7f;
  }

  .cell {
    font-variant-numeric: tabular-nums;
  }

  .when {
    justify-self: end;
    color: #a8a296;
    white-space: nowrap;
  }

  .detail {
    padding: 0 calc(8 * var(--u)) calc(10 * var(--u));
  }

  /* The analyze button sits at the left, above the stats, rather than at a header's right edge. */
  .detail > .coach-link {
    margin-left: 0;
  }

  /* ---- The bar under the editor ---- */
  /* One row when the editor is wide: Console, the account (taking what's left), then the buttons.
     When it's narrow (the AI coach open beside it) the account would be squeezed to nothing, so the
     bar wraps instead: the buttons drop to a row of their own, to the right under the account. The
     account's `flex-basis` is how much room it needs before the buttons have to wrap. */
  .bar {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: calc(6 * var(--u)) calc(10 * var(--u));
    padding: calc(8 * var(--u)) calc(14 * var(--u));
  }

  .actions {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    margin-left: auto;
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(5 * var(--u)) calc(10 * var(--u)) calc(5 * var(--u)) calc(7 * var(--u));
    border: 0;
    border-radius: calc(8 * var(--u));
    background: none;
    font: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 500;
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    cursor: pointer;
    transition:
      background-color 0.15s,
      color 0.15s;
  }

  .toggle:hover {
    background-color: rgba(255, 255, 255, 0.05);
    color: #edeae3;
  }

  .toggle svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    transition: transform 0.2s;
  }

  .toggle svg.flip {
    transform: rotate(180deg);
  }

  .account {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-wrap: wrap; /* a long error message goes under the name, not off the edge */
    align-items: center;
    gap: calc(2 * var(--u)) calc(8 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #a8a296;
  }

  .who {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #c9c2b4;
  }

  .who.muted {
    color: #a8a296;
  }

  .who.warn {
    color: #e9bc22;
  }

  /* The green dot, with a roomier button around it. */
  .account-dot {
    display: grid;
    place-items: center;
    width: calc(22 * var(--u));
    height: calc(22 * var(--u));
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .account-dot:hover {
    background-color: rgba(255, 255, 255, 0.07);
  }

  .account-dot:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 1px;
  }

  .auth-error {
    color: #ff8a8a;
  }

  .link-btn {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: #a8a296;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .link-btn.strong {
    color: #e9bc22;
    text-decoration: none;
    font-weight: 500;
  }

  .link-btn:hover:not(:disabled) {
    color: #edeae3;
  }

  .link-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: calc(7 * var(--u));
    box-sizing: border-box;
    min-width: calc(92 * var(--u));
    justify-content: center;
    padding: calc(7 * var(--u)) calc(16 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(9 * var(--u));
    font: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 600;
    line-height: calc(18 * var(--u));
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s,
      opacity 0.2s;
  }

  .btn svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
  }

  .btn.run {
    background-color: #1d2027;
    color: #edeae3;
  }

  .btn.run:hover:not(:disabled) {
    border-color: rgba(255, 255, 255, 0.2);
  }

  .btn.submit {
    border-color: transparent;
    background-color: #e9bc22;
    color: #1b1608;
  }

  .btn.submit:hover:not(:disabled) {
    background-color: #f2c93a;
  }

  .btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .toggle:focus-visible,
  .btn:focus-visible,
  .tab:focus-visible,
  .case:focus-visible,
  .tc-select:focus-visible,
  .tc-remove:focus-visible,
  .tc-add:focus-visible,
  .row:focus-visible,
  .link-btn:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .spinner {
    flex: none;
    box-sizing: border-box;
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Custom tooltip (like the rest of the app), shown above since the bar is at the bottom. */
  .tip {
    position: relative;
    display: inline-flex;
  }

  .tip.has-tip::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 6;
    bottom: calc(100% + 8 * var(--u));
    right: 0;
    padding: calc(5 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(8 * var(--u));
    background-color: #12141a;
    box-shadow: 0 calc(6 * var(--u)) calc(16 * var(--u)) rgba(0, 0, 0, 0.35);
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transform: translateY(calc(4 * var(--u)));
    transition:
      opacity 0.15s,
      transform 0.15s;
  }

  /* This one is at the left of the bar, so its tooltip opens rightwards from it. */
  .tip.left.has-tip::after {
    right: auto;
    left: 0;
  }

  .tip.has-tip:hover::after,
  .tip.has-tip:has(:focus-visible)::after {
    opacity: 1;
    transform: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2.4s;
    }

    .toggle svg,
    .tip.has-tip::after {
      transition: none;
    }

    /* No flying confetti or expanding rings: the badge just fades in and out. */
    .burst,
    .ring {
      display: none;
    }

    .badge {
      animation: fade 2.1s both;
    }

    .badge path {
      animation: none;
    }

    @keyframes fade {
      0%,
      100% {
        opacity: 0;
      }
      15%,
      80% {
        opacity: 1;
      }
    }
  }
</style>
