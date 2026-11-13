<script module lang="ts">
  /** A single-colour glyph, drawn with the text colour. */
  export type DropdownIcon = {
    /** SVG path data. */
    d: string;
    /** The path's coordinate box. Defaults to "0 0 24 24". */
    viewBox?: string;
    /** Outline width (in viewBox units) to thicken a thin glyph. */
    stroke?: number;
  };

  export type DropdownOption = { value: string; label: string; icon?: DropdownIcon };

  let instances = 0;
</script>

<script lang="ts">
  // A custom listbox dropdown, styled and behaving like the collection dropdown on the
  // Problems tab: a button with a chevron that opens a card-styled menu, with a check mark on
  // the selected option, full keyboard support, and closing on outside click.
  import { tick } from "svelte";

  let {
    options,
    value,
    onselect,
    label,
    width = 220,
  }: {
    options: DropdownOption[];
    /** The selected option's `value`. */
    value: string;
    onselect: (value: string) => void;
    /** What the dropdown chooses, for assistive tech (e.g. "Programming language"). */
    label: string;
    /** Button width, in the app's scaled units. */
    width?: number;
  } = $props();

  const id = `dropdown-${++instances}`;

  let open = $state(false);
  let activeIndex = $state(0);
  let rootEl: HTMLElement | undefined = $state();
  let buttonEl: HTMLButtonElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  let selected = $derived(options.find((o) => o.value === value) ?? options[0]);

  // With many options the menu scrolls, so keep the highlighted one in view.
  function scrollActiveIntoView() {
    listEl?.querySelector(`#${id}-option-${activeIndex}`)?.scrollIntoView({ block: "nearest" });
  }

  async function openMenu() {
    open = true;
    activeIndex = Math.max(
      options.findIndex((o) => o.value === value),
      0,
    );
    await tick();
    listEl?.focus();
    scrollActiveIntoView();
  }

  function closeMenu(refocus = false) {
    open = false;
    if (refocus) buttonEl?.focus();
  }

  function selectOption(index: number) {
    onselect(options[index].value);
    closeMenu(true);
  }

  function onButtonKeydown(event: KeyboardEvent) {
    if (!open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      openMenu();
    }
  }

  async function onListKeydown(event: KeyboardEvent) {
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
    await tick();
    scrollActiveIntoView();
  }

  function onWindowPointerDown(event: PointerEvent) {
    if (open && rootEl && !rootEl.contains(event.target as Node)) closeMenu();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

{#snippet glyph(icon: DropdownIcon)}
  <svg
    class="glyph"
    viewBox={icon.viewBox ?? "0 0 24 24"}
    fill="currentColor"
    stroke={icon.stroke ? "currentColor" : undefined}
    stroke-width={icon.stroke}
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path d={icon.d} />
  </svg>
{/snippet}

<div class="dropdown" bind:this={rootEl}>
  <button
    type="button"
    class="dropdown-button"
    class:open
    style="width: calc({width} * var(--u))"
    bind:this={buttonEl}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label="{label}: {selected.label}"
    onclick={() => (open ? closeMenu() : openMenu())}
    onkeydown={onButtonKeydown}
  >
    <span class="dropdown-value">
      {#if selected.icon}{@render glyph(selected.icon)}{/if}
      <span class="dropdown-label">{selected.label}</span>
    </span>
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
      aria-label={label}
      aria-activedescendant="{id}-option-{activeIndex}"
      bind:this={listEl}
      onkeydown={onListKeydown}
    >
      {#each options as option, i (option.value)}
        <!-- Keyboard handling lives on the listbox (aria-activedescendant pattern). -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{id}-option-{i}"
          class="option"
          class:active={i === activeIndex}
          class:selected={option.value === value}
          role="option"
          aria-selected={option.value === value}
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
          {#if option.icon}{@render glyph(option.icon)}{/if}
          <span class="option-name">{option.label}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .dropdown {
    position: relative;
  }

  .dropdown-button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(12 * var(--u));
    box-sizing: border-box;
    padding: calc(8 * var(--u)) calc(12 * var(--u)) calc(8 * var(--u)) calc(14 * var(--u));
    border: 1px solid var(--card-border);
    border-radius: calc(10 * var(--u));
    background-color: var(--control-bg, #1d2027);
    font-family: inherit;
    font-size: calc(13 * var(--u));
    font-weight: 500;
    line-height: calc(18 * var(--u));
    color: #edeae3;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.2s;
  }

  .dropdown-button:hover,
  .dropdown-button.open {
    border-color: rgba(255, 255, 255, 0.16);
  }

  .dropdown-button:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  .dropdown-value {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    min-width: 0;
  }

  .dropdown-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Icons are drawn in the text colour, a step dimmer until highlighted or chosen. */
  .glyph {
    flex: none;
    width: calc(16 * var(--u));
    height: calc(16 * var(--u));
    color: #a8a296;
    transition: color 0.15s;
  }

  .dropdown-button .glyph,
  .option.active .glyph,
  .option.selected .glyph {
    color: #edeae3;
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

  @media (prefers-reduced-motion: reduce) {
    .menu {
      animation: none;
    }

    .chevron {
      transition: none;
    }
  }
</style>
