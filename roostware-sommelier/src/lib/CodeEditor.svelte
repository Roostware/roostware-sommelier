<script lang="ts">
  // A Monaco editor (the editor VS Code is built on). It's created once with the given text
  // and language and then owns its content, so to show a different document the parent
  // re-creates it (with `{#key}`) rather than changing props.
  import { onMount } from "svelte";
  import type * as Monaco from "monaco-editor";
  import { loadMonaco } from "$lib/monaco";
  import { coachJump } from "$lib/coachJump.svelte";

  /** How long lines the coach points at stay highlighted. */
  const HIGHLIGHT_MS = 3500;

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
    let unregisterJump: (() => void) | undefined;
    let highlightTimer: ReturnType<typeof setTimeout> | undefined;
    // Where each underlined line's extent is set (see the coach's `show` below).
    const underlineRules = document.createElement("style");
    document.head.append(underlineRules);

    // Monaco measures character widths when the editor is created, so the bundled font has to
    // be loaded first or the cursor and selections would be misaligned. If it can't load, the
    // fallback fonts in the stack are used.
    const fontReady = Promise.all([
      document.fonts.load(`${MAX_FONT_SIZE}px "Cascadia Code"`),
      document.fonts.load(`italic ${MAX_FONT_SIZE}px "Cascadia Code"`),
    ]).catch(() => undefined);

    Promise.all([loadMonaco(), fontReady])
      .then(([monaco]) => {
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
        // The coach can point at lines of the code: scroll them into view and underline them.
        // The underline is the AI gradient, drawn under the text of each line (not the whole width).
        // Monaco splits a line's text into a span per token, which would restart a gradient on every
        // token, so each line gets one whole-line decoration instead, and a style rule that trims it
        // to where the text starts and ends. The code is monospaced, so that's just a column count.
        const highlights = editor.createDecorationsCollection();
        let shown: { start: number; end: number } | null = null;

        // `draw` is whether the underlines are drawn in (the first time) or just there (re-fitted
        // after the text changed size, which must not replay the drawing).
        const paintUnderlines = (draw = true) => {
          const model = editor?.getModel();
          if (!editor || !model || !shown) return;
          // Lines are drawn one after another, the whole thing taking at most about 0.6s.
          const stagger = Math.min(70, 600 / Math.max(1, shown.end - shown.start));
          let drawnSoFar = 0;
          const charWidth = editor.getOption(monaco.editor.EditorOption.fontInfo).typicalHalfwidthCharacterWidth;
          const tabSize = model.getOptions().tabSize;
          const columns = (text: string) => {
            let width = 0;
            for (const ch of text) width += ch === "\t" ? tabSize - (width % tabSize) : 1;
            return width;
          };
          const decorations: Monaco.editor.IModelDeltaDecoration[] = [];
          let rules = "";
          for (let line = shown.start; line <= shown.end; line++) {
            const text = model.getLineContent(line).trimEnd();
            const indent = text.length - text.trimStart().length;
            if (text.length === indent) continue; // blank: nothing to underline
            const left = columns(text.slice(0, indent)) * charWidth;
            const width = columns(text) * charWidth - left;
            // The delay is for the drawing, the first of the two animations (the other is the colour drift).
            const delay = draw ? `animation-delay:${Math.round(drawnSoFar++ * stagger)}ms,0s!important;` : "";
            rules += `.coach-underline-${line}{left:${left}px!important;width:${width}px!important;${delay}}\n`;
            decorations.push({
              range: new monaco.Range(line, 1, line, 1),
              options: { isWholeLine: true, className: `coach-underline coach-underline-${line}${draw ? "" : " coach-underline-still"}` },
            });
          }
          underlineRules.textContent = rules;
          highlights.set(decorations);
        };

        const clearUnderlines = () => {
          shown = null;
          highlights.clear();
          underlineRules.textContent = "";
        };

        editor.onDidChangeModelContent(() => {
          onchange(editor!.getValue());
          coachJump.changed(); // line references in the coach's text may no longer fit
          if (shown) clearUnderlines(); // the lines have moved on; the underline would no longer fit
        });
        editor.onDidChangeConfiguration((e) => {
          if (e.hasChanged(monaco.editor.EditorOption.fontInfo)) paintUnderlines(false); // the text got bigger or smaller
        });

        unregisterJump = coachJump.register({
          code: () => editor?.getValue() ?? "",
          show: (from, to) => {
            const model = editor?.getModel();
            if (!editor || !model) return;
            const last = model.getLineCount();
            const start = Math.min(Math.max(1, from), last);
            const end = Math.min(Math.max(start, to), last);
            editor.revealLinesInCenterIfOutsideViewport(start, end, monaco.editor.ScrollType.Smooth);
            // Put the cursor on the first line (not a selection: typing next must not replace the code).
            editor.setPosition({ lineNumber: start, column: model.getLineFirstNonWhitespaceColumn(start) || 1 });
            editor.focus();
            shown = { start, end };
            paintUnderlines();
            clearTimeout(highlightTimer);
            highlightTimer = setTimeout(clearUnderlines, HIGHLIGHT_MS);
          },
        });

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
      unregisterJump?.();
      clearTimeout(highlightTimer);
      underlineRules.remove();
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

  /* Lines the coach points at: an underline in the AI gradient, drawn in from the left and then
     drifting gently. Monaco puts the class on its own elements, so it has to be global. The script
     sets each line's left and width (to fit the text) and, for the drawing, a delay per line; the
     line the underline sits on is the element's bottom edge. */
  :global(.coach-underline) {
    background-image: var(--ai-flow);
    background-position: 0 calc(100% - 1px);
    background-repeat: repeat-x;
    background-size: 200% 2px;
    border-radius: 1px;
    opacity: 0.85;
    animation:
      coach-underline-draw 0.5s cubic-bezier(0.3, 0.6, 0.2, 1) backwards,
      ai-flow 12s linear infinite;
  }

  /* Already drawn (the text was resized under it): just keep drifting. */
  :global(.coach-underline-still) {
    animation: ai-flow 12s linear infinite;
  }

  /* Opens the element from its left edge, so the line is drawn out to exactly the width of the text. */
  @keyframes -global-coach-underline-draw {
    from {
      clip-path: inset(0 100% 0 0);
    }
    to {
      clip-path: inset(0 0 0 0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.coach-underline),
    :global(.coach-underline-still) {
      animation: none;
    }
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
