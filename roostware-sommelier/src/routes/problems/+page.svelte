<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import {
    listCollections,
    listProblems,
    type CollectionSummary,
    type Difficulty,
    type ProblemQuery,
    type ProblemSummary,
    type Progress,
    type SortKey as ApiSortKey,
    type StatusFilter as Status,
  } from "$lib/db";
  import { settings } from "$lib/settings.svelte";
  import { openProblem } from "$lib/solve.svelte";

  // ---- Search --------------------------------------------------------------
  // `searchText` is what's typed; `search` is what the list actually uses. Typing applies
  // after a short pause so each keystroke doesn't trigger a query; Enter and clearing apply
  // immediately. Matching is done by the database (title and problem number).
  let searchText = $state("");
  let search = $state("");
  let searchEl: HTMLInputElement | undefined = $state();

  $effect(() => {
    const text = searchText.trim();
    const timer = setTimeout(() => (search = text), 200);
    return () => clearTimeout(timer);
  });

  function clearSearch() {
    searchText = "";
    search = "";
    searchEl?.focus();
  }

  function onSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      search = searchText.trim();
    } else if (event.key === "Escape" && searchText) {
      event.preventDefault();
      clearSearch();
    }
  }

  // ---- Status filter -------------------------------------------------------

  const statuses: { value: Status; label: string }[] = [
    { value: "all", label: "All" },
    { value: "solved", label: "Solved" },
    { value: "unsolved", label: "Unsolved" },
  ];

  let status = $state<Status>("all");
  let statusIndex = $derived(statuses.findIndex((s) => s.value === status));

  // ---- Difficulty toggles --------------------------------------------------
  const difficulties = [
    { label: "Easy", color: "#3FAE7F" },
    { label: "Medium", color: "#EBA337" },
    { label: "Hard", color: "#FF5555" },
  ];

  // Every difficulty is shown until toggled off.
  let shown = $state<Record<string, boolean>>({ Easy: true, Medium: true, Hard: true });

  // ---- Collections dropdown ------------------------------------------------
  // The user's own collections (custom groups of problems; not LeetCode's topic tags).
  let collections = $state<CollectionSummary[]>([]);
  // The built-in (curated) lists only show while the "Show curated collections" setting is on.
  let visibleCollections = $derived(
    settings.showCuratedCollections ? collections : collections.filter((c) => !c.builtin),
  );

  let options = $derived<{ id: string | null; name: string; count: number | null }[]>([
    { id: null, name: "All collections", count: null },
    ...visibleCollections.map((c) => ({ id: c.id, name: c.name, count: c.problemCount })),
  ]);

  let collectionId = $state<string | null>(null);
  // Turning the setting off while one of those lists is selected goes back to all problems.
  $effect(() => {
    if (collectionId !== null && collections.length > 0 && !options.some((o) => o.id === collectionId)) {
      collectionId = null;
    }
  });
  let selectedOption = $derived(options.find((o) => o.id === collectionId) ?? options[0]);

  let open = $state(false);
  let activeIndex = $state(0);
  let dropdownEl: HTMLElement | undefined = $state();
  let buttonEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  async function openMenu() {
    open = true;
    activeIndex = Math.max(
      options.findIndex((o) => o.id === collectionId),
      0,
    );
    await tick();
    listEl?.focus();
  }

  function closeMenu(refocus = false) {
    open = false;
    if (refocus) buttonEl?.focus();
  }

  function selectOption(index: number) {
    collectionId = options[index].id;
    closeMenu(true);
  }

  function onButtonKeydown(event: KeyboardEvent) {
    if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      openMenu();
    }
  }

  function onListKeydown(event: KeyboardEvent) {
    const last = options.length - 1;
    switch (event.key) {
      case "ArrowDown":
        activeIndex = activeIndex === last ? 0 : activeIndex + 1;
        break;
      case "ArrowUp":
        activeIndex = activeIndex === 0 ? last : activeIndex - 1;
        break;
      case "Home":
        activeIndex = 0;
        break;
      case "End":
        activeIndex = last;
        break;
      case "Enter":
      case " ":
        selectOption(activeIndex);
        break;
      case "Escape":
        closeMenu(true);
        break;
      case "Tab":
        closeMenu();
        return;
      default:
        return;
    }
    event.preventDefault();
  }

  // ---- Problem list --------------------------------------------------------
  const progressLabels: Record<Progress, string> = {
    solved: "Solved",
    attempted: "Attempted, not yet solved",
    inProgress: "In progress, not yet submitted",
    untouched: "Not started",
  };

  const difficultyColor = Object.fromEntries(difficulties.map((d) => [d.label, d.color]));
  let collectionName = $derived(Object.fromEntries(visibleCollections.map((c) => [c.id, c.name])));

  // ---- Sorting -------------------------------------------------------------
  type SortKey = "name" | "difficulty" | "progress" | "solveCount";
  type SortDir = "asc" | "desc";

  const sortColumns: { key: SortKey; label: string; num?: boolean }[] = [
    { key: "name", label: "Name" },
    { key: "difficulty", label: "Difficulty" },
    { key: "progress", label: "Solved" },
    { key: "solveCount", label: "Solve count", num: true },
  ];

  // The name column leads with the problem number, so that's what it sorts on.
  const apiSortKey: Record<SortKey, ApiSortKey> = {
    name: "number",
    difficulty: "difficulty",
    progress: "progress",
    solveCount: "solveCount",
  };

  // No sort selected means the default order (by problem number).
  let sortKey = $state<SortKey | null>(null);
  let sortDir = $state<SortDir>("asc");

  // Clicking a column cycles ascending -> descending -> default order.
  function toggleSort(key: SortKey) {
    if (sortKey !== key) {
      sortKey = key;
      sortDir = "asc";
    } else if (sortDir === "asc") {
      sortDir = "desc";
    } else {
      sortKey = null;
    }
  }

  // ---- Loading -------------------------------------------------------------
  // Filtering, sorting and solved-progress all happen in the database. The list
  // is fetched a page at a time and grows as the user scrolls toward the bottom.
  const PAGE_SIZE = 100;

  let query = $derived<ProblemQuery>({
    filter: {
      query: search || null,
      // null means "any difficulty"; every toggle off is an empty list, which matches nothing.
      difficulties: difficulties.every((d) => shown[d.label])
        ? null
        : difficulties.filter((d) => shown[d.label]).map((d) => d.label as Difficulty),
      status,
      collectionId,
    },
    sort: sortKey === null ? undefined : { key: apiSortKey[sortKey], dir: sortDir },
  });

  let items = $state<ProblemSummary[]>([]);
  let total = $state(0);
  let loaded = $state(false);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let hasMore = $derived(items.length < total);

  // Bumped on every reload so a slow, outdated response can't overwrite newer results.
  let generation = 0;

  const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function reload(q: ProblemQuery) {
    const gen = ++generation;
    loadingMore = false;
    try {
      const page = await listProblems({ ...q, limit: PAGE_SIZE, offset: 0 });
      if (gen !== generation) return;
      items = page.items;
      total = page.total;
      error = null;
    } catch (e) {
      if (gen !== generation) return;
      items = [];
      total = 0;
      error = messageOf(e);
    }
    loaded = true;
  }

  async function loadMore() {
    if (!hasMore || loadingMore || error) return;
    const gen = generation;
    loadingMore = true;
    try {
      const page = await listProblems({ ...query, limit: PAGE_SIZE, offset: items.length });
      if (gen !== generation) return;
      items = [...items, ...page.items];
      // An empty page means the total was stale; stop asking instead of looping.
      total = page.items.length === 0 ? items.length : page.total;
    } catch (e) {
      if (gen !== generation) return;
      error = messageOf(e);
    }
    if (gen === generation) loadingMore = false;
  }

  // Re-run whenever a filter or the sort changes.
  $effect(() => {
    const q = query;
    untrack(() => reload(q));
  });

  onMount(async () => {
    try {
      collections = await listCollections();
    } catch {
      // The list below reports storage problems; the dropdown just stays at "All collections".
    }
  });

  // The page scrolls inside a container, not the window. An observer rooted at the window
  // would see the sentinel clipped by that container, and its rootMargin couldn't help.
  function scrollParent(el: HTMLElement): HTMLElement | null {
    for (let p = el.parentElement; p; p = p.parentElement) {
      if (/(auto|scroll)/.test(getComputedStyle(p).overflowY)) return p;
    }
    return null;
  }

  // Load the next page when the end of the list nears the viewport. The observer is
  // recreated after each page so it reports the current state straight away, which
  // keeps loading until the list overflows the screen.
  let sentinel: HTMLElement | undefined = $state();
  $effect(() => {
    if (!sentinel || !hasMore) return;
    void items.length;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) loadMore();
      },
      { root: scrollParent(sentinel), rootMargin: "600px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  });

  function onWindowPointerDown(event: PointerEvent) {
    if (open && dropdownEl && !dropdownEl.contains(event.target as Node)) closeMenu();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<div class="filters" role="group" aria-label="Problem filters">
  <div class="field search">
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
        aria-label="Search problems"
        autocomplete="off"
        spellcheck="false"
        bind:this={searchEl}
        bind:value={searchText}
        onkeydown={onSearchKeydown}
      />
      {#if searchText}
        <button type="button" class="search-clear" aria-label="Clear search" onclick={clearSearch}>
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
  </div>

  <div class="field">
    <div class="segmented" role="group" aria-label="Status" style="--i: {statusIndex}">
      <span class="segmented-indicator" aria-hidden="true"></span>
      {#each statuses as s}
        <button
          type="button"
          class="segment"
          class:active={status === s.value}
          aria-pressed={status === s.value}
          onclick={() => (status = s.value)}
        >
          {s.label}
        </button>
      {/each}
    </div>
  </div>

  <div class="field">
    <div class="toggles" role="group" aria-label="Difficulty">
      {#each difficulties as d}
        <button
          type="button"
          class="toggle"
          class:on={shown[d.label]}
          style="--c: {d.color}"
          aria-pressed={shown[d.label]}
          onclick={() => (shown[d.label] = !shown[d.label])}
        >
          <span class="toggle-dot"></span>
          {d.label}
        </button>
      {/each}
    </div>
  </div>

  <div class="field">
    <div class="dropdown" bind:this={dropdownEl}>
      <button
        type="button"
        class="dropdown-button"
        class:open
        bind:this={buttonEl}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="Collection: {selectedOption.name}"
        onclick={() => (open ? closeMenu() : openMenu())}
        onkeydown={onButtonKeydown}
      >
        <span class="dropdown-value">{selectedOption.name}</span>
        <svg
          class="chevron"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>

      {#if open}
        <ul
          class="menu card"
          role="listbox"
          tabindex="-1"
          aria-label="Collection"
          aria-activedescendant="collection-option-{activeIndex}"
          bind:this={listEl}
          onkeydown={onListKeydown}
        >
          {#each options as option, i}
            <!-- Keyboard handling lives on the listbox (aria-activedescendant pattern). -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <li
              id="collection-option-{i}"
              class="option"
              class:active={i === activeIndex}
              class:selected={option.id === collectionId}
              role="option"
              aria-selected={option.id === collectionId}
              onpointerenter={() => (activeIndex = i)}
              onclick={() => selectOption(i)}
            >
              <svg
                class="check"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="3"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M20 6 9 17l-5-5" />
              </svg>
              <span class="option-name">{option.name}</span>
              {#if option.count !== null}
                <span class="option-count">{option.count}</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</div>

<!-- Shared by every solved-indicator: the hatch fill for "attempted". -->
<svg class="defs" width="0" height="0" aria-hidden="true" focusable="false">
  <defs>
    <pattern id="hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <line x1="0" y1="0" x2="0" y2="4" stroke="#e9bc22" stroke-width="1.5" />
    </pattern>
    <pattern id="hatch-grey" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <line x1="0" y1="0" x2="0" y2="4" stroke="#a8a296" stroke-width="1.5" />
    </pattern>
  </defs>
</svg>

<div class="list" role="table" aria-label="Problems">
  <div class="list-header" role="row">
    {#each sortColumns as col}
      {@const active = sortKey === col.key}
      <span
        role="columnheader"
        class:num={col.num}
        aria-sort={active ? (sortDir === "asc" ? "ascending" : "descending") : undefined}
      >
        <button
          type="button"
          class="sort"
          class:active
          class:desc={active && sortDir === "desc"}
          onclick={() => toggleSort(col.key)}
        >
          {col.label}
          <svg
            class="sort-arrow"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="3"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M12 19V5M5 12l7-7 7 7" />
          </svg>
        </button>
      </span>
    {/each}
    <span role="columnheader">Collections</span>
  </div>

  {#each items as p (p.id)}
    <div class="row" role="row">
      <span class="cell name" role="cell">
        <button
          type="button"
          class="open"
          aria-label="Open problem {p.frontendId}: {p.title}"
          onclick={() => openProblem(p.id)}
        >
          <span class="number">{p.frontendId}.</span>
          <span class="title">{p.title}</span>
        </button>
      </span>

      <span class="cell difficulty" role="cell" style="--c: {difficultyColor[p.difficulty]}">
        <span class="dot"></span>
        {p.difficulty}
      </span>

      <span class="cell" role="cell">
        <span class="tip" data-tip={progressLabels[p.progress]}>
        <svg
          class="progress"
          viewBox="0 0 20 20"
          fill="none"
          role="img"
          aria-label={progressLabels[p.progress]}
        >
          {#if p.progress === "solved"}
            <circle cx="10" cy="10" r="9" fill="#e9bc22" />
            <path
              d="m6 10.3 2.8 2.8L14.2 7.6"
              stroke="#1d2027"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          {:else if p.progress === "attempted"}
            <circle cx="10" cy="10" r="8.25" fill="url(#hatch)" fill-opacity="0.55" />
            <circle
              cx="10"
              cy="10"
              r="8.25"
              stroke="#e9bc22"
              stroke-width="1.5"
              stroke-dasharray="1.5 3.2"
              stroke-linecap="round"
            />
          {:else if p.progress === "inProgress"}
            <!-- Like "attempted", but grey: unsubmitted code, no submissions yet. -->
            <circle cx="10" cy="10" r="8.25" fill="url(#hatch-grey)" fill-opacity="0.55" />
            <circle
              cx="10"
              cy="10"
              r="8.25"
              stroke="#a8a296"
              stroke-width="1.5"
              stroke-dasharray="1.5 3.2"
              stroke-linecap="round"
            />
          {:else}
            <circle cx="10" cy="10" r="8.25" stroke="#6f6a60" stroke-width="1.5" />
          {/if}
        </svg>
        </span>
      </span>

      <span class="cell num" role="cell">{p.solveCount}</span>

      <span class="cell collections" role="cell">
        {#each p.collectionIds.filter((id) => collectionName[id]) as id}
          <span class="chip">{collectionName[id]}</span>
        {:else}
          <span class="none" aria-label="No collections">—</span>
        {/each}
      </span>
    </div>
  {:else}
    {#if error}
      <p class="empty" role="alert">Couldn't load problems: {error}</p>
    {:else if !loaded}
      <p class="empty" role="status">Loading…</p>
    {:else}
      <p class="empty">No problems match these filters.</p>
    {/if}
  {/each}

  {#if hasMore}
    <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
    {#if loadingMore}
      <p class="empty" role="status">Loading more…</p>
    {/if}
  {/if}
</div>

<style>
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(calc(10 * var(--u)));
    }
  }

  .filters {
    /* Same color as the page behind the dashboard area. */
    --control-bg: #1d2027;

    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: calc(12 * var(--u)) calc(24 * var(--u));
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  /* ---- Problem list: header and rows share one column template ---- */
  .defs {
    position: absolute;
  }

  .list {
    --columns:
      minmax(0, 3fr) calc(90 * var(--u)) calc(70 * var(--u)) calc(100 * var(--u))
      minmax(0, 2fr);

    margin-top: calc(20 * var(--u));
  }

  .list-header,
  .row {
    display: grid;
    grid-template-columns: var(--columns);
    align-items: center;
    gap: calc(16 * var(--u));
    padding: calc(10 * var(--u)) calc(16 * var(--u));
  }

  .list-header {
    border-bottom: 1px solid var(--card-border);
    font-size: calc(11 * var(--u));
    font-weight: 500;
    line-height: calc(16 * var(--u));
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: #a8a296;
    animation: rise 0.45s 0.05s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .num {
    text-align: right;
  }

  .list-header > span {
    display: flex;
    align-items: center;
  }

  .list-header > .num {
    justify-content: flex-end;
  }

  /* Sortable column header: a button that inherits the header typography. */
  .sort {
    display: inline-flex;
    align-items: center;
    gap: calc(4 * var(--u));
    padding: 0;
    border: none;
    border-radius: calc(4 * var(--u));
    background: none;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    transition: color 0.2s;
  }

  .sort:hover,
  .sort.active {
    color: #edeae3;
  }

  /* The arrow's space is always reserved so headers don't shift when sorting. */
  .sort-arrow {
    flex: none;
    width: calc(11 * var(--u));
    height: calc(11 * var(--u));
    opacity: 0;
    transition:
      opacity 0.2s,
      transform 0.2s;
  }

  /* Right-aligned column: put the arrow before the label so the label's right edge
     lines up with the numbers below it. */
  .num .sort-arrow {
    order: -1;
  }

  .sort:hover .sort-arrow {
    opacity: 0.45;
  }

  .sort.active .sort-arrow {
    opacity: 1;
    color: #e9bc22;
  }

  .sort.desc .sort-arrow {
    transform: rotate(180deg);
  }

  .row {
    min-height: calc(40 * var(--u));
    box-sizing: border-box;
    border-bottom: 1px solid var(--card-border);
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    transition: background-color 0.15s;
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.2, 1) backwards;
  }

  .row:hover {
    background-color: rgba(255, 255, 255, 0.03);
  }

  .row {
    position: relative;
    cursor: pointer;
  }

  .cell {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .cell.num {
    justify-content: flex-end;
  }

  .name {
    display: flex;
    gap: calc(8 * var(--u));
  }

  /* The name is a real button (keyboard and assistive tech use it) that looks like plain
     text. Its ::after stretches over the whole row, so clicking anywhere on the row opens
     the problem without putting a click handler on a non-interactive element. */
  .open {
    display: flex;
    flex: 1;
    gap: calc(8 * var(--u));
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
  }

  .open::after {
    content: "";
    position: absolute;
    inset: 0;
  }

  .number {
    flex: none;
    min-width: calc(32 * var(--u));
    color: #a8a296;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }

  .difficulty {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    color: #c9c2b4;
  }

  .dot {
    flex: none;
    width: calc(8 * var(--u));
    height: calc(8 * var(--u));
    border-radius: 50%;
    background-color: var(--c);
  }

  /* Custom tooltip: any element with `data-tip`. Hover only; the text is also
     exposed to assistive tech through the element's own aria-label. */
  .tip {
    position: relative;
    display: inline-flex;
  }

  .tip::after {
    content: attr(data-tip);
    position: absolute;
    z-index: 5;
    bottom: calc(100% + 8 * var(--u));
    left: 50%;
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
    transform: translate(-50%, calc(4 * var(--u)));
    transition:
      opacity 0.15s,
      transform 0.15s;
  }

  .tip:hover::after {
    opacity: 1;
    transform: translate(-50%, 0);
    transition-delay: 0.25s;
  }

  .progress {
    display: block;
    width: calc(20 * var(--u));
    height: calc(20 * var(--u));
  }

  .row .num {
    font-variant-numeric: tabular-nums;
    color: #c9c2b4;
  }

  .collections {
    display: flex;
    flex-wrap: wrap;
    gap: calc(6 * var(--u));
  }

  .chip {
    padding: calc(2 * var(--u)) calc(8 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(6 * var(--u));
    background-color: var(--control-bg);
    font-size: calc(11 * var(--u));
    line-height: calc(16 * var(--u));
    color: #c9c2b4;
    white-space: nowrap;
  }

  .none {
    color: #6f6a60;
  }

  .sentinel {
    height: 1px;
  }

  .empty {
    margin: 0;
    padding: calc(32 * var(--u)) calc(16 * var(--u));
    text-align: center;
    color: #a8a296;
    font-size: calc(13 * var(--u));
  }

  /* Shared control surface, matching the cards. */
  .segmented,
  .toggle,
  .dropdown-button {
    border: 1px solid var(--card-border);
    background-color: var(--control-bg);
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
  }

  button {
    cursor: pointer;
  }

  button:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* ---- Search ---- */
  .search {
    flex: 1 1 calc(260 * var(--u));
    min-width: calc(220 * var(--u));
    max-width: calc(420 * var(--u));
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    box-sizing: border-box;
    padding: calc(8 * var(--u)) calc(12 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(10 * var(--u));
    background-color: var(--control-bg);
    transition: border-color 0.2s, box-shadow 0.2s;
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
    width: calc(15 * var(--u));
    height: calc(15 * var(--u));
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
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
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
    width: calc(20 * var(--u));
    height: calc(20 * var(--u));
    padding: 0;
    border: 0;
    border-radius: calc(5 * var(--u));
    background: transparent;
    color: #a8a296;
    transition: color 0.15s, background-color 0.15s;
  }

  .search-clear:hover {
    background-color: rgba(255, 255, 255, 0.08);
    color: #edeae3;
  }

  .search-clear svg {
    width: calc(11 * var(--u));
    height: calc(11 * var(--u));
  }

  /* ---- Status: segmented control with a sliding highlight ---- */
  .segmented {
    --pad: calc(3 * var(--u));

    position: relative;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    padding: var(--pad);
    border-radius: calc(10 * var(--u));
  }

  .segmented-indicator {
    position: absolute;
    top: var(--pad);
    bottom: var(--pad);
    left: var(--pad);
    width: calc((100% - 2 * var(--pad)) / 3);
    border-radius: calc(7 * var(--u));
    background-color: #e9bc22;
    transform: translateX(calc(var(--i) * 100%));
    transition: transform 0.3s cubic-bezier(0.34, 1.3, 0.64, 1);
  }

  .segment {
    position: relative;
    padding: calc(5 * var(--u)) calc(16 * var(--u));
    border: none;
    border-radius: calc(7 * var(--u));
    background: transparent;
    font: inherit;
    font-weight: 500;
    color: #a8a296;
    transition: color 0.2s;
  }

  .segment:hover {
    color: #edeae3;
  }

  /* Keep the weight identical in every state: a heavier active label would widen the
     control and shift everything next to it. */
  .segment.active,
  .segment.active:hover {
    color: #1d2027;
  }

  /* ---- Difficulty: independent on/off toggles ---- */
  .toggles {
    display: flex;
    gap: calc(8 * var(--u));
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    gap: calc(8 * var(--u));
    padding: calc(8 * var(--u)) calc(14 * var(--u));
    border-radius: calc(10 * var(--u));
    font-family: inherit;
    font-weight: 500;
    color: #a8a296;
    transition:
      background-color 0.2s,
      border-color 0.2s,
      color 0.2s;
  }

  .toggle:hover {
    color: #edeae3;
  }

  .toggle-dot {
    width: calc(8 * var(--u));
    height: calc(8 * var(--u));
    border-radius: 50%;
    background-color: var(--c);
    opacity: 0.35;
    transition: opacity 0.2s;
  }

  .toggle.on {
    border-color: color-mix(in srgb, var(--c) 45%, transparent);
    background-color: color-mix(in srgb, var(--c) 16%, var(--control-bg));
    color: #edeae3;
  }

  .toggle.on .toggle-dot {
    opacity: 1;
  }

  /* ---- Collections: custom dropdown ---- */
  .dropdown {
    position: relative;
  }

  .dropdown-button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    box-sizing: border-box;
    width: calc(220 * var(--u));
    padding: calc(8 * var(--u)) calc(12 * var(--u)) calc(8 * var(--u)) calc(14 * var(--u));
    border-radius: calc(10 * var(--u));
    font-family: inherit;
    font-weight: 500;
    color: #edeae3;
    text-align: left;
    transition: border-color 0.2s;
  }

  .dropdown-button:hover,
  .dropdown-button.open {
    border-color: rgba(255, 255, 255, 0.16);
  }

  .dropdown-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    flex: none;
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    color: #a8a296;
    transition: transform 0.2s;
  }

  .dropdown-button.open .chevron {
    transform: rotate(180deg);
  }

  .menu {
    position: absolute;
    z-index: 5;
    top: calc(100% + 6 * var(--u));
    left: 0;
    min-width: 100%;
    max-height: calc(300 * var(--u));
    margin: 0;
    padding: calc(4 * var(--u));
    overflow-y: auto;
    list-style: none;
    outline: none;
    animation: menu-in 0.15s ease-out;
  }

  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(calc(-4 * var(--u)));
    }
  }

  .option {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    padding: calc(8 * var(--u)) calc(10 * var(--u));
    border-radius: calc(8 * var(--u));
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    white-space: nowrap;
    cursor: pointer;
  }

  .option.active {
    background-color: rgba(255, 255, 255, 0.06);
    color: #edeae3;
  }

  .option.selected {
    color: #edeae3;
  }

  .check {
    flex: none;
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
    color: #e9bc22;
    visibility: hidden;
  }

  .option.selected .check {
    visibility: visible;
  }

  .option-name {
    flex: 1;
  }

  .option-count {
    font-size: calc(11 * var(--u));
    color: #a8a296;
  }

  /* ---- Narrower windows ------------------------------------------------------
     Columns drop out, least important first, instead of squeezing the name to nothing. The
     header and the rows share one template, so hiding the same column in both keeps them lined up. */
  @media (max-width: 1000px) {
    .list {
      --columns: minmax(0, 1fr) calc(90 * var(--u)) calc(70 * var(--u)) calc(100 * var(--u));
    }

    .list-header > :nth-child(5),
    .row > :nth-child(5) {
      display: none;
    }
  }

  @media (max-width: 680px) {
    .list {
      --columns: minmax(0, 1fr) calc(90 * var(--u)) calc(70 * var(--u));
    }

    .list-header > :nth-child(4),
    .row > :nth-child(4) {
      display: none;
    }

    /* The solved column is now the last one: its tooltip opens leftwards, so it stays in the page. */
    .tip::after {
      left: auto;
      right: calc(-8 * var(--u));
      transform: translate(0, calc(4 * var(--u)));
    }

    .tip:hover::after {
      transform: none;
    }
  }

  @media (max-width: 480px) {
    .list-header,
    .row {
      gap: calc(8 * var(--u));
      padding-left: calc(8 * var(--u));
      padding-right: calc(8 * var(--u));
    }

    .list {
      --columns: minmax(0, 1fr) calc(70 * var(--u)) calc(56 * var(--u));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .filters,
    .list-header,
    .row,
    .menu {
      animation: none;
    }

    .segmented-indicator {
      transition: none;
    }
  }
</style>
