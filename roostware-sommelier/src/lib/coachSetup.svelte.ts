// Whether the AI coach is set up: does it have what it needs to answer (a key, a model, an
// address, and the switch on)? The Rust side decides (`coach_status`) and says what's missing in a
// sentence; this just keeps the answer where the UI can read it. It also holds the key actions,
// since saving or removing a key changes the answer.
//
// A key is only ever sent *to* the backend. What comes back is whether one is saved, never the key.

import { callCoach } from "./coachProvider";
import type { CoachProviderName } from "./db";

export interface CoachStatus {
  ready: boolean;
  /** What to do, in a sentence, when not ready. */
  reason: string | null;
  /** The model that will be used (Anthropic's default when none is set). */
  model: string;
  /** Which providers have a key saved. */
  keys: Record<CoachProviderName, boolean>;
}

/** What a connection test found out. */
export interface ConnectionReport {
  model: string;
  usage: { inputTokens: number; outputTokens: number };
}

class CoachSetup {
  /** Null until first read. */
  status = $state<CoachStatus | null>(null);

  /** Whether the coach can answer. False until the status is known. */
  get ready(): boolean {
    return this.status?.ready === true;
  }

  /** What's missing, or null when ready (or not known yet). */
  get reason(): string | null {
    return this.status?.reason ?? null;
  }

  /** Reads the status again. Call it after anything that can change it. */
  async refresh() {
    try {
      this.status = await callCoach<CoachStatus>("coach_status");
    } catch (e) {
      // Outside the app there's no backend; the coach just isn't ready.
      console.warn("Couldn't read the coach's status:", e);
      this.status ??= null;
    }
  }

  /** Saves a provider's API key. Rejects with what's wrong with it. */
  async saveKey(provider: CoachProviderName, key: string) {
    await callCoach<void>("set_coach_key", { provider, key });
    await this.refresh();
  }

  async removeKey(provider: CoachProviderName) {
    await callCoach<void>("clear_coach_key", { provider });
    await this.refresh();
  }

  /** Sends the smallest possible request with the saved settings and key. Rejects with what went wrong. */
  testConnection(): Promise<ConnectionReport> {
    return callCoach<ConnectionReport>("test_coach_connection");
  }
}

export const coachSetup = new CoachSetup();
