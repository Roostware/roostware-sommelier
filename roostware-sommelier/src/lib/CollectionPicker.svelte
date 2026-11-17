<script lang="ts">
  // The button on a problem that lets the user file it under their own collections: tick a
  // collection to add the problem, untick it to take it out, or make a new collection for it.
  // A collection only exists while it has problems, so there is no "new empty collection", and
  // unticking the last problem in one deletes it (the list says so).
  import { onMount, tick } from "svelte";
  import {
    addProblemToCollection,
    createCollection,
    listCollections,
    removeProblemFromCollection,
    type CollectionSummary,
  } from "$lib/db";

  let {
    problemId,
    collectionIds,
    onchange,
  }: {
    problemId: number;
    /** The collections the problem is in right now. */
    collectionIds: string[];
    /** Called after every change, to refresh whatever shows the problem's collections. */
    onchange: () => Promise<void> | void;
  } = $props();

  const MAX_NAME = 60;

  let open = $state(false);
  let rootEl: HTMLElement | undefined = $state();
  let buttonEl: HTMLButtonElement | undefined = $state();
  let popoverEl: HTMLElement | undefined = $state();
  let nameEl: HTMLInputElement | undefined = $state();

  // The user's own collections: null until loaded (reloaded each time the popover opens, so it's
  // never stale). The app's built-in lists (Blind 75, …) are fixed, so they're not offered here.
  let collections = $state<CollectionSummary[] | null>(null);
  let builtinIds = $state(new Set<string>());
  let busy = $state(false);
  let error = $state<string | null>(null);

  let adding = $state(false); // the new-collection form is showing (always, when there are none)
  let name = $state("");

  let memberOf = $derived(new Set(collectionIds));
  /** How many of the user's own collections the problem is in. */
  let ownCount = $derived(collectionIds.filter((id) => !builtinIds.has(id)).length);
  let showForm = $derived(adding || (collections !== null && collections.length === 0));

  const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function load() {
    try {
      const all = await listCollections();
      builtinIds = new Set(all.filter((c) => c.builtin).map((c) => c.id));
      collections = all.filter((c) => !c.builtin);
    } catch (e) {
      error = messageOf(e);
    }
  }

  /** Focuses the first thing to act on: a collection to tick, or the name field. */
  async function focusFirst() {
    await tick();
    popoverEl?.querySelector<HTMLElement>("button:not(:disabled), input")?.focus();
  }

  async function openPopover() {
    open = true;
    error = null;
    adding = false;
    name = "";
    await load();
    await focusFirst();
  }

  function closePopover(refocus = false) {
    open = false;
    if (refocus) buttonEl?.focus();
  }

  /** Runs one change to the user's collections, then brings the list and the problem up to date. */
  async function change(action: () => Promise<unknown>): Promise<boolean> {
    busy = true;
    error = null;
    let ok = true;
    try {
      await action();
    } catch (e) {
      error = messageOf(e);
      ok = false;
    }
    // Even after a failure: it may mean the list was out of date.
    await load();
    try {
      await onchange();
    } catch (e) {
      console.warn("Couldn't refresh the problem after changing its collections:", e);
    }
    busy = false;
    return ok;
  }

  async function toggle(collection: CollectionSummary) {
    const act = memberOf.has(collection.id) ? removeProblemFromCollection : addProblemToCollection;
    await change(() => act(collection.id, problemId));
  }

  async function startAdding() {
    adding = true;
    await tick();
    nameEl?.focus();
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !name.trim()) return;
    if (await change(() => createCollection(name, problemId))) {
      name = "";
      adding = false;
      await focusFirst();
    } else {
      nameEl?.focus();
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      closePopover(true);
    }
  }

  function onWindowPointerDown(event: PointerEvent) {
    if (open && rootEl && !rootEl.contains(event.target as Node)) closePopover();
  }

  // Tabbing out of the popover closes it (a null target is just the window losing focus).
  function onFocusOut(event: FocusEvent) {
    const next = event.relatedTarget as Node | null;
    if (open && next && rootEl && !rootEl.contains(next)) closePopover();
  }

  let label = $derived(
    ownCount === 0 ? "Add to collection" : `In ${ownCount} ${ownCount === 1 ? "collection" : "collections"}`,
  );

  // The label needs to know which of the problem's collections are built in.
  onMount(() => void load());
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<!-- These handlers only catch Escape and focus leaving from the controls inside. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="picker" bind:this={rootEl} onkeydown={onKeydown} onfocusout={onFocusOut}>
  <span class="tip" class:hidden={open} data-tip="Add to or remove from your collections">
    <button
      type="button"
      class="picker-btn"
      class:on={ownCount > 0}
      aria-haspopup="dialog"
      aria-expanded={open}
      bind:this={buttonEl}
      onclick={() => (open ? closePopover() : openPopover())}
    >
      <svg
        viewBox="0 0 24 24"
        fill={ownCount > 0 ? "currentColor" : "none"}
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
      </svg>
      <span>{label}</span>
    </button>
  </span>

  {#if open}
    <div class="popover card" role="dialog" aria-label="Your collections" bind:this={popoverEl}>
      {#if collections === null}
        {#if !error}<p class="note" role="status">Loading…</p>{/if}
      {:else if collections.length > 0}
        <ul class="list">
          {#each collections as collection (collection.id)}
            {@const member = memberOf.has(collection.id)}
            <li>
              <button
                type="button"
                class="row"
                class:member
                role="checkbox"
                aria-checked={member}
                disabled={busy}
                onclick={() => toggle(collection)}
              >
                <span class="box" aria-hidden="true">
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="3.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  >
                    <path d="M20 6 9 17l-5-5" />
                  </svg>
                </span>
                <span class="name">{collection.name}</span>
                <span class="count" aria-label="{collection.problemCount} problems">{collection.problemCount}</span>
              </button>
            </li>
          {/each}
        </ul>
        <p class="note">A collection with no problems is deleted.</p>
      {/if}

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      {#if collections !== null}
        <div class="new" class:divided={collections.length > 0}>
          {#if showForm}
            <form onsubmit={create}>
              <input
                type="text"
                placeholder="New collection name"
                aria-label="New collection name"
                maxlength={MAX_NAME}
                autocomplete="off"
                spellcheck="false"
                disabled={busy}
                bind:value={name}
                bind:this={nameEl}
              />
              <button type="submit" class="create" disabled={busy || !name.trim()}>Create</button>
            </form>
            <p class="note form-note">The new collection will start with this problem.</p>
          {:else}
            <button type="button" class="row add" disabled={busy} onclick={startAdding}>
              <span class="box plus" aria-hidden="true">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="3"
                  stroke-linecap="round"
                >
                  <path d="M12 5v14" />
                  <path d="M5 12h14" />
                </svg>
              </span>
              <span class="name">New collection</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }

  .picker-btn {
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
    transition: border-color 0.2s;
  }

  .picker-btn svg {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    color: #a8a296;
  }

  .picker-btn.on svg {
    color: #e9bc22;
  }

  .picker-btn:hover,
  .picker-btn[aria-expanded="true"] {
    border-color: rgba(233, 188, 34, 0.5);
  }

  .picker-btn:focus-visible,
  .row:focus-visible,
  .create:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* Custom tooltip (like the rest of the app), below and aligned to the right edge. It steps
     aside while the popover is open, which sits in the same spot. */
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

  .tip.hidden::after {
    display: none;
  }

  /* ---- The popover ---- */
  .popover {
    position: absolute;
    z-index: 7;
    top: calc(100% + 6 * var(--u));
    right: 0;
    box-sizing: border-box;
    width: calc(270 * var(--u));
    padding: calc(4 * var(--u));
    animation: popover-in 0.15s ease-out;
  }

  @keyframes popover-in {
    from {
      opacity: 0;
      transform: translateY(calc(-4 * var(--u)));
    }
  }

  .list {
    max-height: calc(240 * var(--u));
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }

  .list li {
    margin: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    width: 100%;
    box-sizing: border-box;
    padding: calc(8 * var(--u)) calc(10 * var(--u));
    border: 0;
    border-radius: calc(8 * var(--u));
    background: none;
    font: inherit;
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
    color: #c9c2b4;
    text-align: left;
    cursor: pointer;
  }

  .row:hover:not(:disabled) {
    background-color: rgba(255, 255, 255, 0.06);
    color: #edeae3;
  }

  .row:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .row.member {
    color: #edeae3;
  }

  .box {
    flex: none;
    display: grid;
    place-items: center;
    width: calc(16 * var(--u));
    height: calc(16 * var(--u));
    box-sizing: border-box;
    border: 1.5px solid #6f6b62;
    border-radius: calc(4 * var(--u));
    color: transparent;
    transition:
      background-color 0.15s,
      border-color 0.15s;
  }

  .box svg {
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
  }

  .row.member .box {
    border-color: #e9bc22;
    background-color: #e9bc22;
    color: #1d2027;
  }

  .box.plus {
    border-style: dashed;
    color: #a8a296;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex: none;
    font-size: calc(11 * var(--u));
    font-variant-numeric: tabular-nums;
    color: #a8a296;
  }

  .note {
    margin: 0;
    padding: calc(4 * var(--u)) calc(10 * var(--u)) calc(6 * var(--u));
    font-size: calc(11 * var(--u));
    line-height: calc(16 * var(--u));
    color: #a8a296;
  }

  .error {
    margin: 0;
    padding: calc(4 * var(--u)) calc(10 * var(--u)) calc(6 * var(--u));
    font-size: calc(12 * var(--u));
    line-height: calc(16 * var(--u));
    color: #ff8a8a;
  }

  .new.divided {
    border-top: 1px solid var(--card-border);
  }

  .new form {
    display: flex;
    gap: calc(6 * var(--u));
    padding: calc(6 * var(--u)) calc(6 * var(--u)) 0;
  }

  .new input {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    padding: calc(6 * var(--u)) calc(10 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #12141a;
    font: inherit;
    font-size: calc(13 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
  }

  .new input::placeholder {
    color: #a8a296;
  }

  .new input:focus-visible {
    border-color: rgba(233, 188, 34, 0.6);
    outline: none;
  }

  .create {
    flex: none;
    padding: calc(6 * var(--u)) calc(12 * var(--u));
    border: 1px solid transparent;
    border-radius: calc(8 * var(--u));
    background-color: #e9bc22;
    font: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 600;
    line-height: calc(18 * var(--u));
    color: #1d2027;
    cursor: pointer;
  }

  .create:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .form-note {
    padding-top: calc(6 * var(--u));
  }

  @media (prefers-reduced-motion: reduce) {
    .popover {
      animation: none;
    }

    .tip::after,
    .picker-btn,
    .box {
      transition: none;
    }
  }
</style>
