<script lang="ts">
  // The settings button in the bottom-left corner of the sidebar, and the modal it opens.
  // The values live in settings.svelte.ts; changing one here saves it straight away. The modal
  // is a native <dialog>, so the browser handles Escape, keeping focus inside it, and putting it
  // above everything else. Its settings are in tabs: General, the AI Coach, and the LeetCode account.
  import { tick } from "svelte";
  import { asCoachError } from "$lib/coachProvider";
  import { coachSetup } from "$lib/coachSetup.svelte";
  import { COACH_NAME } from "$lib/coachName";
  import { compactTokens } from "$lib/coachUsage";
  import { MAX_DAILY_PROBLEMS, MAX_MODEL_NAME_LENGTH, MIN_DAILY_PROBLEMS, resetUserData, type CoachProviderName } from "$lib/db";
  import { signOutOfLeetCode } from "$lib/leetcode";
  import {
    settings,
    updateCoachSettings,
    updateDailyProblemCount,
    updateShowCuratedCollections,
  } from "$lib/settings.svelte";
  import { leetcode } from "$lib/leetcode.svelte";
  import { settingsDialog, type SettingsTab } from "$lib/settingsDialog.svelte";

  const TABS: { id: SettingsTab; label: string }[] = [
    { id: "general", label: "General" },
    { id: "coach", label: "AI Coach" },
    { id: "leetcode", label: "LeetCode" },
  ];

  const PROVIDERS: { value: CoachProviderName; label: string }[] = [
    { value: "anthropic", label: "Anthropic" },
    { value: "openai", label: "OpenAI" },
    { value: "other", label: "Other" },
  ];

  const MODEL_HINT: Record<CoachProviderName, string> = {
    anthropic: "Leave empty for the default (claude-opus-5-5)",
    openai: "The model's name",
    other: "The model's name on your server",
  };

  let open = $state(false);
  let dialogEl: HTMLDialogElement | undefined = $state();
  let tab = $state<SettingsTab>("general");

  function openModal() {
    settings.error = null;
    if (leetcode.status === null) void leetcode.load(); // for the LeetCode tab
    dialogEl?.showModal();
    open = true;
  }

  // Something elsewhere in the app asked for the settings (the coach, when it isn't set up yet),
  // maybe on a particular tab. If the dialog is already open it just changes tab.
  let seenRequests = settingsDialog.requested;
  $effect(() => {
    const requested = settingsDialog.requested;
    if (requested === seenRequests) return;
    seenRequests = requested;
    if (settingsDialog.tab) tab = settingsDialog.tab;
    if (!dialogEl?.open) openModal();
  });

  /** Arrow keys, Home and End move between the tabs (and take focus with them), as in any tab strip. */
  async function onTabsKeydown(event: KeyboardEvent) {
    const at = TABS.findIndex((t) => t.id === tab);
    const to =
      event.key === "ArrowRight" ? (at + 1) % TABS.length
      : event.key === "ArrowLeft" ? (at - 1 + TABS.length) % TABS.length
      : event.key === "Home" ? 0
      : event.key === "End" ? TABS.length - 1
      : -1;
    if (to < 0) return;
    event.preventDefault();
    tab = TABS[to].id;
    await tick();
    document.getElementById(`tab-${tab}`)?.focus();
  }

  // ---- The AI coach's key and connection ----
  let keyDraft = $state("");
  let keyBusy = $state(false);
  let keyError = $state<string | null>(null);
  let testing = $state(false);
  let testResult = $state<{ ok: boolean; text: string } | null>(null);

  let hasKey = $derived(coachSetup.status?.keys[settings.coachProvider] ?? false);
  /** A local server needs no key; the others can't be reached without one. */
  let keyOptional = $derived(settings.coachProvider === "other");

  // A different provider, model or address means whatever was shown about the last one is stale.
  $effect(() => {
    void [settings.coachProvider, settings.coachModel, settings.coachBaseUrl];
    keyDraft = "";
    keyError = null;
    testResult = null;
  });

  async function saveKey() {
    if (keyBusy || keyDraft.trim() === "") return;
    keyBusy = true;
    keyError = null;
    testResult = null;
    try {
      await coachSetup.saveKey(settings.coachProvider, keyDraft);
      keyDraft = "";
    } catch (e) {
      keyError = asCoachError(e).message;
    } finally {
      keyBusy = false;
    }
  }

  async function removeKey() {
    if (keyBusy) return;
    keyBusy = true;
    keyError = null;
    testResult = null;
    try {
      await coachSetup.removeKey(settings.coachProvider);
    } catch (e) {
      keyError = asCoachError(e).message;
    } finally {
      keyBusy = false;
    }
  }

  async function testConnection() {
    if (testing) return;
    testing = true;
    testResult = null;
    try {
      const report = await coachSetup.testConnection();
      testResult = {
        ok: true,
        text: `Connected. ${report.model} answered (${compactTokens(report.usage.inputTokens)} tokens in, ${compactTokens(report.usage.outputTokens)} out).`,
      };
    } catch (e) {
      testResult = { ok: false, text: asCoachError(e).message };
    } finally {
      testing = false;
    }
  }

  // ---- Starting over ----
  // Erasing everything can't be undone, so it takes two steps: the button opens the list of what goes,
  // and the word has to be typed.
  const RESET_WORD = "RESET";
  let resetStep = $state<"idle" | "confirming" | "working">("idle");
  let resetWord = $state("");
  /** Also forget the LeetCode login and the API keys (they live in the system's credential store, not the data). */
  let resetLogins = $state(true);
  let resetError = $state<string | null>(null);
  let resetArmed = $derived(resetWord.trim().toUpperCase() === RESET_WORD);

  function startReset() {
    resetStep = "confirming";
    resetWord = "";
    resetLogins = true;
    resetError = null;
  }

  function cancelReset() {
    if (resetStep === "working") return;
    resetStep = "idle";
    resetWord = "";
    resetError = null;
  }

  async function runReset() {
    if (resetStep !== "confirming" || !resetArmed) return;
    resetStep = "working";
    resetError = null;
    try {
      // The logins first: if one can't be removed, nothing of the data has gone yet, so it can be tried again.
      if (resetLogins) {
        await signOutOfLeetCode();
        for (const p of PROVIDERS) await coachSetup.removeKey(p.value);
      }
      // Keeping the keys means keeping the provider they belong to, or they couldn't be found again.
      await resetUserData(!resetLogins);
      // Start fresh from the home page: everything the app holds in memory was about the old data.
      window.location.replace("/");
    } catch (e) {
      resetError = e instanceof Error ? e.message : typeof e === "object" && e !== null && "message" in e ? String(e.message) : String(e);
      resetStep = "confirming";
    }
  }

  /** A price field: blank means "not set"; anything else must be a number from 0 up. */
  function savePrice(which: "priceInput" | "priceOutput", input: HTMLInputElement) {
    const text = input.value.trim();
    const value = text === "" ? null : Number(text);
    if (value !== null && (!Number.isFinite(value) || value < 0)) {
      input.value = String((which === "priceInput" ? settings.coachPriceInput : settings.coachPriceOutput) ?? "");
      return;
    }
    void updateCoachSettings({ [which]: value });
  }

  // A click on the dim area around the card lands on the dialog itself, not on anything inside.
  function onDialogClick(event: MouseEvent) {
    if (event.target === dialogEl) dialogEl?.close();
  }
</script>

<div class="settings">
  <button
    type="button"
    class="settings-btn"
    aria-label="Settings"
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={openModal}
  >
    <svg
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path
        d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
      />
      <circle cx="12" cy="12" r="3" />
    </svg>
    {#if !open}<span class="tooltip" role="tooltip">Settings</span>{/if}
  </button>

  <dialog
    class="modal card"
    bind:this={dialogEl}
    aria-labelledby="settings-title"
    onclose={() => {
      open = false;
      cancelReset(); // a half-finished confirmation shouldn't be waiting the next time it opens
    }}
    onclick={onDialogClick}
  >
    <div class="panel">
      <header>
        <h2 id="settings-title">Settings</h2>
        <button type="button" class="close" aria-label="Close settings" onclick={() => dialogEl?.close()}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </header>

      <div class="tabs" role="tablist" aria-label="Settings sections" tabindex="-1" onkeydown={onTabsKeydown}>
        {#each TABS as t (t.id)}
          <button
            type="button"
            role="tab"
            id="tab-{t.id}"
            class="tab"
            class:ai={t.id === "coach"}
            aria-selected={tab === t.id}
            aria-controls="settings-panel"
            tabindex={tab === t.id ? 0 : -1}
            onclick={() => (tab = t.id)}>{t.label}</button
          >
        {/each}
      </div>

      <div class="body" role="tabpanel" id="settings-panel" aria-labelledby="tab-{tab}">
        {#if tab === "general"}
        <div class="setting">
          <div class="text">
            <span class="label" id="curated-label">Show curated collections</span>
            <span class="help" id="curated-help">Blind 75, NeetCode 150 and the other built-in lists, on the Problems page.</span>
          </div>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-checked={settings.showCuratedCollections}
            aria-labelledby="curated-label"
            aria-describedby="curated-help"
            onclick={() => updateShowCuratedCollections(!settings.showCuratedCollections)}
          >
            <span class="knob"></span>
          </button>
        </div>

        <div class="setting">
          <div class="text">
            <span class="label" id="daily-label">Problems per day</span>
            <span class="help" id="daily-help">How many problems Sommelier recommends on the Home page each day.</span>
          </div>
          <div class="stepper" role="group" aria-labelledby="daily-label" aria-describedby="daily-help">
            <button
              type="button"
              aria-label="Fewer problems per day"
              disabled={settings.dailyProblemCount <= MIN_DAILY_PROBLEMS}
              onclick={() => updateDailyProblemCount(settings.dailyProblemCount - 1)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true">
                <path d="M5 12h14" />
              </svg>
            </button>
            <output aria-live="polite">{settings.dailyProblemCount}</output>
            <button
              type="button"
              aria-label="More problems per day"
              disabled={settings.dailyProblemCount >= MAX_DAILY_PROBLEMS}
              onclick={() => updateDailyProblemCount(settings.dailyProblemCount + 1)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true">
                <path d="M12 5v14" />
                <path d="M5 12h14" />
              </svg>
            </button>
          </div>
        </div>

        <div class="setting col danger">
          <div class="text">
            <span class="label">Reset the app</span>
            <span class="help">Erases everything you've saved here and starts over, as if you had just installed it.</span>
          </div>
          {#if resetStep === "idle"}
            <div class="key-row">
              <button type="button" class="small-btn danger-btn" onclick={startReset}>Reset…</button>
            </div>
          {:else}
            <form
              class="reset-box"
              aria-label="Confirm resetting the app"
              onsubmit={(e) => {
                e.preventDefault();
                void runReset();
              }}
            >
              <p class="reset-lead">This permanently deletes:</p>
              <ul>
                <li>your code, drafts and every submission</li>
                <li>your rating, reviews and daily recommendations</li>
                <li>your collections, notes and settings</li>
                <li>every saved conversation with {COACH_NAME}</li>
              </ul>
              <p class="help">
                The problem list stays, and the built-in collections come back. Your LeetCode account itself isn't
                affected. This can't be undone.
              </p>
              <label class="check">
                <input type="checkbox" bind:checked={resetLogins} disabled={resetStep === "working"} />
                <span>
                  Also sign out of LeetCode and remove my {COACH_NAME} setup (API keys, provider and model)
                  <span class="help">Untick this to keep your LeetCode login and your {COACH_NAME} setup as they are.</span>
                </span>
              </label>
              <label class="text" for="reset-word">
                <span class="help">Type <strong>{RESET_WORD}</strong> to confirm</span>
              </label>
              <input
                id="reset-word"
                class="field"
                type="text"
                spellcheck="false"
                autocomplete="off"
                maxlength="20"
                placeholder={RESET_WORD}
                bind:value={resetWord}
                disabled={resetStep === "working"}
              />
              {#if resetError}
                <p class="error" role="alert">Couldn't finish resetting: {resetError}</p>
              {/if}
              <div class="key-row">
                <button type="button" class="small-btn" disabled={resetStep === "working"} onclick={cancelReset}>Cancel</button>
                <button type="submit" class="small-btn danger-btn strong" disabled={!resetArmed || resetStep === "working"}>
                  {#if resetStep === "working"}<span class="spinner" aria-hidden="true"></span>{/if}Erase everything
                </button>
              </div>
            </form>
          {/if}
        </div>
        {:else if tab === "coach"}
        <div class="setting">
          <div class="text">
            <span class="label ai-text" id="coach-label">{COACH_NAME}, your AI coach</span>
            <span class="help" id="coach-help">Hints, debugging help and feedback on the Solve page. Everything else works without it.</span>
          </div>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-checked={settings.coachEnabled}
            aria-labelledby="coach-label"
            aria-describedby="coach-help"
            onclick={() => updateCoachSettings({ enabled: !settings.coachEnabled })}
          >
            <span class="knob"></span>
          </button>
        </div>

        {#if settings.coachEnabled}
          <div class="setting col">
            <div class="text">
              <span class="label" id="provider-label">Provider</span>
              <span class="help">What you ask sends the problem, your code and the conversation to this provider.</span>
            </div>
            <div class="segments" role="radiogroup" aria-labelledby="provider-label">
              {#each PROVIDERS as p (p.value)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={settings.coachProvider === p.value}
                  onclick={() => updateCoachSettings({ provider: p.value })}>{p.label}</button
                >
              {/each}
            </div>
            {#if settings.coachProvider === "other"}
              <span class="help">Any server that speaks OpenAI's chat API: OpenRouter, or a local Ollama or LM Studio.</span>
            {/if}
          </div>

          {#if settings.coachProvider === "other"}
            <div class="setting col">
              <label class="text" for="coach-url">
                <span class="label">Server address</span>
                <span class="help">The base URL, for example http://localhost:11434/v1</span>
              </label>
              <input
                id="coach-url"
                class="field"
                type="url"
                spellcheck="false"
                autocomplete="off"
                maxlength="300"
                placeholder="http://localhost:11434/v1"
                value={settings.coachBaseUrl}
                onchange={(e) => updateCoachSettings({ baseUrl: e.currentTarget.value })}
              />
            </div>
          {/if}

          <div class="setting col">
            <label class="text" for="coach-model">
              <span class="label">Model</span>
              <span class="help">{MODEL_HINT[settings.coachProvider]}</span>
            </label>
            <input
              id="coach-model"
              class="field"
              type="text"
              spellcheck="false"
              autocomplete="off"
              maxlength={MAX_MODEL_NAME_LENGTH}
              placeholder={settings.coachProvider === "anthropic" ? "claude-opus-5-5" : "Model name"}
              value={settings.coachModel}
              onchange={(e) => updateCoachSettings({ model: e.currentTarget.value })}
            />
          </div>

          <div class="setting col">
            <label class="text" for="coach-key">
              <span class="label">API key{keyOptional ? " (optional)" : ""}</span>
              <span class="help">
                Kept in your system's credential store, never in the app's data, and it can't be read back out of the app.
                {#if keyOptional}A local server usually needs none.{/if}
              </span>
            </label>
            {#if hasKey}
              <div class="key-row">
                <span class="key-saved" role="status"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>A key is saved</span>
                <button type="button" class="small-btn" disabled={keyBusy} onclick={removeKey}>Remove</button>
              </div>
            {:else}
              <form
                class="key-row"
                onsubmit={(e) => {
                  e.preventDefault();
                  void saveKey();
                }}
              >
                <input
                  id="coach-key"
                  class="field"
                  type="password"
                  spellcheck="false"
                  autocomplete="off"
                  maxlength="500"
                  placeholder="Paste your key"
                  bind:value={keyDraft}
                />
                <button type="submit" class="small-btn strong" disabled={keyBusy || keyDraft.trim() === ""}>Save</button>
              </form>
            {/if}
            {#if keyError}
              <p class="error" role="alert">{keyError}</p>
            {/if}
          </div>

          <div class="setting col">
            <div class="text">
              <span class="label">Connection</span>
              <span class="help">
                {#if coachSetup.status?.reason}{coachSetup.status.reason}{:else}Sends one tiny request to check the key, model and address work.{/if}
              </span>
            </div>
            <div class="key-row">
              <button type="button" class="small-btn strong" disabled={testing || !coachSetup.ready} onclick={testConnection}>
                {#if testing}<span class="spinner" aria-hidden="true"></span>{/if}Test connection
              </button>
            </div>
            {#if testResult}
              <p class={testResult.ok ? "ok" : "error"} role={testResult.ok ? "status" : "alert"}>{testResult.text}</p>
            {/if}
          </div>

          <div class="setting col">
            <div class="text">
              <span class="label">What it costs (optional)</span>
              <span class="help">Your model's price in US dollars per million tokens, so {COACH_NAME} can show what a conversation cost. Leave blank to see tokens only.</span>
            </div>
            <div class="prices">
              <label>
                <span class="help">Input</span>
                <input
                  class="field"
                  type="number"
                  min="0"
                  step="any"
                  inputmode="decimal"
                  placeholder="$ / 1M"
                  value={settings.coachPriceInput ?? ""}
                  onchange={(e) => savePrice("priceInput", e.currentTarget)}
                />
              </label>
              <label>
                <span class="help">Output</span>
                <input
                  class="field"
                  type="number"
                  min="0"
                  step="any"
                  inputmode="decimal"
                  placeholder="$ / 1M"
                  value={settings.coachPriceOutput ?? ""}
                  onchange={(e) => savePrice("priceOutput", e.currentTarget)}
                />
              </label>
            </div>
          </div>
        {/if}
        {:else}
        <div class="setting col">
          <div class="text">
            <span class="label">LeetCode account</span>
            <span class="help">
              Needed to run and submit your code. You sign in through a LeetCode window inside the app, and the login is kept in your system's credential store.
            </span>
          </div>
          <div class="key-row">
            {#if leetcode.status === null}
              <span class="who muted" role="status">Checking…</span>
            {:else if leetcode.signingIn}
              <span class="who" role="status"><span class="spinner" aria-hidden="true"></span>Waiting for you to sign in…</span>
            {:else if leetcode.expired}
              <span class="who warn" role="status"><span class="status-dot warn" aria-hidden="true"></span>Your session expired</span>
              <button type="button" class="small-btn strong" onclick={() => leetcode.signIn()}>Sign in again</button>
            {:else if leetcode.status.signedIn}
              <span class="who" role="status">
                <span class="status-dot ok" aria-hidden="true"></span>
                <span class="name">Signed in{leetcode.status.username ? ` as ${leetcode.status.username}` : ""}</span>
              </span>
              <button type="button" class="small-btn" onclick={() => leetcode.signOut()}>Sign out</button>
            {:else}
              <span class="who muted" role="status"><span class="status-dot" aria-hidden="true"></span>Not signed in</span>
              <button type="button" class="small-btn strong" onclick={() => leetcode.signIn()}>Sign in to LeetCode</button>
            {/if}
          </div>
          {#if leetcode.authError}
            <p class="error" role="alert">{leetcode.authError}</p>
          {/if}
        </div>
        {/if}

        {#if settings.error}
          <p class="error" role="alert">{settings.error}</p>
        {/if}
      </div>
    </div>
  </dialog>
</div>

<style>
  /* The same small card as the nav above it, pinned to the bottom of the left strip. */
  .settings {
    --item: 36px;
    --pad: 5px;

    position: absolute;
    z-index: 3;
    bottom: 40px;
    left: calc((var(--strip) - var(--item) - 2 * var(--pad) - 2px) / 2);
    padding: var(--pad);
    box-sizing: border-box;
    border: 1px solid var(--card-border);
    border-radius: var(--card-radius);
    background-color: var(--card-bg);
    box-shadow: var(--card-shadow);
    animation: settings-in 0.5s cubic-bezier(0.2, 0.7, 0.2, 1) 0.05s backwards;
  }

  @keyframes settings-in {
    from {
      opacity: 0;
      transform: translateX(-14px);
    }
  }

  .settings-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--item);
    height: var(--item);
    padding: 0;
    border: 0;
    border-radius: 12px;
    background: none;
    color: #8b93a7;
    cursor: pointer;
    transition:
      color 0.25s,
      background-color 0.15s;
  }

  .settings-btn:hover {
    color: #e6e8ee;
    background-color: rgba(255, 255, 255, 0.05);
  }

  .settings-btn:focus-visible,
  .close:focus-visible,
  .switch:focus-visible,
  .stepper button:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* Custom tooltip, like the nav's. */
  .tooltip {
    position: absolute;
    left: calc(100% + 16px);
    top: 50%;
    transform: translate(-4px, -50%);
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.06);
    background-color: #14171c;
    box-shadow: 4px 4px 16px rgba(0, 0, 0, 0.4);
    color: #e6e8ee;
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition:
      opacity 0.15s,
      transform 0.15s;
  }

  .settings-btn:hover .tooltip,
  .settings-btn:focus-visible .tooltip {
    opacity: 1;
    transform: translate(0, -50%);
  }

  /* ---- The modal ---- */
  .modal {
    width: min(480px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    margin: auto;
    padding: 0;
    overflow: hidden;
    color: inherit;
  }

  .modal[open] {
    display: flex;
    animation: modal-in 0.18s ease-out;
  }

  .modal::backdrop {
    background-color: rgba(10, 12, 16, 0.62);
    backdrop-filter: blur(2px);
    animation: backdrop-in 0.18s ease-out;
  }

  @keyframes modal-in {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.98);
    }
  }

  @keyframes backdrop-in {
    from {
      opacity: 0;
    }
  }

  .panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 20px 8px 24px;
  }

  /* The sections. The line under them is the header's divider, with the open tab's mark on it. */
  .tabs {
    display: flex;
    gap: 2px;
    padding: 0 16px;
    border-bottom: 1px solid var(--card-border);
  }

  .tab {
    position: relative;
    padding: 9px 12px 11px;
    border: 0;
    border-radius: 8px 8px 0 0;
    background: none;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    color: #8b93a7;
    cursor: pointer;
    transition: color 0.15s;
  }

  .tab:hover,
  .tab[aria-selected="true"] {
    color: #edeae3;
  }

  .tab::after {
    content: "";
    position: absolute;
    right: 12px;
    bottom: -1px;
    left: 12px;
    height: 2px;
    border-radius: 2px;
    background: #e9bc22;
    opacity: 0;
    transform: scaleX(0.4);
    transition:
      opacity 0.2s,
      transform 0.2s;
  }

  .tab[aria-selected="true"]::after {
    opacity: 1;
    transform: none;
  }

  /* The AI Coach's tab is marked in the AI gradient, like everything else from the AI. */
  .tab.ai::after {
    background: var(--ai-flow);
    background-size: 200% 100%;
    animation: ai-flow var(--ai-drift) linear infinite;
  }

  .tab:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: -2px;
  }

  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: #edeae3;
  }

  .close {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: none;
    color: #8b93a7;
    cursor: pointer;
  }

  .close:hover {
    background-color: rgba(255, 255, 255, 0.07);
    color: #edeae3;
  }

  .close svg {
    width: 16px;
    height: 16px;
  }

  /* The same height on every tab, so the dialog doesn't jump as you switch; scrolls on its own
     when a tab (the AI Coach's) has more settings than fit. */
  .body {
    box-sizing: border-box;
    height: min(440px, calc(100vh - 210px));
    padding: 4px 24px 8px;
    overflow-y: auto;
  }

  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 16px 0;
  }

  .setting + .setting {
    border-top: 1px solid var(--card-border);
  }

  /* A setting whose control is wide: it goes under its label instead of beside it. */
  .setting.col {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    padding-top: 6px;
    border-top: 0;
  }

  .setting.col:last-child {
    padding-bottom: 12px;
  }

  /* The coach's settings hang off its switch, so they sit closer together. */
  .setting + .setting.col {
    border-top: 0;
  }

  .segments {
    display: flex;
    padding: 3px;
    border: 1px solid var(--card-border);
    border-radius: 10px;
    background-color: #1d2027;
  }

  .segments button {
    flex: 1;
    padding: 6px 10px;
    border: 0;
    border-radius: 7px;
    background: none;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    color: #a8a296;
    cursor: pointer;
    transition:
      background-color 0.15s,
      color 0.15s;
  }

  .segments button:hover:not([aria-checked="true"]) {
    color: #edeae3;
  }

  .segments button[aria-checked="true"] {
    background-color: rgba(233, 188, 34, 0.16);
    color: #edeae3;
  }

  .field {
    box-sizing: border-box;
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--card-border);
    border-radius: 10px;
    background-color: #1d2027;
    font: inherit;
    font-size: 13px;
    color: #edeae3;
    transition: border-color 0.2s;
  }

  .field::placeholder {
    color: #6f6a60;
  }

  .field:focus-visible {
    outline: 0;
    border-color: rgba(233, 188, 34, 0.6);
  }

  .field:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .segments button:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 1px;
  }

  .key-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .key-row .field {
    flex: 1;
    min-width: 0;
  }

  /* The LeetCode account row: who's signed in, and the button for what you can do about it. */
  .who {
    flex: 1;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 13px;
    font-weight: 500;
    color: #c9c2b4;
  }

  .who.muted {
    color: #8b93a7;
  }

  .who.warn {
    color: #e9bc22;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status-dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background-color: #6f7a94;
  }

  .status-dot.ok {
    background-color: #3fae7f;
  }

  .status-dot.warn {
    background-color: #e9bc22;
  }

  .key-saved {
    flex: 1;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 500;
    color: #5fd0a0;
  }

  .key-saved svg {
    width: 14px;
    height: 14px;
  }

  .small-btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    flex: none;
    padding: 7px 14px;
    border: 1px solid var(--card-border);
    border-radius: 9px;
    background-color: #1d2027;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    color: #c9c2b4;
    cursor: pointer;
    transition:
      border-color 0.2s,
      color 0.15s;
  }

  .small-btn:hover:not(:disabled) {
    border-color: rgba(255, 255, 255, 0.2);
    color: #edeae3;
  }

  .small-btn.strong {
    border-color: rgba(233, 188, 34, 0.5);
    color: #edeae3;
  }

  .small-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .small-btn:focus-visible {
    outline: 2px solid #e9bc22;
    outline-offset: 2px;
  }

  /* Reset: set apart from the ordinary settings above it, in the red the app uses for "this costs you". */
  .setting + .setting.col.danger {
    margin-top: 6px;
    padding-top: 16px;
    border-top: 1px solid var(--card-border);
  }

  .small-btn.danger-btn {
    border-color: rgba(255, 138, 138, 0.35);
    color: #ff9a9a;
  }

  .small-btn.danger-btn:hover:not(:disabled) {
    border-color: rgba(255, 138, 138, 0.7);
    background-color: rgba(255, 85, 85, 0.1);
    color: #ffb0b0;
  }

  .small-btn.danger-btn.strong {
    background-color: rgba(255, 85, 85, 0.14);
  }

  .reset-box {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid rgba(255, 138, 138, 0.3);
    border-radius: 10px;
    background-color: rgba(255, 85, 85, 0.05);
  }

  .reset-box p,
  .reset-box ul {
    margin: 0;
  }

  .reset-lead {
    font-size: 13px;
    font-weight: 600;
    color: #edeae3;
  }

  .reset-box ul {
    padding-left: 18px;
    font-size: 12.5px;
    line-height: 19px;
    color: #c9c2b4;
  }

  .reset-box .key-row {
    justify-content: flex-end;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 12.5px;
    line-height: 18px;
    color: #c9c2b4;
    cursor: pointer;
  }

  .check .help {
    display: block;
    margin-top: 2px;
  }

  .check input {
    flex: none;
    margin: 2px 0 0;
    accent-color: #e9bc22;
  }

  .prices {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .prices label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ok {
    margin: 0;
    font-size: 12.5px;
    line-height: 17px;
    color: #5fd0a0;
  }

  .setting.col .error {
    margin: 0;
  }

  .spinner {
    box-sizing: border-box;
    width: 12px;
    height: 12px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: settings-spin 0.8s linear infinite;
  }

  @keyframes settings-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .label {
    font-size: 14px;
    font-weight: 500;
    color: #edeae3;
  }

  .help {
    font-size: 12.5px;
    line-height: 17px;
    color: #8b93a7;
  }

  /* On/off switch. */
  .switch {
    flex: none;
    position: relative;
    width: 38px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background-color: #3a3f4b;
    cursor: pointer;
    transition: background-color 0.2s;
  }

  .switch[aria-checked="true"] {
    background-color: #e9bc22;
  }

  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background-color: #edeae3;
    transition: transform 0.2s cubic-bezier(0.34, 1.3, 0.64, 1);
  }

  .switch[aria-checked="true"] .knob {
    transform: translateX(16px);
    background-color: #1d2027;
  }

  /* Number stepper. */
  .stepper {
    flex: none;
    display: flex;
    align-items: center;
    border: 1px solid var(--card-border);
    border-radius: 999px;
    background-color: #1d2027;
  }

  .stepper button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: #a8a296;
    cursor: pointer;
  }

  .stepper button:hover:not(:disabled) {
    background-color: rgba(255, 255, 255, 0.08);
    color: #edeae3;
  }

  .stepper button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .stepper svg {
    width: 12px;
    height: 12px;
  }

  output {
    min-width: 26px;
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-align: center;
    color: #edeae3;
  }

  .error {
    margin: 0 0 12px;
    font-size: 12.5px;
    line-height: 17px;
    color: #ff6b6b;
  }

  @media (prefers-reduced-motion: reduce) {
    .settings,
    .modal[open],
    .modal::backdrop {
      animation: none;
    }

    .knob,
    .tab::after {
      transition: none;
    }

    .tab.ai::after {
      animation: none;
    }

    .spinner {
      animation-duration: 2.4s;
    }
  }
</style>
