<script lang="ts">
  // Under the editor: the LeetCode account, the Run and Submit buttons, and a console that opens
  // above them with three tabs: the test cases Run will use (the problem's examples, which can be
  // edited and added to), what the last Run or Submit came to, and the problem's submission history.
  import { tick } from "svelte";
  import type { ProblemDetail, Submission } from "$lib/db";
  import { languageLabel } from "$lib/languages";
  import { layoutOf, leetcode } from "$lib/leetcode.svelte";
  import type { RunResult } from "$lib/leetcode";
  import { caseFromInput, hasBlank } from "$lib/testcases";

  let { problem, lang, getCode }: { problem: ProblemDetail; lang: string; getCode: () => string } = $props();

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
            </div>
            {#if isAccepted(sub.status)}
              {#if sub.totalTestcases !== null}
                <p class="line">{sub.totalCorrect ?? sub.totalTestcases} / {sub.totalTestcases} test cases passed</p>
              {/if}
              {@render stats(sub)}
            {:else}
              {@render failure(sub)}
            {/if}
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
                      {@render stats(sub)}
                    {:else}
                      {@render failure(sub)}
                    {/if}
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
        <button type="button" class="link-btn" onclick={() => leetcode.signIn()}>Sign in again</button>
      {:else if leetcode.status.signedIn}
        <span class="who"><span class="dot ok" aria-hidden="true"></span>{leetcode.status.username || "Signed in"}</span>
        <button type="button" class="link-btn" onclick={() => leetcode.signOut()}>Sign out</button>
      {:else}
        <button type="button" class="link-btn strong" onclick={() => leetcode.signIn()}>Sign in to LeetCode</button>
      {/if}
      {#if leetcode.authError}
        <span class="auth-error" role="alert">{leetcode.authError}</span>
      {/if}
    </div>

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

<style>
  @keyframes spin {
    to {
      transform: rotate(360deg);
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
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
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
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
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
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
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

  /* ---- The bar under the editor ---- */
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    padding: calc(8 * var(--u)) calc(14 * var(--u));
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
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
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

  .tip.has-tip:hover::after {
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
  }
</style>
