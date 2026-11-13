// Loads Monaco (the editor VS Code is built on) the first time it's needed. It's large, so
// it's pulled in lazily and the rest of the app doesn't pay for it at startup. Everything is
// bundled with the app (no CDN), so the editor works offline.

import type * as Monaco from "monaco-editor";

let loading: Promise<typeof Monaco> | undefined;

/** Resolves to the Monaco API, loading and configuring it on first use. */
export function loadMonaco(): Promise<typeof Monaco> {
  loading ??= setUp().catch((error) => {
    loading = undefined; // allow a retry on the next open
    throw error;
  });
  return loading;
}

async function setUp(): Promise<typeof Monaco> {
  // Monaco runs its heavy lifting in web workers. Only JavaScript/TypeScript have a language
  // service (completions, hover); every other language is highlighting-only on the base worker.
  const [{ default: EditorWorker }, { default: TsWorker }] = await Promise.all([
    import("monaco-editor/editor/editor.worker?worker"),
    import("monaco-editor/language/typescript/ts.worker?worker"),
  ]);
  (globalThis as { MonacoEnvironment?: Monaco.Environment }).MonacoEnvironment = {
    getWorker: (_id, label) =>
      label === "typescript" || label === "javascript" ? new TsWorker() : new EditorWorker(),
  };

  const monaco = await import("monaco-editor");
  monaco.editor.defineTheme("sommelier", SOMMELIER_THEME);
  return monaco;
}

// The editor's look, drawn from the app's own palette: the brand yellow for keywords and the
// cursor, the Easy/Medium/Hard colors for strings/numbers, and the steel blue of the "Review"
// tag for types. Colors here are all in one place so the theme is easy to tune.
//
// Syntax `rules` match by token-name prefix, so "keyword" also covers "keyword.flow" and so
// on, across every language's grammar. Every color has at least 4.5:1 contrast against the
// background except line numbers, which are meant to recede.
const PALETTE = {
  background: "#1d2027", // the page behind the dashboard
  surface: "#262a33", // cards and popups
  text: "#e6e8ee",
  muted: "#a8a296",
  accent: "#e9bc22", // brand yellow
  green: "#62c79a", // from "Easy"
  orange: "#eba337", // from "Medium"
  red: "#ff8a8a", // softened "Hard"
  blue: "#9db4e8", // from the steel-blue "Review" tag
  teal: "#7ccfd0",
  warm: "#c9c2b4", // the app's secondary text
  comment: "#8d887b",
};

const SOMMELIER_THEME: Monaco.editor.IStandaloneThemeData = {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "", foreground: PALETTE.text.slice(1) },
    { token: "comment", foreground: PALETTE.comment.slice(1), fontStyle: "italic" },
    { token: "keyword", foreground: PALETTE.accent.slice(1) },
    { token: "string", foreground: PALETTE.green.slice(1) },
    { token: "string.escape", foreground: PALETTE.orange.slice(1) },
    { token: "number", foreground: PALETTE.orange.slice(1) },
    { token: "constant", foreground: PALETTE.orange.slice(1) },
    { token: "regexp", foreground: PALETTE.red.slice(1) },
    { token: "type", foreground: PALETTE.blue.slice(1) },
    { token: "namespace", foreground: PALETTE.blue.slice(1) },
    { token: "identifier", foreground: PALETTE.text.slice(1) },
    { token: "variable", foreground: PALETTE.text.slice(1) },
    { token: "variable.predefined", foreground: PALETTE.teal.slice(1) },
    { token: "predefined", foreground: PALETTE.teal.slice(1) },
    { token: "annotation", foreground: PALETTE.orange.slice(1) },
    { token: "meta", foreground: PALETTE.muted.slice(1) },
    { token: "operator", foreground: PALETTE.warm.slice(1) },
    { token: "delimiter", foreground: PALETTE.muted.slice(1) },
    { token: "tag", foreground: PALETTE.accent.slice(1) },
    { token: "attribute.name", foreground: PALETTE.blue.slice(1) },
    { token: "attribute.value", foreground: PALETTE.green.slice(1) },
    { token: "invalid", foreground: "ff5555" },
  ],
  colors: {
    "editor.background": PALETTE.background,
    "editor.foreground": PALETTE.text,
    "editorGutter.background": PALETTE.background,
    "editorLineNumber.foreground": "#6f6a60",
    "editorLineNumber.activeForeground": PALETTE.muted,
    "editor.lineHighlightBackground": "#ffffff0a",
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": "#e9bc2238",
    "editor.inactiveSelectionBackground": "#e9bc2220",
    "editor.selectionHighlightBackground": "#e9bc2220",
    "editor.wordHighlightBackground": "#ffffff14",
    "editor.wordHighlightStrongBackground": "#e9bc2230",
    "editor.findMatchBackground": "#e9bc2260",
    "editor.findMatchHighlightBackground": "#e9bc2830",
    "editorCursor.foreground": PALETTE.accent,
    "editorWhitespace.foreground": "#ffffff1c",
    "editorIndentGuide.background1": "#ffffff12",
    "editorIndentGuide.activeBackground1": "#ffffff30",
    "editorLink.activeForeground": PALETTE.accent,
    // Matching and nested brackets.
    "editorBracketMatch.background": "#e9bc2226",
    "editorBracketMatch.border": "#e9bc2266",
    "editorBracketHighlight.foreground1": PALETTE.accent,
    "editorBracketHighlight.foreground2": PALETTE.blue,
    "editorBracketHighlight.foreground3": PALETTE.green,
    "editorBracketHighlight.foreground4": PALETTE.orange,
    "editorBracketHighlight.foreground5": PALETTE.red,
    "editorBracketHighlight.foreground6": PALETTE.warm,
    "editorBracketHighlight.unexpectedBracket.foreground": "#ff5555",
    // Diagnostics (JavaScript/TypeScript only).
    "editorError.foreground": "#ff5555",
    "editorWarning.foreground": PALETTE.orange,
    "editorInfo.foreground": PALETTE.blue,
    // Popups: completions, hover, find.
    "focusBorder": PALETTE.accent,
    "editorWidget.background": PALETTE.surface,
    "editorWidget.border": "#ffffff14",
    "editorSuggestWidget.background": PALETTE.surface,
    "editorSuggestWidget.border": "#ffffff14",
    "editorSuggestWidget.foreground": PALETTE.text,
    "editorSuggestWidget.selectedBackground": "#ffffff14",
    "editorSuggestWidget.highlightForeground": PALETTE.accent,
    "editorSuggestWidget.focusHighlightForeground": PALETTE.accent,
    "editorHoverWidget.background": PALETTE.surface,
    "editorHoverWidget.border": "#ffffff14",
    "input.background": PALETTE.background,
    "input.border": "#ffffff1f",
    // The little icons beside completion items.
    "symbolIcon.methodForeground": PALETTE.blue,
    "symbolIcon.functionForeground": PALETTE.blue,
    "symbolIcon.constructorForeground": PALETTE.orange,
    "symbolIcon.fieldForeground": PALETTE.teal,
    "symbolIcon.propertyForeground": PALETTE.teal,
    "symbolIcon.variableForeground": PALETTE.teal,
    "symbolIcon.classForeground": PALETTE.orange,
    "symbolIcon.interfaceForeground": PALETTE.orange,
    "symbolIcon.enumeratorForeground": PALETTE.orange,
    "symbolIcon.constantForeground": PALETTE.orange,
    "symbolIcon.keywordForeground": PALETTE.accent,
    "symbolIcon.moduleForeground": PALETTE.muted,
    "symbolIcon.snippetForeground": PALETTE.muted,
    "symbolIcon.textForeground": PALETTE.muted,
    // Scrollbars match the app's slim dark ones.
    "scrollbarSlider.background": "#ffffff1f",
    "scrollbarSlider.hoverBackground": "#ffffff42",
    "scrollbarSlider.activeBackground": "#ffffff52",
  },
};
