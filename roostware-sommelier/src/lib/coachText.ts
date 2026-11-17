// A tiny, safe formatter for what the coach writes: paragraphs, "- " lists, fenced code, and
// **bold**, *italic* and `code` inside them, plus references to lines of the user's code (which the
// UI turns into buttons that highlight those lines in the editor).
//
// Models are asked to write references as [L7] or [L7-9], but they drift, so a reference is also
// understood as [Line 7], [L7–9] (any dash), [L7, L9], a bare "L7" in backticks or bold, and, when
// the code is known, plain prose like "line 7", "lines 7-9" or "lines 3 and 5". They are found
// inside bold, italic and parentheses too, and wrapped in markdown ("**[L7]**", "*L7*"). Prose is only
// linked when every number is a line that exists, so "line 40 of the statement" stays text. An
// array like [1, 2, 3] is never mistaken for one: a reference has to say it's a line.
//
// It produces plain data that Svelte renders as text, never HTML, so whatever a model writes can't
// inject markup.

export type Inline =
  | { t: "text"; s: string }
  | { t: "bold"; parts: Inline[] } // bold and italic can hold references (and each other)
  | { t: "em"; parts: Inline[] }
  | { t: "code"; s: string }
  | { t: "line"; from: number; to: number };

export type Block =
  | { type: "p"; parts: Inline[] }
  | { type: "list"; items: Inline[][] }
  | { type: "code"; code: string };

export interface ParseOptions {
  /**
   * How many lines the code the text is about has (0 or missing when there's none). Prose such as
   * "line 7" is only turned into a reference when it names lines within this.
   */
  lines?: number;
}

const DASH = String.raw`[-‐-―−]`;
const PROSE = String.raw`\blines?\s+\d+(?:\s*${DASH}\s*\d+)?(?:\s*(?:,|;|&|and)\s*\d+(?:\s*${DASH}\s*\d+)?)*`;
const INLINE_SOURCE = String.raw`\*\*(.+?)\*\*|\*([^*\s][^*]*)\*|\x60([^\x60]+)\x60|[\[(]([^\])\n]{1,40})[\])]|(${PROSE})`;
const ONE_REF = new RegExp(String.raw`^(?:(?:lines?|l)\s*)?(\d{1,6})(?:\s*${DASH}\s*(?:(?:lines?|l)\s*)?(\d{1,6}))?$`, "i");
const SAYS_LINE = /^(?:lines?|l)\s*\d/i;

/** "L7", "Line 7", "L7-9", "L7, L9", "lines 3 and 5": the lines they name, or null if this isn't a reference. */
function parseRefs(text: string): { from: number; to: number }[] | null {
  const parts = text.split(/\s*(?:,|;|&|\band\b)\s*/i).map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0 || !SAYS_LINE.test(parts[0])) return null;
  const refs: { from: number; to: number }[] = [];
  for (const part of parts) {
    const m = ONE_REF.exec(part);
    if (!m) return null;
    const a = Number(m[1]);
    const b = m[2] === undefined ? a : Number(m[2]);
    if (a < 1 || b < 1) return null;
    refs.push({ from: Math.min(a, b), to: Math.max(a, b) });
  }
  return refs;
}

function refParts(refs: { from: number; to: number }[]): Inline[] {
  return refs.flatMap((r, i): Inline[] => [...(i > 0 ? [{ t: "text", s: ", " } as Inline] : []), { t: "line", from: r.from, to: r.to }]);
}

/**
 * A reference with the markdown and brackets around it taken off, plus any punctuation that was
 * inside them: "**[L7]**", "`L7`", "[**L7**]", "**Line 7:**" are all line 7 (the last with a ":" after).
 */
function bareRefs(styled: string): { refs: { from: number; to: number }[]; tail: string } | null {
  const m = /^[\s[(]*(.*?)[\s\])]*([:.,;!?]*)$/.exec(styled.replace(/[*\x60]/g, ""));
  const refs = m && parseRefs(m[1]);
  return m && refs ? { refs, tail: m[2] } : null;
}

/**
 * Splits one line of text into its styled pieces. Bold and italic are read recursively, so a
 * reference inside them ("**the loop at [L7-9]**") is still found, and so is one inside a
 * parenthesis ("(see [L7])"), which is only treated as a reference when it is one itself.
 */
export function parseInline(text: string, options: ParseOptions = {}): Inline[] {
  const lines = options.lines ?? 0;
  const parts: Inline[] = [];
  const inline = new RegExp(INLINE_SOURCE, "gi"); // its own cursor: this calls itself for bold and italic
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = inline.exec(text)) !== null) {
    if (m.index > last) parts.push({ t: "text", s: text.slice(last, m.index) });
    last = m.index + m[0].length;

    const styled = m[1] ?? m[2] ?? m[3];
    if (styled !== undefined) {
      // **L7**, *L7* and `L7` (with or without brackets, and a trailing colon) are references too.
      const bare = bareRefs(styled);
      if (bare) parts.push(...refParts(bare.refs), ...(bare.tail ? [{ t: "text", s: bare.tail } as Inline] : []));
      else if (m[3] !== undefined) parts.push({ t: "code", s: m[3] });
      else parts.push({ t: m[1] !== undefined ? "bold" : "em", parts: parseInline(styled, options) });
    } else if (m[4] !== undefined) {
      // [L7] or (L7): a reference if it says it's a line, otherwise it's just brackets.
      const refs = parseRefs(m[4].replace(/[*\x60]/g, "").trim());
      if (refs) {
        parts.push(...refParts(refs));
      } else {
        // Not one. Keep only the opening bracket and read on from just after it, so whatever is
        // inside ("(see [L7])", "(**L7**)") is still looked at.
        parts.push({ t: "text", s: m[0][0] });
        last = m.index + 1;
        inline.lastIndex = last;
      }
    } else {
      // Prose: "line 7". Only when the code is known and has those lines.
      const refs = parseRefs(m[5]);
      if (refs && lines > 0 && refs.every((r) => r.to <= lines)) {
        // The word "line(s)" stays as written; the numbers become buttons.
        const word = /^lines?\s+/i.exec(m[5])?.[0] ?? "";
        parts.push({ t: "text", s: word }, ...refParts(refs));
      } else {
        parts.push({ t: "text", s: m[0] });
      }
    }
  }
  if (last < text.length) parts.push({ t: "text", s: text.slice(last) });
  return mergeText(parts);
}

/** Neighbouring plain text joined, so "[1, 2]" that was split while scanning is one piece again. */
function mergeText(parts: Inline[]): Inline[] {
  const out: Inline[] = [];
  for (const p of parts) {
    const prev = out[out.length - 1];
    if (p.t === "text" && prev?.t === "text") prev.s += p.s;
    else out.push(p);
  }
  return out;
}

/** Splits the coach's text into blocks. An unclosed code fence runs to the end (the text may still be arriving). */
export function parseBlocks(text: string, options: ParseOptions = {}): Block[] {
  const blocks: Block[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let paragraph: string[] = [];
  let list: Inline[][] = [];
  let code: string[] | null = null;

  const endParagraph = () => {
    if (paragraph.length > 0) blocks.push({ type: "p", parts: parseInline(paragraph.join(" "), options) });
    paragraph = [];
  };
  const endList = () => {
    if (list.length > 0) blocks.push({ type: "list", items: list });
    list = [];
  };

  for (const line of lines) {
    if (code !== null) {
      if (line.trim().startsWith("```")) {
        blocks.push({ type: "code", code: code.join("\n") });
        code = null;
      } else code.push(line);
      continue;
    }
    if (line.trim().startsWith("```")) {
      endParagraph();
      endList();
      code = [];
    } else if (/^\s*[-•*]\s+/.test(line)) {
      endParagraph();
      list.push(parseInline(line.replace(/^\s*[-•*]\s+/, ""), options));
    } else if (line.trim() === "") {
      endParagraph();
      endList();
    } else {
      endList();
      paragraph.push(line.trim());
    }
  }
  if (code !== null) blocks.push({ type: "code", code: code.join("\n") });
  endParagraph();
  endList();
  return blocks;
}

/** A code's lines, as a person compares them: no line-ending or trailing-space differences. */
function linesOf(code: string): string[] {
  return code.replace(/\r\n/g, "\n").split("\n").map((l) => l.replace(/[ \t]+$/, ""));
}

/**
 * Whether lines `from` to `to` (1-based) still read the same in `now` as in `was`, the code a
 * message was written about. A reference to lines that haven't changed is still right, however much
 * was edited elsewhere (unless that moved them, which this also catches, since it compares the lines
 * at those numbers). "missing" means the message cites lines the code never had.
 */
export function citedLines(was: string, now: string, from: number, to: number): "same" | "changed" | "missing" {
  const before = linesOf(was);
  // A trailing newline is not a line of its own.
  const count = before.length > 1 && before[before.length - 1] === "" ? before.length - 1 : before.length;
  if (from < 1 || from > count) return "missing";
  const after = linesOf(now);
  for (let n = from; n <= Math.min(to, count); n++) {
    if (before[n - 1] !== after[n - 1]) return "changed";
  }
  return "same";
}
