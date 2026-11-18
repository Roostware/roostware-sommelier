// What happens when the user is done with a problem they just solved: the next visit starts clean.
// Run when they leave the Solve tab, and when they go back to the picker.

import { coach } from "$lib/coach.svelte";
import { leetcode } from "$lib/leetcode.svelte";
import { solve } from "$lib/solve.svelte";

/**
 * If the open problem's latest result is an Accepted submission: throws the accepted code away
 * (back to the starter code, saved too so it stays gone after a restart), gives its AI coach a
 * fresh start and clears its console. Leaving mid-attempt, or after editing the code past what was
 * accepted, keeps the code. Does nothing otherwise. Call before closing the problem, which
 * forgets the starter code. Returns whether it reset anything.
 */
export function resetIfSolved(): boolean {
  const id = solve.problemId;
  if (id === null) return false;
  const accepted = leetcode.acceptedThisSitting(id);
  if (!accepted) return false;
  const code = solve.codeFor(id, accepted.lang);
  if (code === undefined || code === accepted.code) solve.resetCode(id, accepted.lang);
  // A fresh start for the coach too: its chat is cleared and, with the problem's submissions
  // up to now set aside, it offers hints and questions again, as if the problem were unsolved.
  coach.newConversation(id, true);
  leetcode.reset(id);
  return true;
}
