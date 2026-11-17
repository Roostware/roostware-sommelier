<script lang="ts">
  // "Give up": ask to be shown the solution. It's the one thing the coach does that costs
  // something, so it asks first, and the question spells out what happens: the review it logs
  // (always Again), what it does to the rating, and when a re-solve starts to win that back. The
  // numbers are the backend's own preview, so the question is never a guess.
  //
  // One component, two looks: a muted button in the editor's bar, and a quiet link in the drawer.
  import { tick } from "svelte";
  import type { ProblemDetail } from "$lib/db";
  import { canGiveUp, coach, contextFor, phaseOf } from "$lib/coach.svelte";
  import { leetcode } from "$lib/leetcode.svelte";
  import { COACH_NAME } from "$lib/coachName";
  import { coachSetup } from "$lib/coachSetup.svelte";
  import { settings } from "$lib/settings.svelte";

  let {
    problem,
    lang,
    getCode,
    variant = "bar",
  }: {
    problem: ProblemDetail;
    lang: string;
    getCode: () => string;
    variant?: "bar" | "link";
  } = $props();

  const DAY_MS = 24 * 60 * 60 * 1000;

  let history = $derived(leetcode.stateFor(problem.id).history);
  let info = $derived(coach.giveUpInfo(problem.id));
  let phase = $derived(phaseOf(history, info));
  let busy = $derived(coach.thread(problem.id).busy);
  /** Not offered until the problem's submissions are known, and not when it makes no sense. */
  let visible = $derived(settings.coachEnabled && history !== null && canGiveUp(phase));

  let open = $state(false);
  let working = $state(false);
  let error = $state<string | null>(null);
  let rootEl: HTMLElement | undefined = $state();
  let keepEl: HTMLButtonElement | undefined = $state();
  let triggerEl: HTMLButtonElement | undefined = $state();

  const dateText = (ms: number) => new Date(ms).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });

  async function ask() {
    error = null;
    open = true;
    void coach.loadGiveUp(problem.id); // fresh numbers for the question
    await tick();
    keepEl?.focus(); // the safe answer is the one that's focused
  }

  function close(refocus = true) {
    open = false;
    if (refocus) triggerEl?.focus();
  }

  async function confirm() {
    if (working) return;
    working = true;
    error = null;
    try {
      const done = await coach.giveUpOn(contextFor(problem, lang, getCode));
      if (done) open = false;
      else error = `${COACH_NAME} is still replying. Try again in a moment.`;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      working = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && open) {
      event.preventDefault();
      close();
    }
  }

  // Clicking anywhere else means "never mind".
  function onWindowPointerDown(event: PointerEvent) {
    if (open && !working && rootEl && !rootEl.contains(event.target as Node)) close(false);
  }

  // Switching problems drops a pending question.
  $effect(() => {
    void problem.id;
    open = false;
    error = null;
  });

  // Giving up costs rating, so it's only possible when the coach can show the solution.
  let notReady = $derived(!coachSetup.ready);
  let tip = $derived(
    notReady ? `Set up ${COACH_NAME} first to give up. ${coachSetup.reason ?? ""}`.trim() : busy ? `Wait for ${COACH_NAME} to finish` : "",
  );
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

{#if visible}
  <div class="root {variant}" bind:this={rootEl}>
    <span class="tip" data-tip={tip} class:has-tip={tip !== ""}>
      <button
        type="button"
        class="trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={busy || working || notReady}
        bind:this={triggerEl}
        onclick={() => (open ? close() : ask())}
      >
        {#if variant === "bar"}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z" />
            <path d="M4 22v-7" />
          </svg>
          Give up
        {:else}
          Stuck for good? Give up and see the solution
        {/if}
      </button>
    </span>

    {#if open}
      <div class="pop" role="dialog" aria-label="Give up on this problem?" tabindex="-1" onkeydown={onKeydown}>
        <p class="title">Give up on this problem?</p>
        <p class="lead">You'll see the full solution.</p>
        <ul>
          <li><span class="k">Reviews</span> Logged as <strong>Again</strong>, so it comes back soon.</li>
          <li>
            <span class="k">Rating</span>
            {#if info === null}
              <span class="muted">Checking…</span>
            {:else if !info.affectsRating}
              Not affected: this problem already has a result.
            {:else if info.previewChange === 0}
              No visible change, since it's far above your rating.
              {#if info.previewRecoverable > 0}A clean re-solve from {dateText(Date.now() + info.waitDays * DAY_MS)} would still earn up to <strong>+{info.previewRecoverable}</strong>.{/if}
            {:else}
              About <strong class="loss">−{Math.abs(info.previewChange)}</strong> now. Solve it again from
              {dateText(Date.now() + info.waitDays * DAY_MS)} to win back up to <strong class="gain">+{info.previewRecoverable}</strong>.
            {/if}
          </li>
        </ul>
        {#if error}
          <p class="error" role="alert">{error}</p>
        {/if}
        <div class="buttons">
          <button type="button" class="keep" bind:this={keepEl} disabled={working} onclick={() => close()}>Keep trying</button>
          <button type="button" class="yes" disabled={working} onclick={confirm}>
            {#if working}<span class="spinner" aria-hidden="true"></span>{/if}Give up
          </button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(calc(6 * var(--u)));
    }
  }

  .root {
    position: relative;
    display: inline-flex;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    font: inherit;
    cursor: pointer;
    transition:
      border-color 0.2s,
      background-color 0.15s,
      color 0.15s;
  }

  /* In the bar: a muted button, the same height as Run and Submit. */
  .bar .trigger {
    box-sizing: border-box;
    padding: calc(7 * var(--u)) calc(14 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(9 * var(--u));
    background: none;
    font-size: calc(13 * var(--u));
    font-weight: 500;
    line-height: calc(18 * var(--u));
    color: #a8a296;
  }

  .bar .trigger svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
  }

  .bar .trigger:hover:not(:disabled),
  .bar .trigger[aria-expanded="true"] {
    border-color: rgba(255, 138, 138, 0.45);
    background-color: rgba(255, 85, 85, 0.08);
    color: #ffb0b0;
  }

  /* In the drawer: just a link. */
  .link .trigger {
    padding: 0;
    border: 0;
    background: none;
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #8d877b;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .link .trigger:hover:not(:disabled) {
    color: #ffb0b0;
  }

  .trigger:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .trigger:focus-visible,
  .keep:focus-visible,
  .yes:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* The question opens upward from the bar (it's at the bottom of the pane), and from the drawer's
     footer alike, aligned to the left edge of a link and the right edge of a button. */
  .pop {
    position: absolute;
    z-index: 8;
    bottom: calc(100% + 10 * var(--u));
    box-sizing: border-box;
    width: calc(310 * var(--u));
    max-width: calc(100vw - 48px);
    padding: calc(14 * var(--u)) calc(16 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(12 * var(--u));
    background-color: #12141a;
    box-shadow: 0 calc(10 * var(--u)) calc(28 * var(--u)) rgba(0, 0, 0, 0.5);
    animation: pop-in 0.15s ease-out;
  }

  .bar .pop {
    right: 0;
  }

  .link .pop {
    left: 0;
  }

  .title {
    margin: 0;
    font-size: calc(14 * var(--u));
    font-weight: 600;
    line-height: calc(20 * var(--u));
    color: #edeae3;
  }

  .lead {
    margin: calc(2 * var(--u)) 0 calc(8 * var(--u));
    font-size: calc(12.5 * var(--u));
    line-height: calc(18 * var(--u));
    color: #a8a296;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: calc(7 * var(--u));
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: calc(12.5 * var(--u));
    line-height: calc(18 * var(--u));
    color: #d7d2c6;
  }

  .k {
    display: inline-block;
    min-width: calc(52 * var(--u));
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #8d877b;
  }

  strong {
    font-weight: 600;
    color: #edeae3;
  }

  .loss {
    color: #ff8a8a;
  }

  .gain {
    color: #5fd0a0;
  }

  .muted {
    color: #8d877b;
  }

  .error {
    margin: calc(10 * var(--u)) 0 0;
    font-size: calc(12 * var(--u));
    line-height: calc(17 * var(--u));
    color: #ff8a8a;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: calc(8 * var(--u));
    margin-top: calc(14 * var(--u));
  }

  .keep,
  .yes {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(6 * var(--u)) calc(14 * var(--u));
    border-radius: calc(8 * var(--u));
    font: inherit;
    font-size: calc(12.5 * var(--u));
    font-weight: 600;
    line-height: calc(18 * var(--u));
    cursor: pointer;
  }

  .keep {
    border: 1px solid transparent;
    background-color: #e9bc22;
    color: #1b1608;
  }

  .keep:hover:not(:disabled) {
    background-color: #f2c93a;
  }

  .yes {
    border: 1px solid rgba(255, 138, 138, 0.45);
    background: none;
    color: #ffb0b0;
  }

  .yes:hover:not(:disabled) {
    background-color: rgba(255, 85, 85, 0.12);
  }

  .keep:disabled,
  .yes:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .spinner {
    box-sizing: border-box;
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Custom tooltip, like the rest of the app (shown above, since this sits at the bottom). */
  .tip {
    position: relative;
    display: inline-flex;
  }

  .tip.has-tip::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 6;
    bottom: calc(100% + 8 * var(--u));
    left: 0;
    padding: calc(5 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(8 * var(--u));
    background-color: #12141a;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
    white-space: normal;
    width: max-content;
    max-width: calc(260 * var(--u));
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .tip.has-tip:hover::after {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .pop {
      animation: none;
    }

    .spinner {
      animation-duration: 2.4s;
    }
  }
</style>
