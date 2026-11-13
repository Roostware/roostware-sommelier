<script lang="ts">
  // A Monaco editor (the editor VS Code is built on). It's created once with the given text
  // and language and then owns its content, so to show a different document the parent
  // re-creates it (with `{#key}`) rather than changing props.
  import { onMount } from "svelte";
  import type * as Monaco from "monaco-editor";
  import { loadMonaco } from "$lib/monaco";

  let {
    value,
    language,
    onchange,
  }: {
    /** Initial text. Later changes to this prop are ignored. */
    value: string;
    /** Monaco language id for highlighting. */
    language: string;
    onchange: (value: string) => void;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let status = $state<"loading" | "ready" | "error">("loading");
  let errorMessage = $state("");

  // ---- Text size follows the editor's width ----------------------------------------
  // The text is at most MAX_FONT_SIZE (it never grows with the window, however big) and
  // shrinks in proportion as the editor gets narrower, so more code stays on screen and
  // there's less sideways scrolling. Full size needs an editor FULL_SIZE_WIDTH wide, which is
  // wider than the default window gives, so the default window already runs a little smaller.
  // It stops shrinking at MIN_FONT_SIZE so code stays readable.
  const MAX_FONT_SIZE = 14;
  const MIN_FONT_SIZE = 10;
  const MAX_LINE_HEIGHT = 22;
  const MAX_PADDING = 14;
  /** Editor width (px) at which the text reaches full size (a window about 1700px wide). */
  const FULL_SIZE_WIDTH = 760;

  /** The font size, line height and padding for the editor's current width. */
  function metrics() {
    const fit = host && host.clientWidth > 0 ? host.clientWidth / FULL_SIZE_WIDTH : 1;
    const scale = Math.max(MIN_FONT_SIZE / MAX_FONT_SIZE, Math.min(fit, 1));
    const fontSize = Math.round(MAX_FONT_SIZE * scale); // whole pixels keep monospace text crisp
    return {
      fontSize,
      lineHeight: Math.round((fontSize * MAX_LINE_HEIGHT) / MAX_FONT_SIZE),
      padding: { top: Math.round(MAX_PADDING * scale), bottom: Math.round(MAX_PADDING * scale) },
    };
  }

  onMount(() => {
    let disposed = false;
    let editor: Monaco.editor.IStandaloneCodeEditor | undefined;
    let observer: ResizeObserver | undefined;
    let frame = 0;

    loadMonaco()
      .then((monaco) => {
        if (disposed || !host) return;
        const initial = metrics();
        editor = monaco.editor.create(host, {
          value,
          language,
          theme: "sommelier",
          automaticLayout: true, // follows the panel as the window resizes
          fixedOverflowWidgets: true, // suggestions/hover aren't clipped by the panel edge
          minimap: { enabled: false },
          ...initial,
          fontFamily: "'Cascadia Code', 'Cascadia Mono', Consolas, 'Courier New', monospace",
          scrollBeyondLastLine: false,
          tabSize: 4,
          insertSpaces: true,
          renderWhitespace: "selection",
          smoothScrolling: true,
          cursorSmoothCaretAnimation: "on",
          bracketPairColorization: { enabled: true },
          overviewRulerLanes: 0,
          hideCursorInOverviewRuler: true,
          scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
        });
        editor.onDidChangeModelContent(() => onchange(editor!.getValue()));

        // Re-fit the text whenever the editor's size changes (window resize, maximize, …).
        let applied = initial.fontSize;
        observer = new ResizeObserver(() => {
          cancelAnimationFrame(frame);
          frame = requestAnimationFrame(() => {
            const next = metrics();
            if (next.fontSize === applied) return;
            applied = next.fontSize;
            editor?.updateOptions(next);
          });
        });
        observer.observe(host);

        status = "ready";
        editor.focus();
      })
      .catch((e) => {
        if (disposed) return;
        errorMessage = e instanceof Error ? e.message : String(e);
        status = "error";
      });

    return () => {
      disposed = true;
      observer?.disconnect();
      cancelAnimationFrame(frame);
      const model = editor?.getModel();
      editor?.dispose();
      model?.dispose();
    };
  });
</script>

<div class="wrap">
  <div class="editor" bind:this={host} aria-label="Code editor"></div>
  {#if status === "loading"}
    <p class="overlay" role="status">Loading editor…</p>
  {:else if status === "error"}
    <p class="overlay" role="alert">Couldn't load the editor: {errorMessage}</p>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    min-height: 0;
    height: 100%;
  }

  .editor {
    height: 100%;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    padding: 24px;
    background-color: #1d2027;
    font-size: 13px;
    color: #a8a296;
    text-align: center;
  }
</style>
