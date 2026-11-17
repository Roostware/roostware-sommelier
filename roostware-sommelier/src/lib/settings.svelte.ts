// The user's preferences, shared by everything that depends on them (the dashboard's daily set,
// the problem list's collections). Module state: loaded once at startup, then updated here
// first and saved behind it, so the UI answers at once. A save that fails puts the old value back.

import { coachSetup } from "$lib/coachSetup.svelte";
import {
  getSettings,
  setCoachSettings,
  setDailyProblemCount,
  setShowCuratedCollections,
  MAX_DAILY_PROBLEMS,
  MAX_MODEL_NAME_LENGTH,
  MIN_DAILY_PROBLEMS,
  type CoachProviderName,
  type CoachSettings,
} from "$lib/db";

export const settings = $state({
  /** False until the saved values have arrived; the ones below are the defaults until then. */
  loaded: false,
  dailyProblemCount: 3,
  /** The daily count as last saved. Follows `dailyProblemCount` once the save lands, so whatever asks the backend for the set can wait for it. */
  savedDailyProblemCount: 3,
  showCuratedCollections: false,
  /** Whether the AI coach shows in the Solve tab. */
  coachEnabled: true,
  coachProvider: "anthropic" as CoachProviderName,
  coachModel: "",
  coachBaseUrl: "",
  /** What the user pays per million tokens (US dollars), if they've said. */
  coachPriceInput: null as number | null,
  coachPriceOutput: null as number | null,
  /** The last save that failed, for the settings menu to show. */
  error: null as string | null,
});

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export async function loadSettings(): Promise<void> {
  try {
    const saved = await getSettings();
    settings.dailyProblemCount = saved.dailyProblemCount;
    settings.savedDailyProblemCount = saved.dailyProblemCount;
    settings.showCuratedCollections = saved.showCuratedCollections;
    settings.coachEnabled = saved.coach.enabled;
    settings.coachProvider = saved.coach.provider;
    settings.coachModel = saved.coach.model;
    settings.coachBaseUrl = saved.coach.baseUrl;
    settings.coachPriceInput = saved.coach.priceInput;
    settings.coachPriceOutput = saved.coach.priceOutput;
  } catch (e) {
    // Outside the app (a plain browser tab) there is no storage; the defaults stand.
    console.warn("Couldn't load the settings:", e);
  }
  settings.loaded = true;
}

export async function updateDailyProblemCount(count: number): Promise<void> {
  const next = Math.min(MAX_DAILY_PROBLEMS, Math.max(MIN_DAILY_PROBLEMS, Math.round(count)));
  const before = settings.dailyProblemCount;
  if (next === before) return;
  settings.dailyProblemCount = next;
  settings.error = null;
  try {
    await setDailyProblemCount(next);
    settings.savedDailyProblemCount = next;
  } catch (e) {
    if (settings.dailyProblemCount === next) settings.dailyProblemCount = before;
    settings.error = messageOf(e);
  }
}

const coachSnapshot = (): CoachSettings => ({
  enabled: settings.coachEnabled,
  provider: settings.coachProvider,
  model: settings.coachModel,
  baseUrl: settings.coachBaseUrl,
  priceInput: settings.coachPriceInput,
  priceOutput: settings.coachPriceOutput,
});

function showCoach(c: CoachSettings) {
  settings.coachEnabled = c.enabled;
  settings.coachProvider = c.provider;
  settings.coachModel = c.model;
  settings.coachBaseUrl = c.baseUrl;
  settings.coachPriceInput = c.priceInput;
  settings.coachPriceOutput = c.priceOutput;
}

/**
 * Changes any of the AI coach's settings (they're saved together). Shown at once, and put back if
 * the save fails. Whether the coach is ready can change with any of them, so that's read again.
 */
export async function updateCoachSettings(change: Partial<CoachSettings>): Promise<void> {
  const before = coachSnapshot();
  const next: CoachSettings = {
    ...before,
    ...change,
    model: (change.model ?? before.model).trim().slice(0, MAX_MODEL_NAME_LENGTH),
    baseUrl: (change.baseUrl ?? before.baseUrl).trim(),
  };
  if (JSON.stringify(next) === JSON.stringify(before)) return;
  showCoach(next);
  settings.error = null;
  try {
    await setCoachSettings(next);
    await coachSetup.refresh();
  } catch (e) {
    // Only undo what's still showing this attempt, not a newer change made meanwhile.
    if (JSON.stringify(coachSnapshot()) === JSON.stringify(next)) showCoach(before);
    settings.error = messageOf(e);
  }
}

export async function updateShowCuratedCollections(show: boolean): Promise<void> {
  const before = settings.showCuratedCollections;
  if (show === before) return;
  settings.showCuratedCollections = show;
  settings.error = null;
  try {
    await setShowCuratedCollections(show);
  } catch (e) {
    if (settings.showCuratedCollections === show) settings.showCuratedCollections = before;
    settings.error = messageOf(e);
  }
}
