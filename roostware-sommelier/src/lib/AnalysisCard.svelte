<script lang="ts">
  // The coach's Big-O analysis of an accepted submission: your complexity (with the reasoning),
  // how it compares with the best known, what went well, what to improve, and what to take
  // away. The runtime and memory beside it are what LeetCode measured, not the coach's opinion.
  import type { Submission } from "$lib/db";
  import type { Analysis } from "$lib/coachProvider";
  import CoachRich from "$lib/CoachRich.svelte";

  let {
    analysis,
    aboutCode,
    submission,
    busy,
    ondiscuss,
    onoptimal,
    onagain,
  }: {
    analysis: Analysis;
    /** The code the analysis was written about, for its line references. */
    aboutCode: string | null;
    /** The submission it's about, if it's still in the history, for the measured numbers. */
    submission: Submission | null;
    /** The coach is replying, so asking for more has to wait. */
    busy: boolean;
    ondiscuss: () => void;
    onoptimal: () => void;
    onagain: () => void;
  } = $props();

  const runtime = $derived(submission?.runtimeMs == null ? null : `${submission.runtimeMs} ms`);
  const memory = $derived(submission?.memoryKb == null ? null : `${(submission.memoryKb / 1024).toFixed(1)} MB`);
</script>

<div class="card-body">
  <div class="big-o" role="group" aria-label="Complexity">
    <div class="cell">
      <span class="k">Time</span>
      <span class="v">{analysis.time.big}</span>
      <div class="why"><CoachRich text={analysis.time.why} {aboutCode} /></div>
    </div>
    <div class="cell">
      <span class="k">Space</span>
      <span class="v">{analysis.space.big}</span>
      <div class="why"><CoachRich text={analysis.space.why} {aboutCode} /></div>
    </div>
  </div>

  <p class="optimal">
    <span class="k">Best known</span>
    <strong>{analysis.optimal.time}</strong> time · <strong>{analysis.optimal.space}</strong> space
  </p>
  <div class="note"><CoachRich text={analysis.optimal.note} {aboutCode} /></div>

  {#if runtime || memory}
    <p class="measured">
      Measured by LeetCode: {[runtime, memory].filter(Boolean).join(" · ")}
    </p>
  {/if}

  <section class="good">
    <h4><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>Went well</h4>
    <ul>
      {#each analysis.wentWell as item, i (i)}
        <li><CoachRich text={item} {aboutCode} /></li>
      {/each}
    </ul>
  </section>

  <section class="better">
    <h4><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 17 17 7" /><path d="M8 7h9v9" /></svg>Could improve</h4>
    <ul>
      {#each analysis.improve as item, i (i)}
        <li><CoachRich text={item} {aboutCode} /></li>
      {/each}
    </ul>
  </section>

  <section class="take">
    <h4><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 2 8l10 5 10-5-10-5Z" /><path d="M6 10.5V16c0 1.5 2.7 3 6 3s6-1.5 6-3v-5.5" /></svg>Techniques to reuse</h4>
    <ul>
      {#each analysis.takeaways as item, i (i)}
        <li><CoachRich text={item} {aboutCode} /></li>
      {/each}
    </ul>
  </section>

  <div class="actions">
    <button type="button" class="chip" onclick={ondiscuss}>Discuss</button>
    <button type="button" class="chip" disabled={busy} onclick={onoptimal}>Show the optimal solution</button>
    <button type="button" class="chip quiet" disabled={busy} onclick={onagain}>Analyze again</button>
  </div>
</div>

<style>
  .card-body {
    display: flex;
    flex-direction: column;
    gap: calc(10 * var(--u));
  }

  .big-o {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: calc(8 * var(--u));
  }

  .cell {
    display: flex;
    flex-direction: column;
    gap: calc(2 * var(--u));
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(10 * var(--u));
    background-color: #1d2027;
  }

  .k {
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #8d877b;
  }

  .v {
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
    font-size: calc(18 * var(--u));
    font-weight: 600;
    line-height: calc(24 * var(--u));
    color: #edeae3;
  }

  .why {
    color: #a8a296;
  }

  .why :global(.rich) {
    font-size: calc(12 * var(--u));
    line-height: calc(17 * var(--u));
    color: #a8a296;
  }

  .optimal {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: calc(6 * var(--u));
    margin: 0;
    font-size: calc(13 * var(--u));
    color: #d7d2c6;
  }

  .optimal strong {
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
    font-weight: 600;
    color: #edeae3;
  }

  .note {
    margin-top: calc(-4 * var(--u));
  }

  .measured {
    margin: 0;
    font-size: calc(12 * var(--u));
    color: #8d877b;
  }

  section {
    padding-top: calc(2 * var(--u));
  }

  h4 {
    display: flex;
    align-items: center;
    gap: calc(6 * var(--u));
    margin: 0 0 calc(4 * var(--u));
    font-size: calc(12.5 * var(--u));
    font-weight: 600;
    color: #edeae3;
  }

  h4 svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
  }

  .good h4 {
    color: #5fd0a0;
  }

  .better h4 {
    color: #e9bc22;
  }

  .take h4 {
    color: #9db4e8;
  }

  ul {
    margin: 0;
    padding-left: calc(18 * var(--u));
  }

  li + li {
    margin-top: calc(3 * var(--u));
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
    padding-top: calc(4 * var(--u));
  }

  .chip {
    padding: calc(5 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    font: inherit;
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .chip:hover:not(:disabled) {
    border-color: var(--ai-border-strong);
    color: #edeae3;
  }

  .chip.quiet {
    color: #8d877b;
  }

  .chip:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .chip:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }
</style>
