<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isMac, isTauri } from "$lib/platform";

  // macOS keeps its native traffic-light buttons (overlay title bar), so the custom
  // controls are only drawn on Windows and Linux.
  const showControls = isTauri && !isMac;

  let maximized = $state(false);

  onMount(() => {
    if (!showControls) return;

    const appWindow = getCurrentWindow();
    const sync = async () => {
      maximized = await appWindow.isMaximized();
    };
    sync();

    let unlisten: (() => void) | undefined;
    let disposed = false;
    appWindow.onResized(sync).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  const minimize = () => getCurrentWindow().minimize();
  const toggleMaximize = () => getCurrentWindow().toggleMaximize();
  const close = () => getCurrentWindow().close();
</script>

<!-- The whole top strip drags the window; double-clicking it toggles maximize. -->
<div class="drag-region" data-tauri-drag-region></div>

{#if showControls}
  <div class="controls">
    <button type="button" class="control" aria-label="Minimize" onclick={minimize}>
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
    </button>
    <button
      type="button"
      class="control"
      aria-label={maximized ? "Restore" : "Maximize"}
      onclick={toggleMaximize}
    >
      <svg viewBox="0 0 10 10" aria-hidden="true">
        {#if maximized}
          <path d="M2.5 2.5V.5h7v7h-2" /><rect x="0.5" y="2.5" width="7" height="7" />
        {:else}
          <rect x="0.5" y="0.5" width="9" height="9" />
        {/if}
      </svg>
    </button>
    <button type="button" class="control close" aria-label="Close" onclick={close}>
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0 0 10" /></svg>
    </button>
  </div>
{/if}

<style>
  .drag-region {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--strip);
  }

  .controls {
    position: absolute;
    top: 0;
    right: 0;
    z-index: 3;
    display: flex;
  }

  .control {
    display: grid;
    place-items: center;
    width: 46px;
    height: 34px;
    padding: 0;
    border: none;
    background: transparent;
    color: #c9c2b4;
    cursor: default;
    transition: background-color 0.12s;
  }

  .control svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }

  .control:hover {
    background-color: rgba(255, 255, 255, 0.08);
  }

  .control:active {
    background-color: rgba(255, 255, 255, 0.05);
  }

  .control.close:hover {
    background-color: #e5484d;
    color: #ffffff;
  }

  .control:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: -2px;
  }
</style>
