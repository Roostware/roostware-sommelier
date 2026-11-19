<script lang="ts">
  // Shown on the Solve tab when no problem is open: search for a problem to open, or pick
  // up one of the recently opened ones.
  import { onMount, untrack } from "svelte";
  import { listProblems, listRecentProblems, type ProblemSummary, type RecentProblem } from "$lib/db";
  import { difficultyColor } from "$lib/difficulty";
  import { openProblem } from "$lib/solve.svelte";

  const RESULT_LIMIT = 10;

  // `text` is what's typed; `search` is what's actually queried (after a short pause).
  let text = $state("");
  let search = $state("");
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  let recents = $state<RecentProblem[]>([]);
  let recentsLoaded = $state(false);

  let results = $state<ProblemSummary[]>([]);
  let total = $state(0);
  let resultsFor = $state<string | null>(null); // the search the current `results` answer
  let error = $state<string | null>(null);

  const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

  onMount(async () => {
    inputEl?.focus();
    try {
      recents = await listRecentProblems(8);
      error = null;
    } catch (e) {
      error = messageOf(e);
    }
    recentsLoaded = true;
  });

  $effect(() => {
    const value = text.trim();
    const timer = setTimeout(() => (search = value), 200);
    return () => clearTimeout(timer);
  });

  // Bumped per search so a slow, outdated response can't replace newer results.
  let generation = 0;
  // Set when Enter is pressed before the results are in: open the top hit once they arrive.
  let openOnLoad: string | null = null;

  $effect(() => {
    const q = search;
    untrack(() => runSearch(q));
  });

  async function runSearch(q: string) {
    const gen = ++generation;
    if (q === "") {
      resultsFor = null;
      return;
    }
    try {
      const page = await listProblems({ filter: { query: q }, limit: RESULT_LIMIT });
      if (gen !== generation) return;
      results = page.items;
      total = page.total;
      resultsFor = q;
      error = null;
      if (openOnLoad === q) {
        openOnLoad = null;
        if (page.items[0]) openProblem(page.items[0].id);
      }
    } catch (e) {
      if (gen !== generation) return;
      error = messageOf(e);
    }
  }

  function submit() {
    const q = text.trim();
    if (!q) return;
    if (resultsFor === q && results.length > 0) {
      openProblem(results[0].id);
      return;
    }
    openOnLoad = q;
    search = q; // applies immediately, skipping the typing pause
  }

  function clear() {
    text = "";
    search = "";
    openOnLoad = null;
    inputEl?.focus();
  }

  function onInputKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      submit();
    } else if (event.key === "Escape" && text) {
      event.preventDefault();
      clear();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      listEl?.querySelector<HTMLElement>(".item")?.focus();
    }
  }

  // Up/Down move between the listed problems; Up from the first goes back to the search box.
  function onItemKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const items = [...(listEl?.querySelectorAll<HTMLElement>(".item") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at === -1) return;
    event.preventDefault();
    if (event.key === "ArrowDown") items[Math.min(at + 1, items.length - 1)].focus();
    else if (at === 0) inputEl?.focus();
    else items[at - 1].focus();
  }

  function timeAgo(epochMs: number): string {
    const minutes = Math.max(0, Math.round((Date.now() - epochMs) / 60_000));
    if (minutes < 1) return "just now";
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours} h ago`;
    const days = Math.round(hours / 24);
    if (days === 1) return "yesterday";
    if (days < 30) return `${days} days ago`;
    return new Date(epochMs).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
  }

  let searching = $derived(search !== "");
</script>

{#snippet item(p: ProblemSummary, meta: string | null)}
  <li>
    <button type="button" class="item" onclick={() => openProblem(p.id)} onkeydown={onItemKeydown}>
      <span class="item-name">
        <span class="number">{p.frontendId}.</span>
        <span class="title">{p.title}</span>
      </span>
      <span class="meta">{meta ?? ""}</span>
      <span class="difficulty" style="--c: {difficultyColor[p.difficulty]}">
        <span class="dot"></span>
        {p.difficulty}
      </span>
    </button>
  </li>
{/snippet}

<div class="picker">
  <header class="intro">
    <h1>Solve</h1>
    <p class="subtitle">Choose a problem to start working on.</p>
  </header>

  <div class="search-box">
    <svg
      class="search-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
    <input
      type="search"
      class="search-input"
      placeholder="Search by name or number"
      aria-label="Search for a problem to open"
      autocomplete="off"
      spellcheck="false"
      bind:this={inputEl}
      bind:value={text}
      onkeydown={onInputKeydown}
    />
    {#if text}
      <button type="button" class="search-clear" aria-label="Clear search" onclick={clear}>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="3"
          stroke-linecap="round"
          aria-hidden="true"
        >
          <path d="M6 6l12 12M18 6 6 18" />
        </svg>
      </button>
    {/if}
  </div>

  {#if error}
    <p class="message" role="alert">Couldn't load problems: {error}</p>
  {:else if searching}
    <section class="group" aria-label="Search results">
      {#if resultsFor !== search}
        <p class="message" role="status">Searching…</p>
      {:else if results.length === 0}
        <p class="message">No problems match "{search}".</p>
      {:else}
        <ul class="list card" bind:this={listEl}>
          {#each results as p (p.id)}
            {@render item(p, null)}
          {/each}
        </ul>
        {#if total > results.length}
          <p class="more">Showing the top {results.length} of {total} matches. Keep typing to narrow it down.</p>
        {/if}
      {/if}
    </section>
  {:else}
    <section class="group" aria-labelledby="recent-heading">
      <h2 id="recent-heading" class="group-title">Recently opened</h2>
      {#if !recentsLoaded}
        <p class="message" role="status">Loading…</p>
      {:else if recents.length === 0}
        <p class="message">Problems you open will show up here. Search above, or pick one from the Problems tab.</p>
      {:else}
        <ul class="list card" bind:this={listEl}>
          {#each recents as p (p.id)}
            {@render item(p, timeAgo(p.lastOpenedAt))}
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(calc(14 * var(--u)));
    }
  }

  .picker {
    /* Same color as the page behind the dashboard area. */
    --control-bg: #1d2027;

    display: flex;
    flex-direction: column;
    gap: calc(20 * var(--u));
    max-width: calc(760 * var(--u));
    margin: calc(24 * var(--u)) auto 0;
  }

  .intro,
  .search-box,
  .group {
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .search-box {
    animation-delay: 0.05s;
  }

  .group {
    animation-delay: 0.1s;
  }

  h1 {
    margin: 0;
    font-size: calc(28 * var(--u));
    font-weight: 600;
    line-height: 1.2;
  }

  .subtitle {
    margin: calc(6 * var(--u)) 0 0;
    font-size: calc(14 * var(--u));
    color: #a8a296;
  }

  /* ---- Search box (same look as the Problems tab) ---- */
  .search-box {
    display: flex;
    align-items: center;
    gap: calc(12 * var(--u));
    box-sizing: border-box;
    padding: calc(12 * var(--u)) calc(16 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(12 * var(--u));
    background-color: var(--control-bg);
    transition:
      border-color 0.2s,
      box-shadow 0.2s;
  }

  .search-box:hover {
    border-color: rgba(255, 255, 255, 0.16);
  }

  .search-box:focus-within {
    border-color: #e9bc22;
    box-shadow: 0 0 0 1px #e9bc22;
  }

  .search-icon {
    flex: none;
    width: calc(18 * var(--u));
    height: calc(18 * var(--u));
    color: #a8a296;
  }

  .search-input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: calc(15 * var(--u));
    line-height: calc(22 * var(--u));
    color: #edeae3;
  }

  .search-input::placeholder {
    color: #6f6a60;
  }

  /* The custom clear button replaces the browser's own. */
  .search-input::-webkit-search-cancel-button,
  .search-input::-webkit-search-decoration {
    display: none;
    -webkit-appearance: none;
  }

  .search-clear {
    display: grid;
    flex: none;
    place-items: center;
    width: calc(24 * var(--u));
    height: calc(24 * var(--u));
    padding: 0;
    border: 0;
    border-radius: calc(6 * var(--u));
    background: transparent;
    color: #a8a296;
    cursor: pointer;
    transition:
      color 0.15s,
      background-color 0.15s;
  }

  .search-clear:hover {
    background-color: rgba(255, 255, 255, 0.08);
    color: #edeae3;
  }

  .search-clear:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .search-clear svg {
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
  }

  /* ---- Lists ---- */
  .group-title {
    margin: 0 0 calc(10 * var(--u));
    font-size: calc(11 * var(--u));
    font-weight: 600;
    letter-spacing: 0.06em;
    line-height: calc(16 * var(--u));
    text-transform: uppercase;
    color: #a8a296;
  }

  .list {
    margin: 0;
    padding: calc(6 * var(--u));
    list-style: none;
  }

  .item {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto calc(80 * var(--u));
    align-items: center;
    gap: calc(16 * var(--u));
    box-sizing: border-box;
    width: 100%;
    padding: calc(10 * var(--u)) calc(12 * var(--u));
    border: 0;
    border-radius: calc(9 * var(--u));
    background: transparent;
    font: inherit;
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    text-align: left;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .item:hover,
  .item:focus-visible {
    background-color: rgba(255, 255, 255, 0.05);
  }

  .item:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: -2px;
  }

  .item-name {
    display: flex;
    gap: calc(6 * var(--u));
    min-width: 0;
  }

  .number {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: #a8a296;
  }

  .title {
    min-width: 0;
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: calc(11 * var(--u));
    color: #a8a296;
    white-space: nowrap;
  }

  .difficulty {
    display: inline-flex;
    align-items: center;
    gap: calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    color: #c9c2b4;
  }

  .dot {
    flex: none;
    width: calc(7 * var(--u));
    height: calc(7 * var(--u));
    border-radius: 50%;
    background-color: var(--c);
  }

  .message,
  .more {
    margin: 0;
    font-size: calc(13 * var(--u));
    line-height: calc(20 * var(--u));
    color: #a8a296;
  }

  .message {
    padding: calc(24 * var(--u)) calc(4 * var(--u));
  }

  .more {
    margin-top: calc(10 * var(--u));
    font-size: calc(12 * var(--u));
    color: #6f6a60;
  }

  /* Narrow window: the "when" column goes, so the name keeps its room. */
  @media (max-width: 640px) {
    .item {
      grid-template-columns: minmax(0, 1fr) calc(72 * var(--u));
      gap: calc(10 * var(--u));
    }

    .meta {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .intro,
    .search-box,
    .group {
      animation: none;
    }
  }
</style>
