<script lang="ts">
  // The full explanation of a problem: the idea, the steps, annotated code, complexity, where the
  // user's own attempt went a different way (when there was one), and what to take away. Shown
  // after giving up, or on request once the problem is solved.
  import type { SolutionExplanation } from "$lib/coachProvider";
  import CoachRich from "$lib/CoachRich.svelte";

  let {
    solution,
    aboutCode,
  }: {
    solution: SolutionExplanation;
    /** The user's code when the solution was written, for the references in "where your attempt went a different way". */
    aboutCode: string | null;
  } = $props();

  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    try {
      await navigator.clipboard.writeText(solution.code);
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 1600);
    } catch (e) {
      console.warn("Couldn't copy the solution:", e);
    }
  }
</script>

<div class="card-body">
  <section>
    <h4>The idea</h4>
    <CoachRich text={solution.intuition} {aboutCode} />
  </section>

  <section>
    <h4>Step by step</h4>
    <ol>
      {#each solution.steps as step, i (i)}
        <li><CoachRich text={step} {aboutCode} /></li>
      {/each}
    </ol>
  </section>

  <section>
    <div class="code-head">
      <h4>The code</h4>
      <button type="button" class="copy" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
    </div>
    <pre><code>{solution.code}</code></pre>
  </section>

  <section>
    <h4>Complexity</h4>
    <CoachRich text={solution.complexity} {aboutCode} />
  </section>

  {#if solution.divergence}
    <section class="diverged">
      <h4>Where your attempt went a different way</h4>
      <CoachRich text={solution.divergence} {aboutCode} />
    </section>
  {/if}

  <section>
    <h4>Techniques to reuse</h4>
    <ul>
      {#each solution.takeaways as item, i (i)}
        <li><CoachRich text={item} {aboutCode} /></li>
      {/each}
    </ul>
  </section>
</div>

<style>
  .card-body {
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
  }

  h4 {
    margin: 0 0 calc(4 * var(--u));
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #8d877b;
  }

  ol,
  ul {
    margin: 0;
    padding-left: calc(20 * var(--u));
  }

  li + li {
    margin-top: calc(3 * var(--u));
  }

  .code-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .code-head h4 {
    margin: 0;
  }

  .copy {
    padding: calc(2 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(999 * var(--u));
    background-color: #181b21;
    font: inherit;
    font-size: calc(11.5 * var(--u));
    font-weight: 500;
    color: #a8a296;
    cursor: pointer;
  }

  .copy:hover {
    color: #edeae3;
  }

  .copy:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }

  pre {
    margin: calc(6 * var(--u)) 0 0;
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #14171c;
    overflow-x: auto;
  }

  pre code {
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    white-space: pre;
  }

  .diverged {
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 1px solid rgba(233, 188, 34, 0.3);
    border-radius: calc(10 * var(--u));
    background-color: rgba(233, 188, 34, 0.07);
  }

  .diverged h4 {
    color: #e9bc22;
  }
</style>
