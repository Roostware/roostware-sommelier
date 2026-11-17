<script lang="ts">
  // A read-only view of the code a submission was made with. It shows as plain text straight
  // away, then is syntax-highlighted with Monaco (the same colors as the editor) once that has loaded.
  import { monacoLanguage } from "$lib/languages";
  import { loadMonaco } from "$lib/monaco";

  let { code, lang }: { code: string; lang: string } = $props();

  let html = $state<string | null>(null);

  $effect(() => {
    const text = code;
    const language = monacoLanguage(lang);
    let stale = false;
    html = null;
    loadMonaco()
      .then((monaco) => monaco.editor.colorize(text, language, { tabSize: 4 }))
      .then((result) => {
        // Monaco writes every space as a non-breaking one and ends every line (the last too) with
        // <br/>. Copying that out would paste NBSPs (a SyntaxError in Python), so turn them back
        // into plain spaces and newlines; `white-space: pre` below renders them the same.
        if (!stale) {
          html = result
            .replaceAll(" ", " ")
            .replaceAll("&#160;", " ")
            .replaceAll("&nbsp;", " ")
            .replaceAll(/<br\s*\/?>/g, "\n")
            .replace(/\n$/, "");
        }
      })
      .catch(() => {
        // Highlighting is a nicety; the plain text is already showing.
      });
    return () => {
      stale = true;
    };
  });
</script>

<!-- Monaco escapes the code itself; the markup is only its token spans and line breaks. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region has to be reachable by keyboard) -->
<pre class="code" tabindex="0" aria-label="Submitted code">{#if html !== null}{@html html}{:else}{code}{/if}</pre>

<style>
  .code {
    box-sizing: border-box;
    max-height: calc(260 * var(--u));
    margin: calc(4 * var(--u)) 0 0;
    padding: calc(8 * var(--u)) calc(10 * var(--u));
    overflow: auto;
    border: 1px solid var(--card-border);
    border-radius: calc(8 * var(--u));
    background-color: #181b21;
    font-family: var(--font-code);
    font-size: calc(12 * var(--u));
    line-height: calc(18 * var(--u));
    color: #edeae3;
    tab-size: 4;
    /* Keeps the code's own indentation; long lines scroll instead of wrapping. */
    white-space: pre;
  }

  .code:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }
</style>
