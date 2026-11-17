// A line of the coach's text can point at lines of the user's code ("see L7"). The code editor
// registers itself here while it exists, and the coach's text calls `show` to have it scroll to
// and flash those lines. Kept as its own small module so neither side has to know the other.
//
// Line numbers only mean something for the code they were written about. A message remembers
// that code, and `linesMatch` says whether the lines it points at still read the same in the
// editor: if the user has changed them (or moved them), the reference is switched off rather than
// pointing at the wrong place. Editing elsewhere doesn't switch it off.

import { citedLines } from "./coachText";

type Handler = {
  /** Scrolls to and highlights lines `from` to `to` (1-based, inclusive). */
  show: (from: number, to: number) => void;
  /** What's in the editor right now. */
  code: () => string;
};

class CoachJump {
  // Raw: the editor's functions are kept as they are (a deep proxy would break comparing them).
  #handler = $state.raw<Handler | null>(null);
  /** Bumped when the editor's content changes, so what depends on `linesMatch` re-reads it. */
  #edits = $state(0);

  /** Whether an editor is there to show lines in. */
  get available(): boolean {
    return this.#handler !== null;
  }

  /** The editor says it can show lines. Returns what to call when it goes away. */
  register(handler: Handler): () => void {
    this.#handler = handler;
    return () => {
      if (this.#handler === handler) this.#handler = null;
    };
  }

  /** The editor's content changed. */
  changed() {
    this.#edits++;
  }

  /** Scrolls to and highlights lines `from` to `to` of the user's code. */
  show(from: number, to: number) {
    this.#handler?.show(from, to);
  }

  /**
   * Whether lines `from` to `to` of the editor still read as they did in `code`, the code a message
   * was written about, so a reference to them is still right.
   */
  linesMatch(code: string | null | undefined, from: number, to: number): "same" | "changed" | "missing" | "none" {
    void this.#edits;
    if (code == null || this.#handler === null) return "none";
    return citedLines(code, this.#handler.code(), from, to);
  }
}

export const coachJump = new CoachJump();
