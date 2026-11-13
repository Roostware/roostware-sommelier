// Hints for the Solve tab, revealed one step at a time.
//
// For now there is a single step: the problem's topic tags (when it has any). The catalog has
// no hints field (on leetcode.com those are a separate field the dataset doesn't include).
// The statement's "Follow-up" is deliberately NOT a hint: it stays in the statement where it
// is. Further steps can be added here, and rendered in ProblemStatement, without changing how
// the hint button works.

export type HintStep = { kind: "topics"; title: string; tags: string[] };

/** The steps to reveal, in order. Empty when there's nothing to hint at. */
export function hintSteps(tags: string[]): HintStep[] {
  return tags.length > 0 ? [{ kind: "topics", title: "Topics", tags }] : [];
}
