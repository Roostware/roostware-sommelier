<script lang="ts">
  // The Solve tab while a problem is open, laid out like LeetCode: the problem on the left
  // half and a code editor on the right half.
  import { onMount, tick } from "svelte";
  import { coach } from "$lib/coach.svelte";
  import { COACH_NAME } from "$lib/coachName";
  import CoachPanel from "$lib/CoachPanel.svelte";
  import CodeEditor from "$lib/CodeEditor.svelte";
  import Dropdown from "$lib/Dropdown.svelte";
  import JudgePanel from "$lib/JudgePanel.svelte";
  import { defaultLanguage, languageIcon, languageLabel, monacoLanguage, sortLanguages } from "$lib/languages";
  import { leetcode } from "$lib/leetcode.svelte";
  import ProblemStatement from "$lib/ProblemStatement.svelte";
  import { settings } from "$lib/settings.svelte";
  import { resetIfSolved } from "$lib/sittingEnd";
  import { closeProblem, solve } from "$lib/solve.svelte";

  onMount(() => {
    if (leetcode.status === null) void leetcode.load();
    // Coming back to the tab: pick up anything that changed since (e.g. new submissions).
    if (solve.problem && !solve.loading) void solve.refresh();
  });

  // Going back to the picker ends this sitting, so a solved problem is reset like on leaving the tab.
  function chooseAnother() {
    resetIfSolved();
    closeProblem();
  }

  let problem = $derived(solve.problem);
  let languages = $derived(problem ? sortLanguages(problem.snippets.map((s) => s.lang)) : []);
  let languageOptions = $derived(languages.map((slug) => ({ value: slug, label: languageLabel(slug), icon: languageIcon(slug) })));
  // The store settles the language when the problem loads (preferring one with unsubmitted
  // code); the fallback only covers a language this problem doesn't have.
  let lang = $derived(
    problem
      ? solve.lang && languages.includes(solve.lang)
        ? solve.lang
        : defaultLanguage(languages, solve.preferredLang)
      : null,
  );
  let starter = $derived(problem?.snippets.find((s) => s.lang === lang)?.code ?? "");

  // The editor is rebuilt (rather than mutated) for each problem + language, and on reset.
  let resetCount = $state(0);
  let editorKey = $derived(problem && lang ? `${problem.id}:${lang}:${resetCount}` : null);

  // What's in the editor right now, to know whether there's anything to reset.
  let current = $state("");
  $effect(() => {
    // Keyed on the editor's identity, so a reset (which rebuilds the editor with the starter
    // code) re-syncs this too, not just a change of problem or language.
    if (editorKey === null || !problem || !lang) return;
    current = solve.codeFor(problem.id, lang) ?? starter;
  });
  let changed = $derived(current !== starter);

  // ---- The AI coach ----------------------------------------------------------
  // A third column on the right, closed until the user opens it (and absent if they've turned the
  // coach off in the settings). What it offers depends on the problem's submissions and whether
  // the user has given up, so those are loaded as soon as the problem is open, drawer or not:
  // the Give up button in the editor's bar needs them too.
  let showCoach = $derived(settings.coachEnabled && coach.open);

  $effect(() => {
    if (!problem) return;
    const id = problem.id;
    if (leetcode.stateFor(id).history === null) void leetcode.loadHistory(id);
    // A new submission can settle a give-up (solving it again), so read it again when one lands.
    void leetcode.stateFor(id).history?.[0]?.id;
    void coach.loadGiveUp(id);
    void coach.loadSaved(id);
  });

  /** The user's code as it stands: what Run and Submit send. */
  function currentCode(): string {
    return problem && lang ? (solve.codeFor(problem.id, lang) ?? starter) : "";
  }

  function onCodeChange(code: string) {
    current = code;
    if (problem && lang) solve.setCode(problem.id, lang, code);
  }

  // ---- Resetting the code ----------------------------------------------------
  // Resetting throws the user's code away, so the reset icon turns into a yes/no question.
  let confirmingReset = $state(false);
  let resetEl: HTMLButtonElement | undefined = $state();
  let noEl: HTMLButtonElement | undefined = $state();
  let resetAreaEl: HTMLElement | undefined = $state();

  async function askToReset() {
    confirmingReset = true;
    await tick();
    noEl?.focus(); // the safe answer is the one that's focused
  }

  async function cancelReset(refocus = true) {
    confirmingReset = false;
    if (refocus) {
      await tick();
      resetEl?.focus();
    }
  }

  function confirmReset() {
    if (!problem || !lang) return;
    confirmingReset = false;
    solve.resetCode(problem.id, lang);
    resetCount++; // rebuilds the editor with the starter code (and it takes focus)
  }

  function onConfirmKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      void cancelReset();
    }
  }

  // Clicking anywhere else means "never mind".
  function onWindowPointerDown(event: PointerEvent) {
    if (confirmingReset && resetAreaEl && !resetAreaEl.contains(event.target as Node)) {
      void cancelReset(false);
    }
  }

  // Switching language or problem also cancels a pending question.
  $effect(() => {
    void editorKey;
    confirmingReset = false;
  });
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<div class="workspace">
  <button type="button" class="back" onclick={chooseAnother}>
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="m15 18-6-6 6-6" />
    </svg>
    Choose another problem
  </button>

  {#if problem}
    <div class="panes" class:with-coach={showCoach}>
      <section class="pane card" aria-label="Problem description">
        <ProblemStatement {problem} />
      </section>

      <section class="pane card" aria-label="Code editor">
        {#if lang}
          <div class="toolbar">
            <Dropdown
              options={languageOptions}
              value={lang}
              onselect={(slug) => solve.selectLanguage(slug)}
              label="Programming language"
              width={170}
            />

            {#if solve.saveError}
              <span class="save-error" role="alert">Couldn't save your code</span>
            {/if}

            <div class="tools">
            {#if settings.coachEnabled}
              <span class="tip" data-tip={coach.open ? `Close ${COACH_NAME}` : `Open ${COACH_NAME}, your AI coach`}>
                <button
                  type="button"
                  class="coach-btn"
                  class:working={coach.thread(problem.id).busy}
                  aria-pressed={coach.open}
                  aria-label="Ask {COACH_NAME}, your AI coach"
                  onclick={() => coach.toggle()}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
                  </svg>
                  Ask {COACH_NAME}
                </button>
              </span>
            {/if}

            <!-- The reset icon and the yes/no question share this spot, right-aligned, so the
                 question grows out to the left of the buttons. Both stay in the page and
                 cross-fade (inert hides whichever is away from keyboard and screen readers). -->
            <div class="reset-area" bind:this={resetAreaEl}>
              <span class="tip" data-tip={changed ? "Reset code" : "No changes to reset"} inert={confirmingReset}>
                <button
                  type="button"
                  class="icon-btn reset"
                  class:away={confirmingReset}
                  aria-label="Reset code"
                  disabled={!changed}
                  bind:this={resetEl}
                  onclick={askToReset}
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                  >
                    <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                    <path d="M3 3v5h5" />
                  </svg>
                </button>
              </span>

              <div
                class="confirm"
                class:open={confirmingReset}
                role="group"
                aria-label="Reset code. Are you sure?"
                inert={!confirmingReset}
              >
                <span class="question"><span class="question-clip"><span class="question-text">Are you sure?</span></span></span>
                <span class="tip" data-tip="Yes, reset code">
                  <button
                    type="button"
                    class="icon-btn yes"
                    aria-label="Yes, reset code"
                    onclick={confirmReset}
                    onkeydown={onConfirmKeydown}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2.5"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M20 6 9 17l-5-5" />
                    </svg>
                  </button>
                </span>
                <span class="tip" data-tip="No, keep my code">
                  <button
                    type="button"
                    class="icon-btn no"
                    aria-label="No, keep my code"
                    bind:this={noEl}
                    onclick={() => cancelReset()}
                    onkeydown={onConfirmKeydown}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2.5"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M18 6 6 18" />
                      <path d="m6 6 12 12" />
                    </svg>
                  </button>
                </span>
              </div>
            </div>
            </div>
          </div>
          <div class="editor-area">
            {#key editorKey}
              <CodeEditor
                value={solve.codeFor(problem.id, lang) ?? starter}
                language={monacoLanguage(lang)}
                onchange={onCodeChange}
              />
            {/key}
          </div>
          <JudgePanel {problem} {lang} getCode={currentCode} />
        {:else}
          <p class="message">This problem has no starter code to edit.</p>
        {/if}
      </section>

      {#if showCoach && lang}
        <section class="pane card coach-pane" aria-label="{COACH_NAME}, the AI coach">
          <CoachPanel {problem} {lang} getCode={currentCode} onclose={() => (coach.open = false)} />
        </section>
      {/if}
    </div>
  {:else if solve.error}
    <p class="message" role="alert">Couldn't load this problem: {solve.error}</p>
  {:else}
    <p class="message" role="status">Loading…</p>
  {/if}
</div>

<style>
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(calc(14 * var(--u)));
    }
  }

  /* Fills the page area, so each half scrolls inside itself rather than the whole page. */
  .workspace {
    display: flex;
    flex-direction: column;
    gap: calc(16 * var(--u));
    height: 100%;
    min-height: calc(560 * var(--u));
  }

  .back {
    flex: none;
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(6 * var(--u)) calc(12 * var(--u)) calc(6 * var(--u)) calc(8 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(9 * var(--u));
    background-color: #1d2027;
    font: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 500;
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .back:hover {
    border-color: rgba(255, 255, 255, 0.16);
    color: #edeae3;
  }

  .back:focus-visible,
  .icon-btn:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .back svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: calc(16 * var(--u));
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  /* With the AI coach open there's a third column: the problem and the editor share what's left. */
  .panes.with-coach {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) minmax(calc(300 * var(--u)), calc(380 * var(--u)));
  }

  .coach-pane {
    animation: coach-in 0.3s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  @keyframes coach-in {
    from {
      opacity: 0;
      transform: translateX(calc(16 * var(--u)));
    }
  }

  .pane {
    /* The judge panel's celebration is laid over the whole pane. */
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .toolbar {
    /* Same color as the page behind the dashboard area (the dropdown's background). */
    --control-bg: #1d2027;

    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    padding: calc(10 * var(--u)) calc(14 * var(--u));
    border-bottom: 1px solid var(--card-border);
  }

  /* Shown only if autosave is failing, so a problem is never silent. */
  .save-error {
    margin-left: auto;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #ff8a8a;
  }

  /* The coach button and the reset control, together at the right of the toolbar. */
  .tools {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
  }

  .coach-btn {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    box-sizing: border-box;
    height: calc(32 * var(--u));
    padding: 0 calc(12 * var(--u)) 0 calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #1d2027;
    font: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 500;
    color: #c9c2b4;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s,
      color 0.15s;
  }

  .coach-btn svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    stroke: url(#ai-gradient);
  }

  .coach-btn:hover {
    border-color: var(--ai-border-strong);
    color: #edeae3;
  }

  .coach-btn[aria-pressed="true"] {
    border-color: var(--ai-border-strong);
    background-color: var(--ai-tint);
    color: #edeae3;
  }

  /* A reply is being written (even with the drawer closed): the border becomes the flowing gradient. */
  .coach-btn.working {
    border-color: transparent;
    background:
      linear-gradient(#1d2027, #1d2027) padding-box,
      var(--ai-flow) border-box;
    background-size:
      auto,
      200% 100%;
    animation: ai-flow-border 4s linear infinite;
  }

  .coach-btn:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .coach-btn.working {
      animation: none;
    }
  }

  /* ---- Reset: an icon that turns into a yes/no question ---- */
  .reset-area {
    display: grid;
    justify-items: end; /* both layers hug the right edge, so the question grows leftwards */
    align-items: center;
  }

  .reset-area > * {
    grid-area: 1 / 1;
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: calc(6 * var(--u));
    opacity: 0;
    visibility: hidden;
    transition:
      opacity 0.15s,
      visibility 0s 0.15s;
  }

  .confirm.open {
    opacity: 1;
    visibility: visible;
    transition-delay: 0s;
  }

  /* The question starts closed (zero width, shifted right) and slides out to the left of the
     buttons. Animating a grid column from 0fr to 1fr makes it grow to the text's real width
     (a max-width would mostly animate past it), and the text stays anchored to the right
     edge, so the clip window opens leftwards over it. */
  .question {
    display: grid;
    grid-template-columns: 0fr;
    opacity: 0;
    transform: translateX(calc(12 * var(--u)));
    transition:
      grid-template-columns 0.28s cubic-bezier(0.2, 0.7, 0.2, 1),
      opacity 0.22s,
      transform 0.28s cubic-bezier(0.2, 0.7, 0.2, 1);
  }

  .confirm.open .question {
    grid-template-columns: 1fr;
    opacity: 1;
    transform: none;
  }

  .question-clip {
    display: flex;
    justify-content: flex-end;
    min-width: 0;
    overflow: hidden;
  }

  .question-text {
    flex: none;
    padding-right: calc(6 * var(--u));
    font-size: calc(13 * var(--u));
    font-weight: 500;
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    white-space: nowrap;
  }

  .reset-area .reset {
    transition:
      opacity 0.12s,
      border-color 0.2s,
      color 0.15s;
  }

  .reset-area .reset.away {
    opacity: 0;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: calc(32 * var(--u));
    height: calc(32 * var(--u));
    padding: 0;
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #1d2027;
    color: #a8a296;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s,
      color 0.15s;
  }

  .icon-btn svg {
    width: calc(15 * var(--u));
    height: calc(15 * var(--u));
  }

  .icon-btn:hover:not(:disabled) {
    border-color: rgba(255, 255, 255, 0.16);
    color: #edeae3;
  }

  .icon-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .icon-btn.yes {
    color: #3fae7f;
  }

  .icon-btn.yes:hover {
    border-color: rgba(63, 174, 127, 0.5);
    background-color: rgba(63, 174, 127, 0.14);
    color: #5fd0a0;
  }

  .icon-btn.no {
    color: #ff8a8a;
  }

  .icon-btn.no:hover {
    border-color: rgba(255, 85, 85, 0.5);
    background-color: rgba(255, 85, 85, 0.12);
    color: #ffa8a8;
  }

  /* ---- Custom tooltips (like the Problems tab), shown below since this sits at the top of
     its card, and aligned to the right edge so they can't spill out of it ---- */
  .tip {
    position: relative;
    display: inline-flex;
  }

  .tip::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 6;
    top: calc(100% + 8 * var(--u));
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
    transform: translateY(calc(-4 * var(--u)));
    transition:
      opacity 0.15s,
      transform 0.15s;
  }

  .tip:hover::after,
  .tip:has(:focus-visible)::after {
    opacity: 1;
    transform: none;
  }

  .editor-area {
    flex: 1;
    min-height: 0;
    /* Keeps Monaco's own layering beneath the dropdown menu and tooltips above it. */
    isolation: isolate;
  }

  .message {
    margin: 0;
    padding: calc(24 * var(--u));
    font-size: calc(13 * var(--u));
    line-height: calc(20 * var(--u));
    color: #a8a296;
  }

  /* Too narrow for two halves: stack them and let the page scroll. */
  @media (max-width: 1000px) {
    .workspace {
      height: auto;
    }

    .panes,
    .panes.with-coach {
      grid-template-columns: minmax(0, 1fr);
    }

    .pane {
      height: calc(560 * var(--u));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .panes,
    .coach-pane {
      animation: none;
    }

    .confirm,
    .question,
    .reset-area .reset,
    .tip::after {
      transition: none;
    }
  }
</style>
