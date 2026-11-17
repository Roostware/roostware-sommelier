<script lang="ts">
  // The coach's text, formatted (see coachText.ts). A reference to lines of the user's code
  // becomes a button that highlights them in the editor, when the editor can.
  import { parseBlocks, type Inline } from "$lib/coachText";
  import { coachJump } from "$lib/coachJump.svelte";
  import { COACH_NAME } from "$lib/coachName";

  let {
    text,
    aboutCode = undefined,
  }: {
    text: string;
    /** The code the text was written about. Its line references only work while the editor still holds it. */
    aboutCode?: string | null;
  } = $props();

  // How many lines the code has, so plain "line 7" in the text can be told from "line 40 of the statement".
  let lineCount = $derived(aboutCode ? aboutCode.trimEnd().split("\n").length : 0);
  let blocks = $derived(parseBlocks(text, { lines: lineCount }));

  /** Why a reference to lines `from` to `to` is switched off, or "" when it works. */
  function whyOff(from: number, to: number): string {
    if (aboutCode == null) return "No code was shared with this message";
    switch (coachJump.linesMatch(aboutCode, from, to)) {
      case "same":
        return "";
      case "changed":
        return "Those lines have changed since this was written";
      case "missing":
        return `That line isn't in the code ${COACH_NAME} was shown`;
      default:
        return "The editor isn't open";
    }
  }
</script>

{#snippet inline(parts: Inline[])}
  {#each parts as part, i (i)}
    {#if part.t === "text"}{part.s}{:else if part.t === "bold"}<strong>{@render inline(part.parts)}</strong>{:else if part.t === "em"}<em>{@render inline(part.parts)}</em>{:else if part.t === "code"}<code>{part.s}</code>{:else}{@const off = whyOff(part.from, part.to)}<span class="ref-tip" data-tip={off}><button
          type="button"
          class="line-ref"
          disabled={off !== ""}
          aria-label={part.from === part.to ? `Show line ${part.from} in the editor` : `Show lines ${part.from} to ${part.to} in the editor`}
          onclick={() => coachJump.show(part.from, part.to)}>{part.from === part.to ? `L${part.from}` : `L${part.from}–${part.to}`}</button
        ></span>{/if}
  {/each}
{/snippet}

<div class="rich">
  {#each blocks as block, i (i)}
    {#if block.type === "p"}
      <p>{@render inline(block.parts)}</p>
    {:else if block.type === "list"}
      <ul>
        {#each block.items as item, j (j)}
          <li>{@render inline(item)}</li>
        {/each}
      </ul>
    {:else}
      <pre><code>{block.code}</code></pre>
    {/if}
  {/each}
</div>

<style>
  .rich {
    font-size: calc(13 * var(--u));
    line-height: calc(20 * var(--u));
    color: #d7d2c6;
    overflow-wrap: anywhere;
  }

  p {
    margin: 0;
  }

  p + p,
  p + ul,
  ul + p,
  pre + p,
  p + pre,
  ul + pre,
  pre + ul {
    margin-top: calc(8 * var(--u));
  }

  ul {
    margin: 0;
    padding-left: calc(18 * var(--u));
  }

  li + li {
    margin-top: calc(3 * var(--u));
  }

  strong {
    font-weight: 600;
    color: #edeae3;
  }

  code {
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
    font-size: 0.92em;
    padding: 0 calc(4 * var(--u));
    border-radius: calc(4 * var(--u));
    background-color: rgba(255, 255, 255, 0.07);
    color: #edeae3;
  }

  pre {
    margin: 0;
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #14171c;
    overflow-x: auto;
  }

  pre code {
    padding: 0;
    background: none;
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    white-space: pre;
  }

  .line-ref {
    margin: 0 calc(1 * var(--u));
    padding: 0 calc(6 * var(--u));
    border: 1px solid var(--ai-border);
    border-radius: calc(6 * var(--u));
    background-color: var(--ai-tint);
    font: inherit;
    font-family: "Cascadia Code", "Cascadia Mono", Consolas, monospace;
    font-size: 0.9em;
    color: var(--ai-solid);
    cursor: pointer;
    transition:
      background-color 0.15s,
      border-color 0.15s;
  }

  .line-ref:hover:not(:disabled),
  .line-ref:active:not(:disabled) {
    border-color: var(--ai-border-strong);
    background-color: var(--ai-tint-strong);
  }

  /* Plainly switched off: grey and dashed, so it doesn't look like a button that failed. */
  .line-ref:disabled {
    border-style: dashed;
    border-color: rgba(255, 255, 255, 0.18);
    background-color: transparent;
    color: #8d877b;
    cursor: not-allowed;
  }

  .line-ref:focus-visible {
    outline: 2px solid var(--ai-solid);
    outline-offset: 2px;
  }

  /* A custom tooltip (like the rest of the app) that says why a reference is switched off. */
  .ref-tip {
    position: relative;
    display: inline-block;
  }

  .ref-tip[data-tip]:not([data-tip=""])::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 6;
    bottom: calc(100% + 6 * var(--u));
    left: 0;
    width: max-content;
    max-width: calc(220 * var(--u));
    padding: calc(5 * var(--u)) calc(10 * var(--u));
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: calc(8 * var(--u));
    background-color: #12141a;
    box-shadow: 0 calc(6 * var(--u)) calc(16 * var(--u)) rgba(0, 0, 0, 0.35);
    font-size: calc(12 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    color: #edeae3;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .ref-tip:hover::after {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .ref-tip::after {
      transition: none;
    }
  }
</style>
