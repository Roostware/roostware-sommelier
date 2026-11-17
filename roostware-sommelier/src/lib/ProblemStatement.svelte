<script lang="ts">
  // The left half of the Solve tab: the problem's title, a few facts about it, its statement,
  // and the hint button. The topic tags are a hint the user asks for, so they aren't shown
  // until then.
  import { tick } from "svelte";
  import CollectionPicker from "$lib/CollectionPicker.svelte";
  import type { Progress, ProblemDetail } from "$lib/db";
  import { difficultyColor } from "$lib/difficulty";
  import { openExternal } from "$lib/external";
  import { hintSteps } from "$lib/hints";
  import { sanitizeStatement } from "$lib/sanitize";
  import { solve } from "$lib/solve.svelte";

  let { problem }: { problem: ProblemDetail } = $props();

  const progressLabels: Record<Progress, string> = {
    solved: "Solved",
    attempted: "Attempted",
    inProgress: "In progress",
    untouched: "Not started",
  };

  const count = new Intl.NumberFormat();
  let html = $derived(sanitizeStatement(problem.contentHtml));

  // ---- Hints --------------------------------------------------------------------
  // For now the only hint is the topic tags (when the problem has any). The statement's
  // follow-up is deliberately not a hint: it stays in the statement.
  let steps = $derived(hintSteps(problem.tags));
  let shown = $derived(Math.min(solve.hintsShown(problem.id), steps.length));
  let shownSteps = $derived(steps.slice(0, shown));
  let canReveal = $derived(shown < steps.length);

  let hintLabel = $derived(shown === 0 ? "Hint" : canReveal ? "Next hint" : "All hints shown");
  let hintTip = $derived(
    steps.length === 0
      ? "No hints for this problem"
      : canReveal
        ? `Show hint ${shown + 1} of ${steps.length}`
        : "You've seen every hint",
  );

  let dockEl: HTMLElement | undefined = $state();

  async function revealHint() {
    if (!canReveal) return;
    solve.revealHint(problem.id, steps.length);
    // The hints are docked at the bottom, so they're always in view. If there are ever enough
    // of them to outgrow their space, make sure the newest is the one showing.
    await tick();
    const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    dockEl?.scrollTo({ top: dockEl.scrollHeight, behavior: calm ? "auto" : "smooth" });
  }

  // Links in a statement must never navigate the app's own window away, so they open in the
  // system browser instead. (An action rather than an onclick: the links are real anchors,
  // so keyboard users reach this too.)
  function externalLinks(node: HTMLElement) {
    function onClick(event: MouseEvent) {
      const link = (event.target as Element | null)?.closest("a");
      if (!link || !node.contains(link)) return;
      event.preventDefault();
      const href = link.getAttribute("href");
      if (href) void openExternal(href);
    }
    node.addEventListener("click", onClick);
    return { destroy: () => node.removeEventListener("click", onClick) };
  }
</script>

<article class="statement-pane">
  <header class="head">
    <h1><span class="num">{problem.frontendId}.</span> {problem.title}</h1>
    <div class="badges">
      <span class="badge" style="--c: {difficultyColor[problem.difficulty]}">
        <span class="dot"></span>
        {problem.difficulty}
      </span>
      {#if problem.isPaidOnly}
        <span class="badge premium">Premium</span>
      {/if}
      <span class="badge" class:solved={problem.progress === "solved"}>{progressLabels[problem.progress]}</span>

      <div class="actions">
        <CollectionPicker
          problemId={problem.id}
          collectionIds={problem.collectionIds}
          onchange={() => solve.refresh()}
        />

        <!-- Greyed out and disabled when there's nothing to reveal (no hints, or all shown). -->
        <span class="tip hint-tip" data-tip={hintTip}>
          <button
            type="button"
            class="hint-btn"
            disabled={!canReveal}
            aria-label={steps.length === 0
              ? "No hints available for this problem"
              : canReveal
                ? `Show hint ${shown + 1} of ${steps.length}`
                : "All hints shown"}
            onclick={revealHint}
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
              <path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5" />
              <path d="M9 18h6" />
              <path d="M10 22h4" />
            </svg>
            <span>{hintLabel}</span>
            {#if steps.length > 0}
              <span class="hint-count" aria-hidden="true">{shown}/{steps.length}</span>
            {/if}
          </button>
        </span>
      </div>
    </div>
    <dl class="facts">
      <div>
        <dt>Acceptance</dt>
        <dd>{(problem.acceptanceRate * 100).toFixed(1)}%</dd>
      </div>
      <div>
        <dt>Accepted</dt>
        <dd>{count.format(problem.totalAccepted)}</dd>
      </div>
      <div>
        <dt>Submissions</dt>
        <dd>{count.format(problem.totalSubmissions)}</dd>
      </div>
      <div>
        <dt>Likes</dt>
        <dd>{count.format(problem.likes)}</dd>
      </div>
      <div>
        <dt>Your solves</dt>
        <dd>{count.format(problem.solveCount)}</dd>
      </div>
    </dl>
  </header>

  <!-- The statement HTML is sanitized by sanitizeStatement above. -->
  <div class="statement" use:externalLinks>
    {@html html}
  </div>

  <!-- Revealed hints are docked below the statement, so they stay in view while it scrolls. -->
  {#if shownSteps.length > 0}
    <div class="hints-dock" bind:this={dockEl}>
      <section class="hints" aria-label="Hints" aria-live="polite">
        <div class="hints-head">
          <span>Hints</span>
          <span class="hints-progress">{shown} of {steps.length} shown</span>
        </div>
        {#each shownSteps as step, i (step.kind)}
          <div class="hint-step">
            <span class="hint-num" aria-hidden="true">{i + 1}</span>
            <div class="hint-main">
              <h2>Hint {i + 1}: {step.title}</h2>
              {#if step.kind === "topics"}
                <ul class="tags">
                  {#each step.tags as tag (tag)}
                    <li class="tag">{tag}</li>
                  {/each}
                </ul>
              {/if}
            </div>
          </div>
        {/each}
      </section>
    </div>
  {/if}
</article>

<style>
  .statement-pane {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }

  .head {
    flex: none;
    padding: calc(20 * var(--u)) calc(24 * var(--u)) calc(16 * var(--u));
    border-bottom: 1px solid var(--card-border);
  }

  h1 {
    margin: 0;
    font-size: calc(22 * var(--u));
    font-weight: 600;
    line-height: 1.25;
  }

  .num {
    color: #a8a296;
    font-variant-numeric: tabular-nums;
  }

  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: calc(8 * var(--u));
    margin-top: calc(12 * var(--u));
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(3 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #1d2027;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
  }

  .badge.solved {
    border-color: transparent;
    background-color: rgba(233, 188, 34, 0.16);
    color: #e9bc22;
  }

  .badge.premium {
    border-color: transparent;
    background-color: rgba(170, 177, 197, 0.16);
    color: #aab1c5;
  }

  .dot {
    width: calc(7 * var(--u));
    height: calc(7 * var(--u));
    border-radius: 50%;
    background-color: var(--c);
  }

  /* The collections and hint buttons, pushed to the right edge of the badges row. */
  .actions {
    display: flex;
    gap: calc(8 * var(--u));
    margin-left: auto;
  }

  /* ---- Hint button ---- */

  .hint-btn {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    padding: calc(3 * var(--u)) calc(10 * var(--u)) calc(3 * var(--u)) calc(8 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #1d2027;
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
    cursor: pointer;
    transition:
      border-color 0.2s,
      opacity 0.2s;
  }

  .hint-btn svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    color: #e9bc22;
  }

  .hint-btn:hover:not(:disabled) {
    border-color: rgba(233, 188, 34, 0.5);
  }

  .hint-btn:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* Greyed out and disabled: nothing (more) to reveal. */
  .hint-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .hint-btn:disabled svg {
    color: #a8a296;
  }

  .hint-count {
    padding-left: calc(6 * var(--u));
    border-left: 1px solid var(--card-border);
    font-size: calc(11 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #a8a296;
  }

  /* Custom tooltip (like the rest of the app), shown below and aligned to the right edge. */
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

  /* ---- Revealed hints, docked at the bottom of the pane ---- */
  .hints-dock {
    flex: none;
    box-sizing: border-box; /* so the cap below covers the padding too */
    max-height: 45%; /* never crowds out the statement; scrolls inside if it ever needs to */
    overflow-y: auto;
    padding: calc(14 * var(--u)) calc(24 * var(--u)) calc(20 * var(--u));
    border-top: 1px solid var(--card-border);
  }

  .hints {
    padding: calc(12 * var(--u)) calc(16 * var(--u)) calc(4 * var(--u));
    border: 1px solid rgba(233, 188, 34, 0.22);
    border-radius: calc(10 * var(--u));
    background-color: #1d2027;
  }

  .hints-head {
    display: flex;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.06em;
    line-height: calc(16 * var(--u));
    text-transform: uppercase;
    color: #e9bc22;
  }

  .hints-progress {
    font-weight: 500;
    letter-spacing: 0;
    text-transform: none;
    color: #a8a296;
  }

  .hint-step {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: calc(12 * var(--u));
    padding: calc(12 * var(--u)) 0;
    animation: hint-in 0.35s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .hint-step + .hint-step {
    border-top: 1px solid var(--card-border);
  }

  @keyframes hint-in {
    from {
      opacity: 0;
      transform: translateY(calc(8 * var(--u)));
    }
  }

  .hint-num {
    display: grid;
    place-items: center;
    width: calc(22 * var(--u));
    height: calc(22 * var(--u));
    border-radius: 50%;
    background-color: rgba(233, 188, 34, 0.16);
    font-size: calc(12 * var(--u));
    font-weight: 600;
    line-height: 1;
    color: #e9bc22;
  }

  .hint-main h2 {
    margin: calc(1 * var(--u)) 0 calc(8 * var(--u));
    font-size: calc(13 * var(--u));
    font-weight: 600;
    line-height: calc(20 * var(--u));
    color: #edeae3;
  }

  /* Scoped under .hints so these outrank the statement's own list styles (indented lists with
     spacing between items), which would otherwise leak into the tag list. */
  .hints .tags {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .hints .tag {
    margin: 0;
    padding: calc(2 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(6 * var(--u));
    background-color: #262a33;
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
  }

  @media (prefers-reduced-motion: reduce) {
    .hint-step {
      animation: none;
    }

    .tip::after,
    .hint-btn {
      transition: none;
    }
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u)) calc(22 * var(--u));
    margin: calc(14 * var(--u)) 0 0;
  }

  .facts div {
    display: flex;
    gap: calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
  }

  dt {
    color: #a8a296;
  }

  dd {
    margin: 0;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: #edeae3;
  }

  .statement {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: calc(20 * var(--u)) calc(24 * var(--u)) calc(28 * var(--u));
    font-size: calc(14 * var(--u));
    line-height: 1.65;
    color: #d6d2c8;
    overflow-wrap: anywhere;
  }

  /* The statement is injected HTML, so its elements are styled with :global, scoped to it. */
  .statement :global(p) {
    margin: 0 0 calc(12 * var(--u));
  }

  .statement :global(strong),
  .statement :global(b) {
    color: #edeae3;
    font-weight: 600;
  }

  .statement :global(a) {
    color: #e9bc22;
    text-decoration: none;
  }

  .statement :global(a:hover) {
    text-decoration: underline;
  }

  .statement :global(code) {
    padding: 1px calc(6 * var(--u));
    border-radius: calc(5 * var(--u));
    background-color: rgba(255, 255, 255, 0.08);
    font-family: var(--font-code);
    font-size: 0.9em;
    color: #edeae3;
  }

  .statement :global(pre) {
    margin: 0 0 calc(14 * var(--u));
    padding: calc(12 * var(--u)) calc(16 * var(--u));
    overflow-x: auto;
    border-left: calc(3 * var(--u)) solid rgba(255, 255, 255, 0.18);
    border-radius: calc(4 * var(--u));
    background-color: #1d2027;
    font-family: var(--font-code);
    font-size: calc(13 * var(--u));
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .statement :global(pre code) {
    padding: 0;
    background: none;
    font-size: inherit;
  }

  .statement :global(ul),
  .statement :global(ol) {
    margin: 0 0 calc(12 * var(--u));
    padding-left: calc(24 * var(--u));
  }

  .statement :global(li) {
    margin-bottom: calc(4 * var(--u));
  }

  .statement :global(img) {
    display: block;
    max-width: 100%;
    height: auto;
    margin: calc(8 * var(--u)) 0 calc(14 * var(--u));
    padding: calc(4 * var(--u));
    border-radius: calc(6 * var(--u));
    background-color: #fff;
  }

  .statement :global(table) {
    margin: 0 0 calc(14 * var(--u));
    border-collapse: collapse;
    font-size: calc(13 * var(--u));
  }

  .statement :global(th),
  .statement :global(td) {
    padding: calc(6 * var(--u)) calc(12 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.14);
    text-align: left;
  }

  .statement :global(th) {
    background-color: rgba(255, 255, 255, 0.05);
    color: #edeae3;
  }

  .statement :global(blockquote) {
    margin: 0 0 calc(12 * var(--u));
    padding-left: calc(14 * var(--u));
    border-left: calc(3 * var(--u)) solid rgba(255, 255, 255, 0.18);
    color: #a8a296;
  }

  .statement :global(sup),
  .statement :global(sub) {
    font-size: 0.75em;
    line-height: 0;
  }
</style>
